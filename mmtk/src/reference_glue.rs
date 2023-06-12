use mmtk::vm::ReferenceGlue;
use mmtk::util::ObjectReference;
use mmtk::util::opaque_pointer::VMWorkerThread;
use crate::ScalaNative;

pub struct VMReferenceGlue {}

impl ReferenceGlue<ScalaNative> for VMReferenceGlue {
    type FinalizableType = ObjectReference;

    fn set_referent(_reference: ObjectReference, _referent: ObjectReference) {
        unimplemented!()
    }
    fn get_referent(_object: ObjectReference) -> ObjectReference {
        unimplemented!()
    }
    fn enqueue_references(_references: &[ObjectReference], _tls: VMWorkerThread) {
        unimplemented!()
    }
}
