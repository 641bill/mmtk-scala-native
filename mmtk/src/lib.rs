extern crate libc;
extern crate mmtk;
#[macro_use]
extern crate lazy_static;

use abi::ArrayHeader;
use abi::GCThreadTLS;
use abi::MutatorThreadNode;
use abi::Object;
use abi::word_t;
use binding::ScalaNativeBinding;
use collection::SendCtxPtr;
use libc::size_t;
use libc::uintptr_t;
use mmtk::Mutator;
use mmtk::util::Address;
use mmtk::util::alloc::AllocationError;
use mmtk::vm::VMBinding;
use mmtk::MMTKBuilder;
use mmtk::MMTK;
use mmtk::util::opaque_pointer::*;
use once_cell::sync::OnceCell;
use std::ptr::null_mut;
use std::collections::HashSet;

pub mod active_plan;
pub mod api;
pub mod collection;
pub mod object_model;
pub mod reference_glue;
pub mod scanning;
pub mod abi;
pub mod object_scanning;
pub mod binding;

mod edges;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct ScalaNative;

impl VMBinding for ScalaNative {
    type VMObjectModel = object_model::VMObjectModel;
    type VMScanning = scanning::VMScanning;
    type VMCollection = collection::VMCollection;
    type VMActivePlan = active_plan::VMActivePlan;
    type VMReferenceGlue = reference_glue::VMReferenceGlue;
    type VMEdge = edges::ScalaNativeEdge;
    type VMMemorySlice = edges::ScalaNativeMemorySlice;

