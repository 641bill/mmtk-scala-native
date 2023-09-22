use std::{mem, fmt::Display, slice};
use mmtk::{vm::{EdgeVisitor, edge_shape::Edge}, util::{constants::LOG_BYTES_IN_ADDRESS, ObjectReference, VMWorkerThread, Address}, memory_manager::{is_mmtk_object, self}};
use crate::{abi::*, edges::ScalaNativeEdge};
use crate::UPCALLS;

const LAST_FIELD_OFFSET: i64 = -1;

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
		let mut wrapper = ClosureWrapper { closure };
		let closure_ptr: *mut _ = &mut wrapper;// Convert &self to a raw pointer
		let self_ptr: *const Object = self;
		
		use std::panic::{catch_unwind, AssertUnwindSafe};
		
		let result = catch_unwind(AssertUnwindSafe(|| {
				let closure_ptr: *mut _ = &mut *closure;
				unsafe {
						((*UPCALLS).mmtk_obj_iterate)(self_ptr, closure_ptr as *mut std::ffi::c_void);
				}
		}));
		
		if result.is_err() {
				print!("Scanning Object: {}, contains invalid edges", self);
				panic!("Stop the world")
		}
	}
}

impl ObjIterate for ArrayHeader {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
		let mut wrapper = ClosureWrapper { closure };
		let closure_ptr: *mut _ = &mut wrapper;
		let self_ptr: *const ArrayHeader = self;
		
		unsafe {
				((*UPCALLS).mmtk_array_iterate)(self_ptr, closure_ptr as *mut std::ffi::c_void);
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
