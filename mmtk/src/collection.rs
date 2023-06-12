use crate::MutatorClosure;
use crate::ScalaNative;
use crate::UPCALLS;
use mmtk::util::alloc::AllocationError;
use mmtk::util::opaque_pointer::*;
use mmtk::Mutator;
use mmtk::MutatorContext;
use mmtk::vm::{Collection, GCThreadContext, Scanning, VMBinding};


pub struct VMCollection {}

const GC_THREAD_KIND_CONTROLLER: libc::c_int = 0;
const GC_THREAD_KIND_WORKER: libc::c_int = 1;

impl Collection<ScalaNative> for VMCollection {
    fn stop_all_mutators<F>(_tls: VMWorkerThread, mut _mutator_visitor: F)
    // fn stop_all_mutators<F>(_tls: VMWorkerThread, _mutator_visitor: F, _current_gc_should_unload_classes: bool)
    where
        F: FnMut(&'static mut Mutator<ScalaNative>),
    {
        let scan_mutators_in_safepoint =
        <ScalaNative as VMBinding>::VMScanning::SCAN_MUTATORS_IN_SAFEPOINT;

        unsafe {
            ((*UPCALLS).stop_all_mutators)(
                _tls,
                scan_mutators_in_safepoint,
                MutatorClosure::from_rust_closure(&mut _mutator_visitor),
            );
        }
    }

    fn resume_mutators(_tls: VMWorkerThread) {
        unsafe {
            ((*UPCALLS).resume_mutators)(_tls);
        }
    }

    fn block_for_gc(_tls: VMMutatorThread) {
        unsafe {
            ((*UPCALLS).block_for_gc)();
        }
    }

    fn spawn_gc_thread(_tls: VMThread, _ctx: GCThreadContext<ScalaNative>) {
        let (ctx_ptr, kind) = match _ctx {
            GCThreadContext::Controller(c) => (
                Box::into_raw(c) as *mut libc::c_void,
                GC_THREAD_KIND_CONTROLLER,
            ),
            GCThreadContext::Worker(w) => {
                (Box::into_raw(w) as *mut libc::c_void, GC_THREAD_KIND_WORKER)
            }
        };
        unsafe {
            ((*UPCALLS).spawn_gc_thread)(_tls, kind, ctx_ptr);
        }
    }

    fn prepare_mutator<T: MutatorContext<ScalaNative>>(
        _tls_w: VMWorkerThread,
        _tls_m: VMMutatorThread,
        _mutator: &T,
    ) {
        unimplemented!()
    }
    fn out_of_memory(tls: VMThread, err_kind: AllocationError) {
        unsafe {
            ((*UPCALLS).out_of_memory)(tls, err_kind);
        }
    }

    fn schedule_finalization(_tls: VMWorkerThread) {
        unsafe {
            ((*UPCALLS).schedule_finalizer)();
        }
    }
}