    /// Allowed maximum alignment in bytes.
    const MIN_ALIGNMENT: usize = 16;
    const MAX_ALIGNMENT: usize = 1 << 6;
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::ThreadId;

/// The singleton object for the ScalaNative binding itself.
pub static BINDING: OnceCell<ScalaNativeBinding> = OnceCell::new();

pub fn binding<'b>() -> &'b ScalaNativeBinding {
    BINDING
        .get()
        .expect("Attempt to use the binding before it's initialization")
}

/// This is used to ensure we initialize MMTk at a specified timing.
pub static MMTK_INITIALIZED: AtomicBool = AtomicBool::new(false);

lazy_static! {
    pub static ref BUILDER: Mutex<MMTKBuilder> = Mutex::new(MMTKBuilder::new());
    pub static ref SINGLETON: MMTK<ScalaNative> = {
        let builder = BUILDER.lock().unwrap();
        debug_assert!(!MMTK_INITIALIZED.load(Ordering::SeqCst));
        let ret = mmtk::memory_manager::mmtk_init(&builder);
        MMTK_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
        *ret
    };
}

#[no_mangle]
pub static GLOBAL_SIDE_METADATA_BASE_ADDRESS: uintptr_t =
    mmtk::util::metadata::side_metadata::GLOBAL_SIDE_METADATA_BASE_ADDRESS.as_usize();

#[no_mangle]
pub static GLOBAL_SIDE_METADATA_VM_BASE_ADDRESS: uintptr_t =
    mmtk::util::metadata::side_metadata::GLOBAL_SIDE_METADATA_VM_BASE_ADDRESS.as_usize();

#[no_mangle]
pub static VO_BIT_ADDRESS: uintptr_t =
    mmtk::util::metadata::side_metadata::VO_BIT_SIDE_METADATA_ADDR.as_usize();

#[no_mangle]
pub static MMTK_MARK_COMPACT_HEADER_RESERVED_IN_BYTES: usize =
    mmtk::util::alloc::MarkCompactAllocator::<ScalaNative>::HEADER_RESERVED_IN_BYTES;

#[no_mangle]
pub static FREE_LIST_ALLOCATOR_SIZE: uintptr_t =
    std::mem::size_of::<mmtk::util::alloc::FreeListAllocator<ScalaNative>>();

#[repr(C)]
pub struct NewBuffer {
    pub ptr: *mut Address,
    pub capacity: usize,
}

use std::marker::PhantomData;

pub struct SendPtr<T>(*mut T, PhantomData<T>);

unsafe impl<T> Send for SendPtr<T> {}

#[repr(C)]
pub struct MutatorClosure {
    pub func: extern "C" fn(mutator: *mut Mutator<ScalaNative>, data: &SendPtr<libc::c_void>),
    pub data: SendPtr<libc::c_void>,
}

impl MutatorClosure {
    fn from_rust_closure<F>(callback: &mut F) -> Self
    where
        F: FnMut(&'static mut Mutator<ScalaNative>),
    {
        Self {
            func: Self::call_rust_closure::<F>,
            data: SendPtr(callback as *mut F as *mut libc::c_void, PhantomData),
        }
    }

    extern "C" fn call_rust_closure<F>(
        mutator: *mut Mutator<ScalaNative>,
        callback_ptr: &SendPtr<libc::c_void>,
    ) where
        F: FnMut(&'static mut Mutator<ScalaNative>),
    {
        let callback: &mut F = unsafe { &mut *(callback_ptr.0 as *mut F) };
        callback(unsafe { &mut *mutator });
    }
}

/// A closure for reporting root edges.  The C++ code should pass `data` back as the last argument.
#[repr(C)]
pub struct EdgesClosure {
    pub func: extern "C" fn(
        buf: *mut Address,
        size: usize,
        cap: usize,
        data: *mut libc::c_void,
    ) -> NewBuffer,
    pub data: *const libc::c_void,
}

#[repr(C)]
pub struct NodesClosure {
    pub func: extern "C" fn(
        buf: *mut Address,
        size: usize,
        cap: usize,
        data: *mut libc::c_void,
    ) -> NewBuffer,
    pub data: *mut libc::c_void,
}

impl Clone for NodesClosure {
    fn clone(&self) -> Self {
        Self {
            func: self.func,
            data: self.data,
        }
    }
}

#[repr(C)]
pub struct StackRange {
    stack_top: *mut *mut usize,
    stack_bottom: *mut *mut usize,
}

#[repr(C)]
pub struct RegsRange {
    regs: *mut *mut usize,
    regs_size: usize,
}

#[repr(C)]
pub struct ScalaNative_Upcalls {
    // collection 
    pub stop_all_mutators: extern "C" fn(
        tls: VMWorkerThread,
        closure: MutatorClosure,
    ),
    pub resume_mutators: extern "C" fn(
        tls: VMWorkerThread,
    ),
    pub block_for_gc: extern "C" fn(),
    pub out_of_memory: extern "C" fn(
        tls: VMThread,
        err_kind: AllocationError,
    ),
    pub schedule_finalizer: extern "C" fn(),
    
    // abi
    pub get_object_array_id: extern "C" fn() -> i32,
    pub get_weak_ref_ids_min: extern "C" fn() -> i32,
    pub get_weak_ref_ids_max: extern "C" fn() -> i32,
    pub get_weak_ref_field_offset: extern "C" fn() -> i32,
    pub get_array_ids_min: extern "C" fn() -> i32,
    pub get_array_ids_max: extern "C" fn() -> i32,
    pub get_allocation_alignment: extern "C" fn() -> size_t,

    // scanning
    pub get_stack_range: extern "C" fn(tls: VMMutatorThread) -> StackRange,
    pub get_regs_range: extern "C" fn(tls: VMMutatorThread) -> RegsRange,
    pub get_modules: extern "C" fn() -> *mut word_t,
    pub get_modules_size: extern "C" fn() -> i32,
    pub get_mutator_threads: extern "C" fn() -> *mut MutatorThreadNode,
    /// Scan all the mutators for roots.
    pub scan_roots_in_all_mutator_threads: extern "C" fn(closure: NodesClosure),
    /// Scan one mutator for roots.
    pub scan_roots_in_mutator_thread: extern "C" fn(closure: NodesClosure, tls: VMMutatorThread),
    pub scan_vm_specific_roots: extern "C" fn(closure: NodesClosure),
    pub prepare_for_roots_re_scanning: extern "C" fn(),
    pub weak_ref_stack_nullify: extern "C" fn(),
    pub weak_ref_stack_call_handlers: extern "C" fn(),

    // active_plan
    pub get_mutators: extern "C" fn(closure: MutatorClosure),
    pub is_mutator: extern "C" fn(tls: VMThread) -> bool,
    pub number_of_mutators: extern "C" fn() -> size_t,
    pub get_mmtk_mutator: extern "C" fn(tls: VMMutatorThread) -> *mut Mutator<ScalaNative>,
    pub init_gc_worker_thread: extern "C" fn(tls: *mut GCThreadTLS, ctx: SendCtxPtr),
    pub get_gc_thread_tls: extern "C" fn() -> *mut GCThreadTLS,
    pub init_synchronizer_thread: extern "C" fn(),
}

pub static mut UPCALLS: *const ScalaNative_Upcalls = null_mut();

pub static GC_THREADS: OnceCell<Mutex<HashSet<ThreadId>>> = OnceCell::new();

pub(crate) fn register_gc_thread(thread_id: ThreadId) {
    let mut gc_threads = GC_THREADS.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap();
    gc_threads.insert(thread_id);
}

pub(crate) fn unregister_gc_thread(thread_id: ThreadId) {
    let mut gc_threads = GC_THREADS.get().unwrap().lock().unwrap();
    gc_threads.remove(&thread_id);
}

pub(crate) fn is_gc_thread(thread_id: ThreadId) -> bool {
    let gc_threads = GC_THREADS.get().unwrap().lock().unwrap();
    gc_threads.contains(&thread_id)
}