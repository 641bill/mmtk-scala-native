use std::mem;

use libc::size_t;
use mmtk::util::{ObjectReference, Address, VMWorkerThread};
use crate::{UPCALLS, ScalaNative, object_scanning::LAST_FIELD_OFFSET};
use mmtk::scheduler::{GCController, GCWorker};
use crate::collection::{GC_THREAD_KIND_CONTROLLER, GC_THREAD_KIND_WORKER};
use crate::scanning::{ALLOCATION_ALIGNMENT_LAZY, is_ptr_aligned, align_ptr};

#[cfg(feature = "scalanative_multithreading_enabled")]
pub const MONITOR_INFLATION_MARK_MASK: word_t = 1;

#[cfg(feature = "scalanative_multithreading_enabled")]
pub const MONITOR_OBJECT_MASK: word_t = !MONITOR_INFLATION_MARK_MASK;

pub type word_t = usize;

lazy_static! {
	static ref ARRAY_IDS_MIN: i32 = unsafe {
		((*UPCALLS).get_array_ids_min)()
	};
	static ref ARRAY_IDS_MAX: i32 = unsafe {
		((*UPCALLS).get_array_ids_max)()
	};
	static ref WEAK_REF_IDS_MIN: i32 = unsafe {
		((*UPCALLS).get_weak_ref_ids_min)()
	};
	static ref WEAK_REF_IDS_MAX: i32 = unsafe {
		((*UPCALLS).get_weak_ref_ids_max)()
	};
	pub static ref WEAK_REF_FIELD_OFFSET: i32 = unsafe {
		((*UPCALLS).get_weak_ref_field_offset)()
	};
}

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
		debug_assert!(is_ptr_aligned(self.rtti as *mut usize), "rtti: {:b} not aligned", self.rtti as usize);
		
		let id = unsafe { (*self.rtti).rt.id };
		*ARRAY_IDS_MIN <= id && id <= *ARRAY_IDS_MAX
	}

	pub fn is_array_for_copy(&self) -> bool {
		let rtti = align_ptr(self.rtti as *mut usize) as *mut Rtti;
		
		let id = unsafe { (*rtti).rt.id };
		*ARRAY_IDS_MIN <= id && id <= *ARRAY_IDS_MAX
	}

	pub fn size(&self) -> size_t {
		if self.is_array() {
			unsafe { self.as_array_object().size() }
		} else {
				round_to_next_multiple((unsafe { &*self.rtti }).size as size_t, *ALLOCATION_ALIGNMENT_LAZY)
		}
	}

	pub fn size_for_copy(&self) -> size_t {
		let rtti = align_ptr(self.rtti as *mut usize) as *mut Rtti;
		if self.is_array_for_copy() {
			unsafe { self.as_array_object().size() }
		} else {
			round_to_next_multiple((unsafe { &*rtti }).size as size_t, *ALLOCATION_ALIGNMENT_LAZY)
		}
	}

	pub fn is_weak_reference(&self) -> bool {
		unsafe {
			*WEAK_REF_IDS_MIN <= (&*self.rtti).rt.id &&
			(&*self.rtti).rt.id <= *WEAK_REF_IDS_MAX
		}
	}

	pub fn is_referant_of_weak_reference(&self, field_offset: i32) -> bool {
		self.is_weak_reference() && field_offset == *WEAK_REF_FIELD_OFFSET
	}

	pub unsafe fn as_array_object(&self) -> &ArrayHeader {
		&*(self as *const _ as *const ArrayHeader)
	}

	pub fn get_fields(&self) -> *mut Field_t {
		let fields = self as *const _ as usize + mem::size_of_val(&self.rtti);
		#[cfg(feature = "uses_lockword")]
		let fields = fields + mem::size_of_val(&self.lock_word);
		fields as *mut Field_t
	}

	pub fn get_field_address(&self) -> Address {
    let base_size = mem::size_of::<*mut Rtti>() as usize;
		#[cfg(feature = "uses_lockword")]
    let base_size = base_size + mem::size_of::<*mut word_t>() as usize;
		// println!("Object address: {}, with base size: {:x}", Address::from_ref(self), base_size);
    Address::from_ref(self) + base_size
	}

	pub fn num_fields(&self) -> usize {
		let ptr_map: *mut i64 = unsafe { (*(self.rtti)).ref_map_struct };
		let mut i = 0;
		unsafe { 
			while *ptr_map.offset(i)  != LAST_FIELD_OFFSET {
				i += 1;
			}
		}
		i as usize
	}
}

impl ArrayHeader {
	pub fn size(&self) -> size_t {
		round_to_next_multiple(
			mem::size_of::<ArrayHeader>() + 
				self.length as size_t * self.stride as size_t,
				*ALLOCATION_ALIGNMENT_LAZY,
		)
	}

	pub fn get_element_address(&self, index: i32) -> Address {
		let base_size = mem::size_of::<ArrayHeader>() as usize;
		// println!("Array address: {}, with base size: {:x}", Address::from_ref(self), base_size);
		Address::from_ref(self) + base_size + (index as usize) * (self.stride as usize)
	}
}

// If the lowest bit is 1, the lock is inflated
#[cfg(feature = "uses_lockword")]
pub fn field_is_inflated_lock(field: Field_t) -> bool {
	(field as word_t & MONITOR_INFLATION_MARK_MASK) != 0
}

// Set the lowest bit to 0
#[cfg(feature = "uses_lockword")]
pub fn field_alligned_lock_ref(field: Field_t) -> Field_t {
	(field as word_t & MONITOR_OBJECT_MASK) as Field_t
}

// Set the lowest bit to 1
#[cfg(feature = "uses_lockword")]
pub fn field_inflate_lock_ref(field: Field_t) -> Field_t {
	(field as word_t | MONITOR_INFLATION_MARK_MASK) as Field_t
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
		debug_assert!(!ptr.is_null());
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
			debug_assert!(self.kind == GC_THREAD_KIND_WORKER);
			unsafe { &mut *(self.gc_context as *mut GCWorker<ScalaNative>) }
	}
}

#[repr(C)]
pub struct MutatorThreadNode {
    pub value: *mut libc::c_void,
    pub next: *mut MutatorThreadNode,
}
