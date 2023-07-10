use std::{mem, fmt::Display, slice};

use mmtk::{vm::{EdgeVisitor}, util::{constants::LOG_BYTES_IN_ADDRESS, ObjectReference, VMWorkerThread, Address}, memory_manager::is_mmtk_object};

use crate::{abi::*, edges::ScalaNativeEdge};

trait ObjIterate: Sized {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>);
}

impl Display for Object {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Display the class name
		let name_str: *mut StringObject = unsafe { std::mem::transmute((&*self.rtti).rt.name) };
		let char_arr: *mut CharArray = unsafe { (*name_str).value };
		let length = unsafe { (*char_arr).header.length as usize };
		let values_slice = unsafe { slice::from_raw_parts((*char_arr).value.as_ptr(), length) };
		write!(f, "Object(0x{:x}), name: [", self as *const _ as usize)?;
		for value in values_slice.iter() {
			write!(f, "{}", (*value as u8) as char)?;
		}
		write!(f, "]")?;
		// Display the layout of the object (offsets of fields)
		let field_address = self.get_field_address();
		let num_fields = self.num_fields();
		write!(f, ", fields: [")?;
		for i in 0..num_fields {
				let field_offset = i * mem::size_of::<Field_t>();
				write!(f, "0x{:x}", field_address + field_offset as usize)?;
				if i < num_fields - 1 {
						write!(f, ", ")?;
				}
		}
		write!(f, "]")?;

		Ok(())
	}
}

impl Display for ArrayHeader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let name_str: *mut StringObject = unsafe { std::mem::transmute((&*self.rtti).rt.name) };
		let char_arr: *mut CharArray = unsafe { (*name_str).value };
		let length = unsafe { (*char_arr).header.length as usize };
		let values_slice = unsafe { slice::from_raw_parts((*char_arr).value.as_ptr(), length) };

		write!(f, "ArrayHeader(0x{:x}), name: [ ", self as *const _ as usize)?;
		for value in values_slice.iter() {
				write!(f, "{}", (*value as u8) as char)?;
		}
		write!(f, "]")?;
		Ok(())
	}
}

impl ObjIterate for Object {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
			// Go through the fields
			let start: mmtk::util::Address = self.get_field_address();
			let num_fields = self.num_fields();
			for i in 0..num_fields {
				let edge = start + (i << LOG_BYTES_IN_ADDRESS);

				// Check if it's null first
				if unsafe { edge.load::<Address>().is_zero() } {
					// println!("Edge: {} is null in object: {}", edge, Address::from_ref(self));
					continue;
				}

				if unsafe { !is_mmtk_object(edge.load::<Address>()) } {
					// println!("Edge: {} with content: {}, is not a valid edge in object: {}, ", edge, unsafe { edge.load::<Address>() }, self);
					continue;
				}

				// println!("Visiting edge: {}, in object: {}, with content: {}", edge, Address::from_ref(self), unsafe { edge.load::<Address>() });
				
				assert!(
					unsafe { is_mmtk_object(edge.load::<Address>()) },
					"{} is not a valid edge but is visited in the object: {}, which is a mmtk object: {}",
					edge, self, is_mmtk_object(Address::from_ref(self))
				);
				closure.visit_edge(edge);
			}
	}
}

impl ObjIterate for ArrayHeader {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
			if self.stride < std::mem::align_of::<Address>().try_into().unwrap() {
				// println!("Iterating through an array of primitives: {}", self);
				return;
			}
			// Go through the elements
			let start: mmtk::util::Address = self.get_element_address(0);
			// println!("Iterating through array: {}, with length: {} and stride: {}", self, self.length, self.stride);
			for i in 0..self.length {
				let edge = start + (i as usize * self.stride as usize);

				// Check if it's null first
				if unsafe { edge.load::<Address>().is_zero() } {
					// println!("Edge: {} is null in array: {}", edge, self);
					continue;
				}

				if unsafe { !is_mmtk_object(edge.load::<Address>()) } {
					// println!("Edge: {} with content: {}, is not a valid edge in array: {} ", edge, unsafe { edge.load::<Address>() }, self);
					continue;
				}

				// println!("Visiting {}_th edge: {}, in array: {}, with content: {}", i, edge, self, unsafe { edge.load::<Address>() });
				
				assert!(
					unsafe { is_mmtk_object(edge.load::<Address>()) },
					"{} is not a valid edge but is visited in an array: {}",
					edge, self
				);
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
