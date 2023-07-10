use crate::MutatorClosure;
use crate::SINGLETON;
use crate::ScalaNative;
use crate::api::{SyncRequest, SyncResponse, REQ_SENDER, RES_RECEIVER};
use crate::UPCALLS;
use log::debug;
use log::warn;
use mmtk::memory_manager;
use mmtk::util::alloc::AllocationError;
use mmtk::util::opaque_pointer::*;
use mmtk::Mutator;
use mmtk::MutatorContext;
use mmtk::vm::{Collection, GCThreadContext, Scanning, VMBinding};
use std::thread;
use mmtk::scheduler::*;
use crate::abi::GCThreadTLS;

pub struct VMCollection {}

pub const GC_THREAD_KIND_CONTROLLER: libc::c_int = 0;
pub const GC_THREAD_KIND_WORKER: libc::c_int = 1;

impl Collection<ScalaNative> for VMCollection {
    fn stop_all_mutators<F>(_tls: VMWorkerThread, mut _mutator_visitor: F)
    where
        F: FnMut(&'static mut Mutator<ScalaNative>),
    {
        let result = REQ_SENDER.lock().unwrap().send(SyncRequest::Acquire(_tls, MutatorClosure::from_rust_closure(&mut _mutator_visitor)));
        match result {
            Err(err) => println!("Failed to send message: {:?}", err),
            _ => ()
        }
    }

    fn resume_mutators(_tls: VMWorkerThread) {
        let result = REQ_SENDER.lock().unwrap().send(SyncRequest::Release(_tls));
        match result {
            Err(err) => println!("Failed to send message: {:?}", err),
            _ => ()
        }        
    }

    fn block_for_gc(_tls: VMMutatorThread) {
        unsafe {
            ((*UPCALLS).block_for_gc)();
        }
    }

    fn spawn_gc_thread(_tls: VMThread, ctx: GCThreadContext<ScalaNative>) {
        match ctx {
            GCThreadContext::Controller(mut controller) => {
                thread::Builder::new()
                    .name("MMTk Controller Thread".to_string())
                    .spawn(move || {
                        debug!("Hello! This is MMTk Controller Thread running!");
                        crate::register_gc_thread(thread::current().id());
                        let ptr_controller = &mut *controller as *mut GCController<ScalaNative>;
                        let gc_thread_tls =
                            Box::into_raw(Box::new(GCThreadTLS::for_controller(ptr_controller)));
                        (unsafe { (*UPCALLS).init_gc_worker_thread })(gc_thread_tls);
                        memory_manager::start_control_collector(
                            &SINGLETON,
                            GCThreadTLS::to_vwt(gc_thread_tls),
                            &mut controller,
                        );

                        // Currently the MMTk controller thread should run forever.
                        // This is an unlikely event, but we log it anyway.
                        warn!("The MMTk Controller Thread is quitting!");
                        crate::unregister_gc_thread(thread::current().id());
                    })
                    .unwrap();
            }
            GCThreadContext::Worker(mut worker) => {
                thread::Builder::new()
                    .name("MMTk Worker Thread".to_string())
                    .spawn(move || {
                        debug!("Hello! This is MMTk Worker Thread running!");
                        crate::register_gc_thread(thread::current().id());
                        let ptr_worker = &mut *worker as *mut GCWorker<ScalaNative>;
                        let gc_thread_tls =
                            Box::into_raw(Box::new(GCThreadTLS::for_worker(ptr_worker)));
                        (unsafe { (*UPCALLS).init_gc_worker_thread })(gc_thread_tls);
                        memory_manager::start_worker(
                            &SINGLETON,
                            GCThreadTLS::to_vwt(gc_thread_tls),
                            &mut worker,
                        );

                        // Currently all MMTk worker threads should run forever.
                        // This is an unlikely event, but we log it anyway.
                        warn!("An MMTk Worker Thread is quitting!");
                        crate::unregister_gc_thread(thread::current().id());
                    })
                    .unwrap();
            }
        }
    }

    // fn spawn_gc_thread(_tls: VMThread, _ctx: GCThreadContext<ScalaNative>) {
    //     let (ctx_ptr, kind) = match _ctx {
    //         GCThreadContext::Controller(c) => (
    //             Box::into_raw(c) as *mut libc::c_void,
    //             GC_THREAD_KIND_CONTROLLER,
    //         ),
    //         GCThreadContext::Worker(w) => {
    //             (Box::into_raw(w) as *mut libc::c_void, GC_THREAD_KIND_WORKER)
    //         }
    //     };
    //     unsafe {
    //         ((*UPCALLS).spawn_gc_thread)(_tls, kind, ctx_ptr);
    //     }
    // }

    fn prepare_mutator<T: MutatorContext<ScalaNative>>(
        _tls_w: VMWorkerThread,
        _tls_m: VMMutatorThread,
        _mutator: &T,
    ) {
        // do nothing
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

    fn post_forwarding(_tls: VMWorkerThread) {}
}
