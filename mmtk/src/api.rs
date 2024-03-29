// All functions here are extern function. There is no point for marking them as unsafe.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use libc::c_char;
use libc::c_void;
use log::debug;
use log::warn;
use mmtk::memory_manager::is_mmtk_object;
use mmtk::util::alloc::AllocatorInfo;
use mmtk::util::alloc::AllocatorSelector;
use mmtk::util::constants;
use mmtk::util::options::GCTriggerSelector;
use mmtk::util::options::PlanSelector;
use mmtk::vm::EdgeVisitor;
use mmtk::vm::edge_shape::SimpleEdge;
use core::panic;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::ffi::CStr;
use std::sync::mpsc;
use std::thread;
use mmtk::memory_manager;
use mmtk::AllocationSemantics;
use mmtk::util::{ObjectReference, Address};
use mmtk::util::opaque_pointer::*;
use mmtk::scheduler::{GCController, GCWorker};
use mmtk::Mutator;
use crate::MutatorClosure;
use crate::ScalaNative;
use crate::SINGLETON;
use crate::BUILDER;
use crate::ScalaNativeUpcalls;
use crate::UPCALLS;
use crate::abi::Object;
use crate::binding::ScalaNativeBinding;
use crate::edges::ScalaNativeEdge;
use crate::object_scanning::ClosureWrapper;
use crate::scanning::HANDLER_FN;

#[no_mangle]
pub extern "C" fn mmtk_init(min_heap_size: usize, max_heap_size: usize) {
    // set heap size first
    {
        let mut builder = BUILDER.lock().unwrap();
        let policy = if min_heap_size == max_heap_size {
            GCTriggerSelector::FixedHeapSize(min_heap_size)
        } else {
            GCTriggerSelector::DynamicHeapSize(min_heap_size, max_heap_size)
        };
        builder.options.plan.set(PlanSelector::Immix);
        let success = builder.options.gc_trigger.set(policy);
        debug_assert!(success, "Failed to set min heap size to {} and max heap size to {}", min_heap_size, max_heap_size);
    }

    // Make sure MMTk has not yet been initialized
    debug_assert!(!crate::MMTK_INITIALIZED.load(Ordering::SeqCst));
    // Initialize MMTk here
    lazy_static::initialize(&SINGLETON);
}

#[no_mangle]
pub extern "C" fn mmtk_get_bytes_in_page() -> usize {
    constants::BYTES_IN_PAGE
}

#[cfg(feature = "object_pinning")]
#[no_mangle]
pub extern "C" fn mmtk_pin_object(addr: *mut word_t) -> bool {
    memory_manager::pin_object::<ScalaNative>(unsafe { ObjectReference::from_raw_address(Address::from_mut_ptr(addr)) })
}

#[cfg(feature = "object_pinning")]
#[no_mangle]
pub extern "C" fn mmtk_append_pinned_objects(data: *const *const usize, len: size_t) {
    let mut vec = unsafe { 
        std::slice::from_raw_parts(data, len)
        .to_vec()
        .iter()
        .map(|x| ObjectReference::from_raw_address(Address::from_usize(*x as usize)))
        .collect() };
    let mut pinned_objects = crate::binding().pinned_objects.lock().unwrap();
    pinned_objects.append(&mut vec)
}

#[no_mangle]
pub extern "C" fn mmtk_init_binding(upcalls: *const ScalaNativeUpcalls) {
    let binding = ScalaNativeBinding::new(&SINGLETON, upcalls);
    crate::BINDING.set(binding).unwrap_or_else(|_| panic!("Binding already initialized"));
}

#[no_mangle]
pub extern "C" fn mmtk_bind_mutator(tls: VMMutatorThread) -> *mut Mutator<ScalaNative> {
    Box::into_raw(memory_manager::bind_mutator(&SINGLETON, tls))
}

