use std::mem;

use libc::size_t;
use mmtk::util::{ObjectReference, Address};
use crate::UPCALLS;

#[cfg(scalanative_multithreading_enabled)]
pub const uses_lockword: bool = true;

#[cfg(scalanative_multithreading_enabled)]
pub const monitor_inflation_mark_mask: word_t = 1;

#[cfg(scalanative_multithreading_enabled)]
pub const monitor_object_mask: word_t = !monitor_inflation_mark_mask;

pub type word_t = usize;

#[repr(C)]
pub struct Runtime {
	pub cls: *mut word_t,
	#[cfg(uses_lockword)]
	pub lock_word: *mut word_t,
	pub id: i32,
	pub tid: i32,
	pub name: *mut word_t,
}

#[repr(C)]
pub struct Rtti {
	pub rt: Runtime,
	pub size: i32,
	pub id_range_until: i32,
	pub ref_map_struct: *mut i64,
}

pub type Field_t = *mut word_t;

#[repr(C)]
pub struct Object {
	pub rtti: *mut Rtti,
	#[cfg(uses_lockword)]
	pub lock_word: *mut word_t,
	pub fields: [Field_t; 0],
}

#[repr(C)]
pub struct ArrayHeader {
	pub rtti: *mut Rtti,
	#[cfg(uses_lockword)]
	pub lock_word: *mut word_t,
	pub length: i32,
	pub stride: i32,
}

#[repr(C)]
pub struct Chunk {
	pub nothing: *mut libc::c_void,
	pub size: size_t,
	pub next: *mut Chunk,
}

pub fn round_to_next_multiple(value: size_t, multiple: size_t) -> size_t {
	(value + multiple - 1) / multiple * multiple
}

impl Object {
	pub fn is_array(&self) -> bool {
		let id = unsafe { (*self.rtti).rt.id };
		unsafe { ((*UPCALLS).get_array_ids_min)() <= id && id <= ((*UPCALLS).get_array_ids_max)() }
	}

	pub fn size(&self) -> size_t {
			if self.is_array() {
				let array_header = unsafe {&*(self as *const _ as *const ArrayHeader) };
				round_to_next_multiple(
					mem::size_of::<ArrayHeader>() + (array_header.length as size_t) * (array_header.stride as size_t),
					unsafe { ((*UPCALLS).get_allocation_alignment)() },
			)
			} else {
					round_to_next_multiple((unsafe { &*self.rtti }).size as size_t, unsafe { ((*UPCALLS).get_allocation_alignment)() })
			}
	}

	pub fn is_weak_reference(&self) -> bool {
		unsafe { &*self.rtti }.rt.id == unsafe { ((*UPCALLS).get_weak_ref_id)() } 
	}

	pub fn is_referant_of_weak_reference(&self, field_offset: i32) -> bool {
		self.is_weak_reference() && field_offset == unsafe { ((*UPCALLS).get_weak_ref_field_offset)() }
	}

	pub unsafe fn as_array_object(&self) -> &ArrayHeader {
		&*(self as *const _ as *const ArrayHeader)
	}

	pub fn get_field_address(&self) -> Address {
    let base_size = mem::size_of::<Rtti>() as isize;
    #[cfg(uses_lockword)]
    let base_size = base_size + mem::size_of::<word_t>() as isize;
    Address::from_ref(self) + base_size
	}
}

impl ArrayHeader {
	pub fn get_element_address(&self, index: i32) -> Address {
		let base_size = mem::size_of::<ArrayHeader>() as isize;
		#[cfg(uses_lockword)]
		let base_size = base_size + mem::size_of::<word_t>() as isize;
		Address::from_ref(self) + base_size + (index as isize) * (self.stride as isize)
	}
}

#[cfg(uses_lockword)]
	pub fn field_is_inflated_lock(&self) -> bool {
		unsafe { *self & monitor_inflation_mark_mask != 0 }
}

#[cfg(uses_lockword)]
	pub fn field_alligned_lock_ref(&self) -> *mut field_t {
		unsafe { self & monitor_object_mask }
}

pub type Obj = &'static Object;

/// Convert ObjectReference to Obj
impl From<ObjectReference> for &Object {
	fn from(o: ObjectReference) -> Self {
		unsafe { mem::transmute(o) }
	}
}

/// Convert Obj to ObjectReference
impl From<&Object> for ObjectReference {
	fn from(o: &Object) -> Self {
		unsafe { mem::transmute(o) }
	}
}
