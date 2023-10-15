use std::{mem, fmt::Display, slice, sync::Mutex};
use mmtk::memory_manager::is_mmtk_object;
use mmtk::util::constants::LOG_BYTES_IN_ADDRESS;
use mmtk::{vm::{EdgeVisitor, edge_shape::Edge}, util::{ObjectReference, VMWorkerThread, Address}, memory_manager};
use crate::{abi::*, edges::ScalaNativeEdge, UPCALLS};
use crate::scanning::{is_word_in_heap, WEAK_REF_STACK, ObjectSendPtr};

pub const LAST_FIELD_OFFSET: i64 = -1;
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
		write!(f, ", fields: [")?;
		let ptr_map: *mut i64 = unsafe { (*(self.rtti)).ref_map_struct };
		let mut i = 0;
		let fields = self.get_fields();
		unsafe {
			while *ptr_map.offset(i) != LAST_FIELD_OFFSET {
				let offset: usize = (*ptr_map.offset(i)).try_into().unwrap();
				let edge = fields.offset(offset as isize);
				let node = *edge as *mut usize;
				write!(f, "0x{:x}", node as usize)?;
				if i < (self.num_fields() - 1).try_into().unwrap() {
					write!(f, ", ")?;
				}
				i += 1;
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

		write!(f, "ArrayHeader(0x{:x}), name: [", self as *const _ as usize)?;
		for value in values_slice.iter() {
				write!(f, "{}", (*value as u8) as char)?;
		}
		write!(f, "]")?;
		// Print all the elements
		write!(f, ", elements: [")?;
		let fields: *mut *mut word_t = 
			((self as *const _ as usize) + std::mem::size_of::<ArrayHeader>()) as *mut *mut word_t;
		unsafe {
			for i in 0..self.length {
				let field = *(fields.offset(i as isize));
				write!(f, "0x{:x}", field as usize)?;
				if i < self.length - 1 {
						write!(f, ", ")?;
				}
			}
		}
		write!(f, "]")?;

		Ok(())
	}
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

#[inline]
pub fn mmtk_scan_lock_words(
    object: *mut Object,
    closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
	#[cfg(feature = "uses_lockword")] {
		if !object.is_null() {
			let rtti_lock: Field_t = unsafe { (*((*object).rtti)).rt.lock_word };
			if field_is_inflated_lock(rtti_lock) {
				let mut node = field_alligned_lock_ref(rtti_lock);
				// Get the pointer to node
				let edge: *mut *mut usize = &mut node;
				let edge = edge as *mut usize;
				let node_addr = Address::from_mut_ptr(node);
				if is_mmtk_object(node_addr) {
					let object = node as *mut Object;
					assert!(is_mmtk_object(node_addr));
					
					unsafe {
						assert!(!(*object).rtti.is_null());
						mmtk_scan_lock_words(object, closure);
						if (*object).is_weak_reference() {
								WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
						}

						assert!((*object).size() != 0);
						// Create the work packets here
						closure.visit_edge(Address::from_mut_ptr(edge as *mut usize));
					}
				}
			}

			let object_lock: Field_t = unsafe { (*object).lock_word };
			if field_is_inflated_lock(object_lock) {
				let mut node = field_alligned_lock_ref(object_lock);
				// Get the pointer to node
				let edge: *mut *mut usize = &mut node;
				let edge = edge as *mut usize;
				let node_addr = Address::from_mut_ptr(node);
				if is_mmtk_object(node_addr) {
					let object = node as *mut Object;
					assert!(is_mmtk_object(node_addr));
					
					unsafe {
						assert!(!(*object).rtti.is_null());
						mmtk_scan_lock_words(object, closure);
						if (*object).is_weak_reference() {
								WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
						}

						assert!((*object).size() != 0);
						// Create the work packets here
						closure.visit_edge(Address::from_mut_ptr(edge as *mut usize));
					}
				}
			}
		}
	}
}

impl ObjIterate for Object {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
		let ptr_map: *mut i64 = unsafe { (*(self.rtti)).ref_map_struct };
		let mut i = 0;
		let fields = self.get_fields();
		unsafe {
			while *ptr_map.offset(i) != LAST_FIELD_OFFSET {
				let offset = *ptr_map.offset(i);
				if self.is_referant_of_weak_reference(offset.try_into().unwrap()) {
					i += 1;
					continue
				}
				let edge = fields.offset(offset as isize);
				let node = *edge;
				if is_word_in_heap(node) {
					let node_addr = Address::from_mut_ptr(node);
					if is_mmtk_object(node_addr) {
							let object = node as *mut Object;
							assert!(is_mmtk_object(node_addr));
							assert!(!(*object).rtti.is_null());
							mmtk_scan_lock_words(object, closure);
							if (*object).is_weak_reference() {
									WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
							}

							assert!((*object).size() != 0);
							// Create the work packets here
							closure.visit_edge(Address::from_mut_ptr(edge as *mut usize));
					}
				}
				i += 1;
			}
		}
	}
}

impl ObjIterate for ArrayHeader {
	fn obj_iterate(&self, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
		unsafe {
			if (*(self.rtti)).rt.id == *__object_array_id.lock().unwrap() {
				let length: usize = self.length.try_into().unwrap();
				let fields: *mut *mut word_t = 
					((self as *const _ as usize) + std::mem::size_of::<ArrayHeader>()) as *mut *mut word_t;
				for i in 0..length {
					let edge = fields.offset(i as isize);
					let node = *edge as *mut usize;
					if is_word_in_heap(node) {
						let node_addr = Address::from_mut_ptr(node);
						if is_mmtk_object(node_addr) {
							let object = node as *mut Object;
							assert!(is_mmtk_object(node_addr));
							assert!(!(*object).rtti.is_null());
							mmtk_scan_lock_words(object, closure);
							if (*object).is_weak_reference() {
									WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
							}

							assert!((*object).size() != 0);
							// Create the work packets here
							closure.visit_edge(Address::from_mut_ptr(edge as *mut usize));
						}
					}
				}
			}
		}
	}
}

fn obj_iterate(obj: Obj, closure: &mut impl EdgeVisitor<ScalaNativeEdge>) {
	match obj.is_array() {
		true => {
			println!("Scanning: Array: {:}", obj);
			unsafe { obj.as_array_object().obj_iterate(closure) }
		},
		false => {
			println!("Scanning: Object: {:}", obj);
			obj.obj_iterate(closure)
		},
	}
}

pub fn scan_object(
	_tls: VMWorkerThread,
	object: ObjectReference,
	closure: &mut impl EdgeVisitor<ScalaNativeEdge>,
) {
	unsafe { obj_iterate(mem::transmute(object), closure) }
}