#[no_mangle]
pub extern "C" fn mmtk_destroy_mutator(mutator: *mut Mutator<ScalaNative>) {
    // notify mmtk-core about destroyed mutator
    memory_manager::destroy_mutator(unsafe { &mut *mutator });
    // turn the ptr back to a box, and let Rust properly reclaim it
    let _ = unsafe { Box::from_raw(mutator) };
}

#[no_mangle]
pub extern "C" fn mmtk_flush_mutator(mutator: *mut Mutator<ScalaNative>) {
    memory_manager::flush_mutator(unsafe { &mut *mutator });
}

#[no_mangle]
pub extern "C" fn mmtk_alloc(mutator: *mut Mutator<ScalaNative>, size: usize,
                    align: usize, offset: usize, semantics: AllocationSemantics) -> Address {
    memory_manager::alloc::<ScalaNative>(unsafe { &mut *mutator }, size, align, offset, semantics)
}

#[no_mangle]
pub extern "C" fn mmtk_post_alloc(mutator: *mut Mutator<ScalaNative>, refer: ObjectReference,
                                        bytes: usize, mut semantics: AllocationSemantics) {
    if bytes >= SINGLETON.get_plan().constraints().max_non_los_default_alloc_bytes {
        semantics = AllocationSemantics::Los;
    }
    memory_manager::post_alloc::<ScalaNative>(unsafe { &mut *mutator }, refer, bytes, semantics)
}

#[no_mangle]
pub extern "C" fn mmtk_will_never_move(object: ObjectReference) -> bool {
    !object.is_movable()
}

#[no_mangle]
pub extern "C" fn mmtk_start_control_collector(tls: VMWorkerThread, controller: &'static mut GCController<ScalaNative>) {
    memory_manager::start_control_collector(&SINGLETON, tls, controller);
}

#[no_mangle]
pub extern "C" fn mmtk_start_worker(tls: VMWorkerThread, worker: &'static mut GCWorker<ScalaNative>) {
    memory_manager::start_worker::<ScalaNative>(&SINGLETON, tls, worker)
}

#[no_mangle]
pub extern "C" fn mmtk_initialize_collection(tls: VMThread) {
    memory_manager::initialize_collection(&SINGLETON, tls)
}

