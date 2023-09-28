use std::{mem, fmt::Display, slice, sync::Mutex};
use mmtk::{vm::{EdgeVisitor, edge_shape::Edge}, util::{constants::LOG_BYTES_IN_ADDRESS, ObjectReference, VMWorkerThread, Address}, memory_manager::{is_mmtk_object, self}};
use crate::{abi::*, edges::ScalaNativeEdge, UPCALLS};
use crate::scanning::mmtk_scan_field;

const LAST_FIELD_OFFSET: i64 = -1;
lazy_static! {
	static ref __object_array_id: Mutex<i32> = Mutex::new(unsafe {
		((*UPCALLS).get_object_array_id)()
	});
}
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

fn is_word_in_heap(word: Address) -> bool {
	word >= memory_manager::starting_heap_address() && word <= memory_manager::last_heap_address()
}

// Modify the ClosureWrapper to hold an EdgeVisitor
pub struct ClosureWrapper<'a, ES: Edge> {
	closure: &'a mut dyn EdgeVisitor<ES>,
}

impl<'a, ES: Edge> EdgeVisitor<ES> for ClosureWrapper<'a, ES> {
	fn visit_edge(&mut self, edge: ES) {
			self.closure.visit_edge(edge);
	}
}

impl ObjIterate for Object {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
		let ptr_map: *mut i64 = unsafe { (*(self.rtti)).ref_map_struct };
		let mut i = 0;
		unsafe {
			while *ptr_map.offset(i) != LAST_FIELD_OFFSET {
				let offset = *ptr_map.offset(i);
				if self.is_referant_of_weak_reference(offset.try_into().unwrap()) {
					i += 1;
					continue
				}
				let field = self.fields[offset as usize];
				mmtk_scan_field(field, closure);
				i += 1;
			}
		}
	}
}

impl ObjIterate for ArrayHeader {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
		let fields: *mut *mut word_t = 
			((self as *const _ as usize) + std::mem::size_of::<ArrayHeader>()) as *mut *mut word_t;
		if unsafe{ ( *(self.rtti) ).rt.id } == *__object_array_id.lock().unwrap() {
			for i in 0..self.length {
				let field = unsafe { *(fields.offset(i as isize)) };
				mmtk_scan_field(field, closure);
			}
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
