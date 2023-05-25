use crate::DummyVM;
use crate::edges::DummyVMEdge;
use mmtk::util::opaque_pointer::*;
use mmtk::util::ObjectReference;
use mmtk::vm::EdgeVisitor;
use mmtk::vm::RootsWorkFactory;
use mmtk::vm::Scanning;
use mmtk::Mutator;

pub struct VMScanning {}

impl Scanning<DummyVM> for VMScanning {
    fn scan_thread_roots(_tls: VMWorkerThread, _factory: impl RootsWorkFactory<DummyVMEdge>) {
        unimplemented!()
    }
    fn scan_thread_root(
        _tls: VMWorkerThread,
        _mutator: &'static mut Mutator<DummyVM>,
        _factory: impl RootsWorkFactory<DummyVMEdge>,
    ) {
        unimplemented!()
    }
    fn scan_vm_specific_roots(_tls: VMWorkerThread, _factory: impl RootsWorkFactory<DummyVMEdge>) {
        unimplemented!()
    }
    fn scan_object<EV: EdgeVisitor<DummyVMEdge>>(
        _tls: VMWorkerThread,
        _object: ObjectReference,
        _edge_visitor: &mut EV,
    ) {
        unimplemented!()
    }
    // fn scan_object_with_klass(
    //         _tls: VMWorkerThread,
    //         _object: ObjectReference,
    //         _edge_visitor: &mut impl EdgeVisitor<<DummyVM as mmtk::vm::VMBinding>::VMEdge>,
    //         _klass: mmtk::util::Address,
    //     ) {
    //     unimplemented!()
    // }
    fn notify_initial_thread_scan_complete(_partial_scan: bool, _tls: VMWorkerThread) {
        unimplemented!()
    }
    fn supports_return_barrier() -> bool {
        unimplemented!()
    }
    fn prepare_for_roots_re_scanning() {
        unimplemented!()
    }
}
