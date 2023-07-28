// All functions here are extern function. There is no point for marking them as unsafe.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use libc::c_char;
use log::debug;
use log::warn;
use mmtk::util::constants;
use mmtk::vm::Scanning;
use mmtk::vm::VMBinding;
use std::sync::Arc;
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
use crate::ScalaNative_Upcalls;
use crate::UPCALLS;
use crate::abi::word_t;

#[no_mangle]
pub extern "C" fn mmtk_init(min_heap_size: usize, max_heap_size: usize) {
    // set heap size first
    {
        let mut builder = BUILDER.lock().unwrap();
        let success = builder.options.gc_trigger.set(mmtk::util::options::GCTriggerSelector::DynamicHeapSize(min_heap_size, max_heap_size));
        assert!(success, "Failed to set min heap size to {} and max heap size to {}", min_heap_size, max_heap_size);
    }

    // Make sure MMTk has not yet been initialized
    assert!(!crate::MMTK_INITIALIZED.load(Ordering::SeqCst));
    // Initialize MMTk here
    lazy_static::initialize(&SINGLETON);
}

#[no_mangle]
pub extern "C" fn mmtk_get_bytes_in_page() -> usize {
    constants::BYTES_IN_PAGE
}

pub extern "C" fn mmtk_pin_object(addr: * mut word_t) -> bool {
    memory_manager::pin_object::<ScalaNative>(addr)
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
                    align: usize, offset: usize, mut semantics: AllocationSemantics) -> Address {
    if size >= SINGLETON.get_plan().constraints().max_non_los_default_alloc_bytes {
        semantics = AllocationSemantics::Los;
    }
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
pub extern "C" fn mmtk_is_live_object(object: ObjectReference) -> bool{
    memory_manager::is_live_object(object)
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
pub extern "C" fn mmtk_modify_check(object: ObjectReference) {
    memory_manager::modify_check(&SINGLETON, object)
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
    Acquire(VMWorkerThread, MutatorClosure),
    Release(VMWorkerThread),
}

pub enum SyncResponse {
    Acquired,
    Released,
}

// Define global channels
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
pub extern "C" fn scalanative_gc_init(calls: *const ScalaNative_Upcalls) {
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
                    SyncRequest::Acquire(_tls, _mutator_visitor) => {
                        let scan_mutators_in_safepoint = <ScalaNative as VMBinding>::VMScanning::SCAN_MUTATORS_IN_SAFEPOINT;
                        let _guard = lock.lock().unwrap();
                        unsafe { ((*UPCALLS).stop_all_mutators)(_tls, scan_mutators_in_safepoint, _mutator_visitor) };
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
pub unsafe extern "C" fn release_buffer(ptr: *mut Address, length: usize, capacity: usize) {
    // Take ownership and then drop it
    let _vec = Vec::<Address>::from_raw_parts(ptr, length, capacity);
}

#[no_mangle]
pub extern "C" fn invoke_mutator_closure(closure: *mut MutatorClosure, mutator: *mut Mutator<ScalaNative>) {
    let closure = unsafe { &mut *closure };
    (closure.func)(mutator, &closure.data);
}