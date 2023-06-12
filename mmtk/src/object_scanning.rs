use std::mem;

use mmtk::{vm::{EdgeVisitor}, util::{constants::LOG_BYTES_IN_ADDRESS, ObjectReference, VMWorkerThread}};

use crate::{abi::*, edges::ScalaNativeEdge};

trait ObjIterate: Sized {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>);
}

impl ObjIterate for Object {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
			// Go through the fields
			let start: mmtk::util::Address = self.get_field_address();
			let fields_size = (unsafe { &*self.rtti }).size as usize - mem::size_of::<Rtti>();
			#[cfg(uses_lockword)]
			let fields_size = size_excluding_rtti - mem::size_of::<word_t>();
			let num_fields = fields_size / mem::size_of::<Field_t>();
			for i in 0..num_fields {
				let edge = start + (i << LOG_BYTES_IN_ADDRESS);
				closure.visit_edge(edge);
			}
	}
}

impl ObjIterate for ArrayHeader {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
			// Go through the elements
			let start: mmtk::util::Address = self.get_element_address(0);
			for i in 0..self.length {
				let edge = start + (i as usize * self.stride as usize);
				closure.visit_edge(edge);
			}
	}
}

fn obj_iterate(obj: Obj, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
	match obj.is_array() {
		true => unsafe { obj.as_array_object().obj_iterate(closure) },
		false => obj.obj_iterate(closure),
	}
}

pub fn scan_object(
	_tls: VMWorkerThread,
	object: ObjectReference,
	closure: &mut impl EdgeVisitor<ScalaNativeEdge>,
) {
	// println!("*****scan_object(0x{:x}) -> \n 0x{:x}, 0x{:x} \n",
	//     object,
	//     unsafe { *(object.value() as *const usize) },
	//     unsafe { *((object.value() + 8) as *const usize) }
	// );
	unsafe { obj_iterate(mem::transmute(object), closure) }
}