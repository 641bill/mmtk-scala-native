use std::{sync::Mutex, ptr::null_mut};

use mmtk::{MMTK, util::ObjectReference, memory_manager};

use crate::{ScalaNative, ScalaNative_Upcalls};

pub struct ScalaNativeBinding {
	pub mmtk: &'static MMTK<ScalaNative>,
	pub upcalls: *const ScalaNative_Upcalls,
	pub pinned_objects: Mutex<Vec<ObjectReference>>,
}

unsafe impl Sync for ScalaNativeBinding {}
unsafe impl Send for ScalaNativeBinding {}

impl ScalaNativeBinding {
	pub fn new(mmtk: &'static MMTK<ScalaNative>, upcalls: *const ScalaNative_Upcalls) -> Self {
		Self {
			mmtk,
			upcalls,
			pinned_objects: Mutex::new(Vec::new()),
		}
	}

	pub fn unpin_pinned_objects(&self) {
		let mut pinned_objects = self
				.pinned_objects
				.try_lock()
				.expect("It is accessed during weak ref processing. Should have no race.");

		for object in pinned_objects.drain(..) {
				let result = memory_manager::unpin_object::<ScalaNative>(object);
				debug_assert!(result);
		}
}
}
