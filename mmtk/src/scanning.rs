use crate::EdgesClosure;
use crate::NewBuffer;
use crate::NodesClosure;
use crate::ScalaNative;
use crate::edges::ScalaNativeEdge;
use mmtk::MutatorContext;
use mmtk::util::Address;
use mmtk::util::opaque_pointer::*;
use mmtk::util::ObjectReference;
use mmtk::vm::EdgeVisitor;
use mmtk::vm::RootsWorkFactory;
use mmtk::vm::Scanning;
use mmtk::Mutator;
use crate::UPCALLS;

pub struct VMScanning {}

const WORK_PACKET_CAPACITY: usize = 4096;

extern "C" fn report_edges_and_renew_buffer<F: RootsWorkFactory<ScalaNativeEdge>>(
    ptr: *mut Address,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let buf = unsafe { Vec::<Address>::from_raw_parts(ptr, length, capacity) };
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_edge_roots_work(buf);
    }
    let (ptr, _, capacity) = {
        // TODO: Use Vec::into_raw_parts() when the method is available.
        use std::mem::ManuallyDrop;
        let new_vec = Vec::with_capacity(WORK_PACKET_CAPACITY);
        let mut me = ManuallyDrop::new(new_vec);
        (me.as_mut_ptr(), me.len(), me.capacity())
    };
    NewBuffer { ptr, capacity }
}

pub(crate) fn to_edges_closure<F: RootsWorkFactory<ScalaNativeEdge>>(factory: &mut F) -> EdgesClosure {
    EdgesClosure {
        func: report_edges_and_renew_buffer::<F>,
        data: factory as *mut F as *mut libc::c_void,
    }
}

extern "C" fn report_nodes_and_renew_buffer<F: RootsWorkFactory<ScalaNativeEdge>>(
    ptr: *mut Address,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let address_buf = unsafe { Vec::<Address>::from_raw_parts(ptr, length, capacity) };
        let buf = address_buf.into_iter().map(|addr| ObjectReference::from_raw_address(addr)).collect();
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_node_roots_work(buf);
    }
    let (ptr, _, capacity) = {
        use std::mem::ManuallyDrop;
        let new_vec = Vec::with_capacity(WORK_PACKET_CAPACITY);
        let mut me = ManuallyDrop::new(new_vec);
        (me.as_mut_ptr(), me.len(), me.capacity())
    };
    NewBuffer { ptr, capacity }
}

pub(crate) fn to_nodes_closure<F: RootsWorkFactory<ScalaNativeEdge>>(factory: &mut F) -> NodesClosure {
    NodesClosure {
        func: report_nodes_and_renew_buffer::<F>,
        data: factory as *mut F as *mut libc::c_void,
    }
}

impl Scanning<ScalaNative> for VMScanning {
    const SCAN_MUTATORS_IN_SAFEPOINT: bool = true;
    const SINGLE_THREAD_MUTATOR_SCANNING: bool = true;

    fn scan_roots_in_all_mutator_threads(_tls: VMWorkerThread, mut _factory: impl RootsWorkFactory<ScalaNativeEdge>) {
        unsafe {
            ((*UPCALLS).scan_roots_in_all_mutator_threads)(to_nodes_closure(&mut _factory));
        }
    }

    fn scan_roots_in_mutator_thread(
        _tls: VMWorkerThread,
        _mutator: &'static mut Mutator<ScalaNative>,
        mut _factory: impl RootsWorkFactory<ScalaNativeEdge>,
    ) {
        let tls = _mutator.get_tls();
        unsafe {
            ((*UPCALLS).scan_roots_in_mutator_thread)(to_nodes_closure(&mut _factory), tls);
        }
    }

    fn scan_vm_specific_roots(_tls: VMWorkerThread, mut _factory: impl RootsWorkFactory<ScalaNativeEdge>) {
        unsafe {
            ((*UPCALLS).scan_vm_specific_roots)(to_nodes_closure(&mut _factory));
        }
    }

    fn scan_object<EV: EdgeVisitor<ScalaNativeEdge>>(
        _tls: VMWorkerThread,
        _object: ObjectReference,
        _edge_visitor: &mut EV,
    ) {
        crate::object_scanning::scan_object(_tls, _object, _edge_visitor);
    }

    // fn scan_object_with_klass(
    //         _tls: VMWorkerThread,
    //         _object: ObjectReference,
    //         _edge_visitor: &mut impl EdgeVisitor<<ScalaNative as mmtk::vm::VMBinding>::VMEdge>,
    //         _klass: mmtk::util::Address,
    //     ) {
    //     unimplemented!()
    // }
    fn notify_initial_thread_scan_complete(_partial_scan: bool, _tls: VMWorkerThread) {
        // do nothing
    }

    fn supports_return_barrier() -> bool {
        unimplemented!()
    }

    fn prepare_for_roots_re_scanning() {
        unsafe {
            ((*UPCALLS).prepare_for_roots_re_scanning)();
        }
    }

    fn process_weak_refs(
            _worker: &mut mmtk::scheduler::GCWorker<ScalaNative>,
            _tracer_context: impl mmtk::vm::ObjectTracerContext<ScalaNative>,
        ) -> bool {
        crate::binding().unpin_pinned_objects();
        false
    }
}