#[no_mangle]
pub extern "C" fn mmtk_disable_collection() {
    memory_manager::disable_collection(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_enable_collection() {
    memory_manager::enable_collection(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_used_bytes() -> usize {
    memory_manager::used_bytes(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_free_bytes() -> usize {
    memory_manager::free_bytes(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_total_bytes() -> usize {
    memory_manager::total_bytes(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_is_live_object(object: ObjectReference) -> bool {
    memory_manager::is_live_object(object)
}

#[no_mangle]
pub extern "C" fn mmtk_is_reachable(object: ObjectReference) -> bool {
    object.is_reachable()
}

#[cfg(feature = "is_mmtk_object")]
#[no_mangle]
pub extern "C" fn mmtk_is_mmtk_object(addr: Address) -> bool {
    memory_manager::is_mmtk_object(addr)
}

#[no_mangle]
pub extern "C" fn mmtk_is_aligned_to(addr: Address, align: usize) -> bool {
    addr.is_aligned_to(align)
}

#[no_mangle]
pub extern "C" fn mmtk_is_in_mmtk_spaces(object: ObjectReference) -> bool {
    memory_manager::is_in_mmtk_spaces::<ScalaNative>(object)
}

#[no_mangle]
pub extern "C" fn mmtk_is_mapped_address(address: Address) -> bool {
    memory_manager::is_mapped_address(address)
}

#[no_mangle]
pub extern "C" fn mmtk_handle_user_collection_request(tls: VMMutatorThread) {
    memory_manager::handle_user_collection_request::<ScalaNative>(&SINGLETON, tls);
    // memory_manager::handle_user_collection_request::<ScalaNative>(&SINGLETON, tls, false);
}

#[no_mangle]
pub extern "C" fn mmtk_add_weak_candidate(reff: ObjectReference) {
    memory_manager::add_weak_candidate(&SINGLETON, reff)
}

#[no_mangle]
pub extern "C" fn mmtk_add_soft_candidate(reff: ObjectReference) {
    memory_manager::add_soft_candidate(&SINGLETON, reff)
}

#[no_mangle]
pub extern "C" fn mmtk_add_phantom_candidate(reff: ObjectReference) {
    memory_manager::add_phantom_candidate(&SINGLETON, reff)
}

#[no_mangle]
pub extern "C" fn mmtk_harness_begin(tls: VMMutatorThread) {
    memory_manager::harness_begin(&SINGLETON, tls)
}

#[no_mangle]
pub extern "C" fn mmtk_harness_end() {
    memory_manager::harness_end(&SINGLETON)
}

#[no_mangle]
pub extern "C" fn mmtk_process(name: *const c_char, value: *const c_char) -> bool {
    let name_str: &CStr = unsafe { CStr::from_ptr(name) };
    let value_str: &CStr = unsafe { CStr::from_ptr(value) };
    let mut builder = BUILDER.lock().unwrap();
    memory_manager::process(&mut builder, name_str.to_str().unwrap(), value_str.to_str().unwrap())
}

#[no_mangle]
pub extern "C" fn mmtk_starting_heap_address() -> Address {
    memory_manager::starting_heap_address()
}

#[no_mangle]
pub extern "C" fn mmtk_last_heap_address() -> Address {
    memory_manager::last_heap_address()
}

#[no_mangle]
#[cfg(feature = "malloc_counted_size")]
pub extern "C" fn mmtk_counted_malloc(size: usize) -> Address {
    memory_manager::counted_malloc::<ScalaNative>(&SINGLETON, size)
}
#[no_mangle]
pub extern "C" fn mmtk_malloc(size: usize) -> Address {
    memory_manager::malloc(size)
}

#[no_mangle]
#[cfg(feature = "malloc_counted_size")]
pub extern "C" fn mmtk_counted_calloc(num: usize, size: usize) -> Address {
    memory_manager::counted_calloc::<ScalaNative>(&SINGLETON, num, size)
}
#[no_mangle]
pub extern "C" fn mmtk_calloc(num: usize, size: usize) -> Address {
    memory_manager::calloc(num, size)
}

#[no_mangle]
#[cfg(feature = "malloc_counted_size")]
pub extern "C" fn mmtk_realloc_with_old_size(addr: Address, size: usize, old_size: usize) -> Address {
    memory_manager::realloc_with_old_size::<ScalaNative>(&SINGLETON, addr, size, old_size)
}
#[no_mangle]
pub extern "C" fn mmtk_realloc(addr: Address, size: usize) -> Address {
    memory_manager::realloc(addr, size)
}

#[no_mangle]
#[cfg(feature = "malloc_counted_size")]
pub extern "C" fn mmtk_free_with_size(addr: Address, old_size: usize) {
    memory_manager::free_with_size::<ScalaNative>(&SINGLETON, addr, old_size)
}
#[no_mangle]
pub extern "C" fn mmtk_free(addr: Address) {
    memory_manager::free(addr)
}

#[no_mangle]
pub extern "C" fn get_max_non_los_default_alloc_bytes() -> usize {
    SINGLETON
        .get_plan()
        .constraints()
        .max_non_los_default_alloc_bytes
}

// Define types for our requests and responses
pub enum SyncRequest {
    Acquire(VMWorkerThread),
    Release(VMWorkerThread),
}

pub enum SyncResponse {
    Acquired,
    Released,
}

// Define global channels
lazy_static! {
    pub static ref REQ_SENDER: Mutex<mpsc::Sender<SyncRequest>> = {
        let (sender, _) = mpsc::channel();
        Mutex::new(sender)
    };
    pub static ref RES_RECEIVER: Mutex<mpsc::Receiver<SyncResponse>> = {
        let (_, receiver) = mpsc::channel();
        Mutex::new(receiver)
    };
}

#[no_mangle]
pub extern "C" fn scalanative_gc_init(calls: *const ScalaNativeUpcalls) {
    unsafe { UPCALLS = calls };
    // Create channels for request and response
    let (req_tx, req_rx) = mpsc::channel::<SyncRequest>();
    let (res_tx, res_rx) = mpsc::channel::<SyncResponse>();

    // Overwrite the global channels with the ones we just created
    *REQ_SENDER.lock().unwrap() = req_tx;
    *RES_RECEIVER.lock().unwrap() = res_rx;

    // Spawn a dedicated thread that owns the lock
    thread::Builder::new()
        .name("MMTk Synchronizer Thread".to_string())
        .spawn(move || {
            debug!("Hello! This is MMTk Synchronizer Thread running!");
            crate::register_gc_thread(thread::current().id());
            unsafe { ((*UPCALLS).init_synchronizer_thread)()};
    
            let lock = Mutex::new(());
            for req in req_rx {
                match req {
                    SyncRequest::Acquire(_tls) => {
                        let _guard = lock.lock().unwrap();
                        unsafe { ((*UPCALLS).stop_all_mutators)(_tls) };
                        res_tx.send(SyncResponse::Acquired).unwrap();
                    }
                    SyncRequest::Release(tls) => {
                        unsafe { ((*UPCALLS).resume_mutators)(tls) };
                        res_tx.send(SyncResponse::Released).unwrap();
                    }
                }
            }
    
            // Currently the MMTk controller thread should run forever.
            // This is an unlikely event, but we log it anyway.
            warn!("The MMTk Controller Thread is quitting!");
            crate::unregister_gc_thread(thread::current().id());
        })
        .unwrap();
}

/// # Safety
/// Caller needs to make sure the ptr is a valid vector pointer.
#[no_mangle]
pub unsafe extern "C" fn release_buffer(ptr: *mut *mut Object, length: usize, capacity: usize) {
    // Take ownership and then drop it
    let _vec = Vec::<*mut Object>::from_raw_parts(ptr, length, capacity);
}

#[no_mangle]
pub extern "C" fn invoke_mutator_closure(closure: *mut MutatorClosure, mutator: *mut Mutator<ScalaNative>) {
    // println!("invoke_mutator_closure on mutator: {:p}", mutator);
    let closure = unsafe { &mut *closure };
    // println!("closure: {:p}", closure);
    // println!("closure.func: {:p}", closure.func);
    // println!("closure.data: {:p}", closure.data.0);
    (closure.func)(mutator, &closure.data);
}

#[no_mangle]
pub extern "C" fn visit_edge(closure_ptr: *mut std::ffi::c_void, edge: Address) {
    let closure = unsafe { &mut *(closure_ptr as *mut ClosureWrapper<ScalaNativeEdge>) };
    if is_mmtk_object(edge) {
        let simple_edge = SimpleEdge::from_address(edge);
        closure.visit_edge(simple_edge);
    }
}

#[no_mangle]
pub extern "C" fn get_immix_bump_ptr_offset() -> usize {
    let AllocatorInfo::BumpPointer {
        bump_pointer_offset,
    } = AllocatorInfo::new::<ScalaNative>(AllocatorSelector::Immix(0)) else {
        panic!("Expected BumpPointer");
    };
    bump_pointer_offset
}

#[no_mangle]
pub extern "C" fn get_vo_bit_log_region_size() -> usize {
    // TODO: Fix mmtk-core to make the log region size public
    mmtk::util::is_mmtk_object::VO_BIT_REGION_SIZE.trailing_zeros() as usize
}

#[no_mangle]
pub extern "C" fn get_vo_bit_base() -> usize {
    mmtk::util::metadata::side_metadata::VO_BIT_SIDE_METADATA_ADDR.as_usize()
}

#[no_mangle]
pub extern "C" fn mmtk_weak_ref_stack_set_handler(handler: *mut c_void) {
    let handler_fn = unsafe { std::mem::transmute::<*mut c_void, fn()>(handler) };
    *HANDLER_FN.lock().unwrap() = Some(handler_fn);
}
