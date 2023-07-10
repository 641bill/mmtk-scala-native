use std::mem;

use libc::size_t;
use mmtk::util::{ObjectReference, Address, VMWorkerThread};
use crate::{UPCALLS, ScalaNative};
use mmtk::scheduler::{GCController, GCWorker};
use crate::collection::{GC_THREAD_KIND_CONTROLLER, GC_THREAD_KIND_WORKER};

#[cfg(feature = "scalanative_multithreading_enabled")]
pub const monitor_inflation_mark_mask: word_t = 1;

#[cfg(feature = "scalanative_multithreading_enabled")]
pub const monitor_object_mask: word_t = !monitor_inflation_mark_mask;

pub type word_t = usize;

#[repr(C)]
pub struct Runtime {
	pub cls: *mut word_t,
	#[cfg(feature = "uses_lockword")]
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
	#[cfg(feature = "uses_lockword")]
	pub lock_word: *mut word_t,
	pub fields: [Field_t; 0],
}

#[repr(C)]
pub struct CharArray {
	pub header: ArrayHeader,
	pub value: [i16; 0],
}

#[repr(C)]
pub struct StringObject {
	pub rtti: *mut Rtti,
	#[cfg(feature = "uses_lockword")]
	pub lock_word: *mut word_t,
	pub value: *mut CharArray,
	pub offset: i32,
	pub count: i32,
	pub cached_hash_code: i32,
}

#[repr(C)]
pub struct ArrayHeader {
	pub rtti: *mut Rtti,
	#[cfg(feature = "uses_lockword")]
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
    let base_size = mem::size_of::<*mut Rtti>() as usize;
		#[cfg(feature = "uses_lockword")]
    let base_size = base_size + mem::size_of::<*mut word_t>() as usize;
		// println!("Object address: {}, with base size: {:x}", Address::from_ref(self), base_size);
    Address::from_ref(self) + base_size
	}

	pub fn num_fields(&self) -> usize {
		let fields_size = (unsafe { &*self.rtti }).size as usize - mem::size_of::<*mut Rtti>();
		#[cfg(feature = "uses_lockword")]
		let fields_size = fields_size - mem::size_of::<*mut word_t>();
		// println!("Object address: {}, with size: {:x}, and fields_size: {:x}", Address::from_ref(self), (unsafe { &*self.rtti }).size, fields_size);
		fields_size / mem::size_of::<Field_t>()
	}

}

impl ArrayHeader {
	pub fn get_element_address(&self, index: i32) -> Address {
		let base_size = mem::size_of::<ArrayHeader>() as usize;
		// println!("Array address: {}, with base size: {:x}", Address::from_ref(self), base_size);
		Address::from_ref(self) + base_size + (index as usize) * (self.stride as usize)
	}
}

#[cfg(feature = "uses_lockword")]
pub fn field_is_inflated_lock(field: Field_t) -> bool {
	(field as word_t & monitor_inflation_mark_mask) != 0
}

#[cfg(feature = "uses_lockword")]
pub fn field_alligned_lock_ref(field: Field_t) -> Field_t {
	((field as word_t & monitor_inflation_mark_mask) as word_t) as Field_t
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

#[repr(C)]
pub struct GCThreadTLS {
    pub kind: libc::c_int,
    pub gc_context: *mut libc::c_void,
}

impl GCThreadTLS {
	fn new(kind: libc::c_int, gc_context: *mut libc::c_void) -> Self {
			Self {
					kind,
					gc_context,
			}
	}

	pub fn for_controller(gc_context: *mut GCController<ScalaNative>) -> Self {
			Self::new(GC_THREAD_KIND_CONTROLLER, gc_context as *mut libc::c_void)
	}

	pub fn for_worker(gc_context: *mut GCWorker<ScalaNative>) -> Self {
			Self::new(GC_THREAD_KIND_WORKER, gc_context as *mut libc::c_void)
	}

	pub fn from_vwt(vwt: VMWorkerThread) -> *mut GCThreadTLS {
			unsafe { std::mem::transmute(vwt) }
	}

	/// Cast a pointer to `GCThreadTLS` to a ref, with assertion for null pointer.
	///
	/// # Safety
	///
	/// Has undefined behavior if `ptr` is invalid.
	pub unsafe fn check_cast(ptr: *mut GCThreadTLS) -> &'static mut GCThreadTLS {
			assert!(!ptr.is_null());
			let result = &mut *ptr;
			debug_assert!({
					let kind = result.kind;
					kind == GC_THREAD_KIND_CONTROLLER || kind == GC_THREAD_KIND_WORKER
			});
			result
	}

	/// Cast a pointer to `VMWorkerThread` to a ref, with assertion for null pointer.
	///
	/// # Safety
	///
	/// Has undefined behavior if `ptr` is invalid.
	pub unsafe fn from_vwt_check(vwt: VMWorkerThread) -> &'static mut GCThreadTLS {
			let ptr = Self::from_vwt(vwt);
			Self::check_cast(ptr)
	}

	#[allow(clippy::not_unsafe_ptr_arg_deref)] // `transmute` does not dereference pointer
	pub fn to_vwt(ptr: *mut Self) -> VMWorkerThread {
			unsafe { std::mem::transmute(ptr) }
	}

	/// Get a ref to `GCThreadTLS` from C-level thread-local storage, with assertion for null
	/// pointer.
	///
	/// # Safety
	///
	/// Has undefined behavior if the pointer held in C-level TLS is invalid.
	pub unsafe fn from_upcall_check() -> &'static mut GCThreadTLS {
			let ptr = ((*UPCALLS).get_gc_thread_tls)();
			Self::check_cast(ptr)
	}

	pub fn worker<'w>(&mut self) -> &'w mut GCWorker<ScalaNative> {
			// NOTE: The returned ref points to the worker which does not have the same lifetime as self.
			assert!(self.kind == GC_THREAD_KIND_WORKER);
			unsafe { &mut *(self.gc_context as *mut GCWorker<ScalaNative>) }
	}
}