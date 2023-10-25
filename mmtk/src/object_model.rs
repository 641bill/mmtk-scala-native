use log::{trace, info};
use mmtk::util::copy::{CopySemantics, GCWorkerCopyContext};
use mmtk::util::{Address, ObjectReference};
use mmtk::vm::*;
use crate::{ScalaNative, UPCALLS};
use crate::abi::Obj;

pub struct VMObjectModel {}

// This is intentionally set to a non-zero value to see if it breaks.
// Change this if you want to test other values.
pub const OBJECT_REF_OFFSET: usize = 0;

impl ObjectModel<ScalaNative> for VMObjectModel {
    const GLOBAL_LOG_BIT_SPEC: VMGlobalLogBitSpec = VMGlobalLogBitSpec::side_first();
    // The forwarding pointer can be anywhere in the from-space object because once the object is moved, 
    // its from-space copy is "condemned", i.e. it's fields must not be read or written again, 
    // and it will be "wrecked" (i.e. recycled) soon
    const LOCAL_FORWARDING_POINTER_SPEC: VMLocalForwardingPointerSpec = VMLocalForwardingPointerSpec::in_header(0);
    // Use the last two bits in the object header
    const LOCAL_FORWARDING_BITS_SPEC: VMLocalForwardingBitsSpec = VMLocalForwardingBitsSpec::in_header(-2);
    const LOCAL_MARK_BIT_SPEC: VMLocalMarkBitSpec = VMLocalMarkBitSpec::side_first();
    const LOCAL_LOS_MARK_NURSERY_SPEC: VMLocalLOSMarkNurserySpec = VMLocalLOSMarkNurserySpec::side_after(Self::LOCAL_MARK_BIT_SPEC.as_spec());

    const OBJECT_REF_OFFSET_LOWER_BOUND: isize = OBJECT_REF_OFFSET as isize;
    const NEED_VO_BITS_DURING_TRACING: bool = true;

    #[cfg(feature = "object_pinning")]
    const LOCAL_PINNING_BIT_SPEC: VMLocalPinningBitSpec = VMLocalPinningBitSpec::side_after(Self::LOCAL_LOS_MARK_NURSERY_SPEC.as_spec());
   
    fn copy(
        from: ObjectReference,
        semantics: CopySemantics,
        copy_context: &mut GCWorkerCopyContext<ScalaNative>,
    ) -> ObjectReference {
        let bytes = Obj::from(from).size();
        let dst = copy_context.alloc_copy(from, bytes, unsafe { ((*UPCALLS).get_allocation_alignment)() }, 0, semantics);
        let src = from.to_raw_address();
        unsafe { std::ptr::copy_nonoverlapping::<u8>(src.to_ptr(), dst.to_mut_ptr(), bytes) }
        let to_obj = ObjectReference::from_raw_address(dst);
        copy_context.post_copy(to_obj, bytes, semantics);
        to_obj
    }

    fn copy_to(_from: ObjectReference, _to: ObjectReference, _region: Address) -> Address {
        unimplemented!(
            "We don't support MarkCompact for Scala Native so this function cannot be called."
        )
    }

    fn get_current_size(_object: ObjectReference) -> usize {
        Obj::from(_object).size()
    }

    fn get_size_when_copied(object: ObjectReference) -> usize {
        Self::get_current_size(object)
    }

    fn get_align_when_copied(_object: ObjectReference) -> usize {
        ::std::mem::size_of::<usize>()
    }

    fn get_align_offset_when_copied(_object: ObjectReference) -> usize {
        0
    }

    fn get_reference_when_copied_to(_from: ObjectReference, _to: Address) -> ObjectReference {
        unimplemented!(
            "We don't support MarkCompact for Scala Native so this function cannot be called."
        )
    }

    fn get_type_descriptor(_reference: ObjectReference) -> &'static [i8] {
        todo!()
    }

    fn ref_to_object_start(object: ObjectReference) -> Address {
        object.to_raw_address().sub(OBJECT_REF_OFFSET)
    }

    fn ref_to_header(object: ObjectReference) -> Address {
        object.to_raw_address()
    }

    fn ref_to_address(object: ObjectReference) -> Address {
        // Just use object start.
        Self::ref_to_object_start(object)
    }

    fn address_to_ref(addr: Address) -> ObjectReference {
        ObjectReference::from_raw_address(addr.add(OBJECT_REF_OFFSET))
    }

    fn dump_object(_object: ObjectReference) {
        unimplemented!()
    }
    
    // fn dump_object_s(object: mmtk::util::ObjectReference) -> String {
    //     // This method should return a string representation of the object.
    //     // Replace this with your own implementation.
    //     format!("{:?}", object)
    // }

    // fn get_class_pointer(_object: mmtk::util::ObjectReference) -> mmtk::util::Address {
    //     // This method should return the address of the class of the object.
    //     // Replace this with your own implementation.
    //     unimplemented!()
    // }
}
