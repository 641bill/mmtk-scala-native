use std::sync::Mutex;

use mmtk::{MMTK, util::ObjectReference};

use crate::{ScalaNative, ScalaNativeUpcalls};

pub struct ScalaNativeBinding {
	pub mmtk: &'static MMTK<ScalaNative>,
	pub upcalls: *const ScalaNativeUpcalls,
	pub pinned_objects: Mutex<Vec<ObjectReference>>,
}

unsafe impl Sync for ScalaNativeBinding {}
unsafe impl Send for ScalaNativeBinding {}

impl ScalaNativeBinding {
	pub fn new(mmtk: &'static MMTK<ScalaNative>, upcalls: *const ScalaNativeUpcalls) -> Self {
		Self {
			mmtk,
			upcalls,
			pinned_objects: Mutex::new(Vec::new()),
		}
	}

	#[cfg(feature = "object_pinning")]
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
