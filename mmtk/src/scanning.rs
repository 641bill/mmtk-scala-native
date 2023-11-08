use std::collections::HashSet;
use std::ptr::null;
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use crate::EdgesClosure;
use crate::NewBuffer;
use crate::NodesClosure;
use crate::ScalaNative;
use crate::abi::Field_t;
use crate::abi::Obj;
use crate::abi::Object;
use crate::abi::WEAK_REF_FIELD_OFFSET;
#[cfg(feature = "uses_lockword")]
use crate::abi::field_alligned_lock_ref;
#[cfg(feature = "uses_lockword")]
use crate::abi::field_is_inflated_lock;
#[cfg(feature = "object_pinning")]
use crate::api::mmtk_append_pinned_objects;
#[cfg(feature = "object_pinning")]
use crate::api:: mmtk_pin_object;
use crate::api::release_buffer;
use crate::edges::ScalaNativeEdge;
use atomic::Ordering;
use log::debug;
use mmtk::MutatorContext;
use mmtk::memory_manager::is_mmtk_object;
use mmtk::memory_manager::last_heap_address;
use mmtk::memory_manager::starting_heap_address;
use mmtk::util::Address;
use mmtk::util::opaque_pointer::*;
use mmtk::util::ObjectReference;
use mmtk::vm::EdgeVisitor;
use mmtk::vm::RootsWorkFactory;
use mmtk::vm::Scanning;
use mmtk::Mutator;
use mmtk::vm::edge_shape::SimpleEdge;
use crate::UPCALLS;
use lazy_static::lazy_static;

pub struct VMScanning {}

const WORK_PACKET_CAPACITY: usize = 4096;

use std::sync::Mutex;

#[repr(C)]
pub struct ObjectSendPtr(pub *mut Object);
unsafe impl Send for ObjectSendPtr {}

pub struct UsizeSendPtr(*mut *mut usize);
unsafe impl Send for UsizeSendPtr {}

lazy_static! {
    pub static ref WEAK_REF_STACK: Mutex<Vec<ObjectSendPtr>> = Mutex::new(Vec::new());
}

lazy_static! {
    pub static ref ALLOCATION_ALIGNMENT_LAZY: usize = unsafe {
        ((*UPCALLS).get_allocation_alignment)()
    };
    pub static ref ALLOCATION_ALIGNMENT_INVERSE_MASK: usize = 
        !(*ALLOCATION_ALIGNMENT_LAZY - 1);
    static ref __MODULES: Mutex<UsizeSendPtr> = Mutex::new(unsafe {
        UsizeSendPtr(((*UPCALLS).get_modules)())
    });
    static ref __MODULES_SIZE: i32 = unsafe {
        ((*UPCALLS).get_modules_size)()
    };
}

pub static VISITED: AtomicBool = AtomicBool::new(false);
pub static HANDLER_FN: Mutex<Option<fn()>> = Mutex::new(None);

extern "C" fn report_edges_and_renew_buffer<F: RootsWorkFactory<ScalaNativeEdge>>(
    ptr: *mut Address,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let address_buf = unsafe { Vec::<Address>::from_raw_parts(ptr, length, capacity) };
        let simple_edge_buf: Vec<SimpleEdge> = address_buf.iter().map(|&addr| SimpleEdge::from_address(addr)).collect();
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_edge_roots_work(simple_edge_buf);
    }
    let (ptr, _, capacity) = {
        // TODO: Use Vec::into_raw_parts() when the method is available.
        use std::mem::ManuallyDrop;
        let new_vec = Vec::with_capacity(WORK_PACKET_CAPACITY);
        let mut me = ManuallyDrop::new(new_vec);
        (me.as_mut_ptr(), me.len(), me.capacity())
    };
    NewBuffer { ptr, capacity }
}

pub(crate) fn to_edges_closure<F: RootsWorkFactory<ScalaNativeEdge>>(factory: &mut F) -> EdgesClosure {
    EdgesClosure {
        func: report_edges_and_renew_buffer::<F>,
        data: factory as *mut F as *mut libc::c_void,
    }
}

extern "C" fn report_nodes_and_renew_buffer<F: RootsWorkFactory<ScalaNativeEdge>>(
    ptr: *mut *mut Object,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let mut address_buf = unsafe { Vec::<*mut Object>::from_raw_parts(ptr, length, capacity) };
        let address_set: HashSet<_> = address_buf.drain(..).collect();
        address_buf.extend(address_set.into_iter());
        let buf: Vec<ObjectReference> = address_buf.into_iter().map(|addr| ObjectReference::from(unsafe { &*addr })).collect();
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_pinning_roots_work(buf);
    }
    let (ptr, _, capacity) = {
        use std::mem::ManuallyDrop;
        let new_vec = Vec::with_capacity(WORK_PACKET_CAPACITY);
        let mut me = ManuallyDrop::new(new_vec);
        (me.as_mut_ptr(), me.len(), me.capacity())
    };
    NewBuffer { ptr, capacity }
}

pub(crate) fn to_nodes_closure<F: RootsWorkFactory<ScalaNativeEdge>>(factory: &mut F) -> NodesClosure {
    NodesClosure {
        func: report_nodes_and_renew_buffer::<F>,
        data: factory as *mut F as *mut libc::c_void,
    }
}

#[repr(C)]
pub struct RootsClosure {
    buffer: *mut *mut Object,
    cursor: usize,
    capacity: usize,
    nodes_closure: NodesClosure,
}

impl RootsClosure {
    pub fn new(nodes_closure: NodesClosure) -> Self {
        let buf = (nodes_closure.func)(null::<*mut Object>() as *mut *mut Object, 0, 0, nodes_closure.data);
        Self {
            buffer: buf.ptr,
            cursor: 0,
            capacity: buf.capacity,
            nodes_closure,
        }
    }

    pub fn do_work(&mut self, p: *mut Object) {
        unsafe {
            *self.buffer.offset(self.cursor as isize) = p;
        }
        self.cursor += 1;
        if self.cursor >= self.capacity {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if self.cursor > 0 {
            let buf = (self.nodes_closure.func)(self.buffer, self.cursor, self.capacity, self.nodes_closure.data);
            self.buffer = buf.ptr;
            self.capacity = buf.capacity;
            self.cursor = 0;
        }
    }
}

impl Drop for RootsClosure {
    fn drop(&mut self) {
        if self.cursor > 0 {
            self.flush();
        }
        if !self.buffer.is_null() {
            unsafe {
                release_buffer(self.buffer, self.cursor, self.capacity);
            }
        }
    }
}

pub(crate) fn is_word_in_heap(address: *mut usize) -> bool {
    let address_num = address as usize;
    address_num >= starting_heap_address().as_usize() && 
        address_num < last_heap_address().as_usize()
}

pub(crate) fn is_ptr_aligned(address: *mut usize) -> bool {
    let address_num = address as usize;
    let mask = *(ALLOCATION_ALIGNMENT_INVERSE_MASK);
    let aligned = address_num & mask;
    (aligned as *mut usize) == address
}

pub(crate) fn align_ptr(address: *mut usize) -> *mut usize {
    let address_num = address as usize;
    let mask = *(ALLOCATION_ALIGNMENT_INVERSE_MASK);
    let aligned = address_num & mask;
    aligned as *mut usize
}

pub fn mmtk_mark_object(
    object: *mut Object,
    roots_closure: &mut RootsClosure,
) {
    unsafe {
        debug_assert!(!(*object).rtti.is_null());
        mmtk_mark_lock_words(object, roots_closure);
        if (*object).is_weak_reference() {
            WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
        }

        debug_assert!((*object).size() != 0);
        // Create the work packets here
        roots_closure.do_work(object);
    }
}

#[inline]
pub fn mmtk_mark_field(
    field: Field_t,
    roots_closure: &mut RootsClosure,
) {
    if is_word_in_heap(field) {
        let field_addr = Address::from_mut_ptr(field);
        if is_mmtk_object(field_addr) {
            mmtk_mark_object(field as *mut Object, roots_closure);
        }
    }
}

pub fn mmtk_mark_conservative(
    address: *mut usize,
    roots_closure: &mut RootsClosure,
) {
    debug_assert!(is_word_in_heap(address));
    let mask = *(ALLOCATION_ALIGNMENT_INVERSE_MASK);
    let object = ((address as usize) & mask) as *mut usize as *mut Object;
    let object_addr = Address::from_mut_ptr(object);
    if !object.is_null() {
        if is_mmtk_object(object_addr) {
            mmtk_mark_object(object, roots_closure);
        }
    }
}

#[inline]
pub fn mmtk_mark_lock_words(
    object: *mut Object,
    roots_closure: &mut RootsClosure,    
) {
    #[cfg(feature = "uses_lockword")] 
    {
        if !object.is_null() {
            let rtti_lock: Field_t = unsafe { (*((*object).rtti)).rt.lock_word };
            if field_is_inflated_lock(rtti_lock) {
                mmtk_mark_field(field_alligned_lock_ref(rtti_lock), roots_closure);
            }

            let object_lock = unsafe { (*object).lock_word };
            if field_is_inflated_lock(object_lock) {
                mmtk_mark_field(field_alligned_lock_ref(object_lock), roots_closure);
            }
        }
    }
}

pub unsafe fn mmtk_mark_modules(
    roots_closure: &mut RootsClosure,  
) {
    let modules = (*(__MODULES.lock().unwrap())).0;
    let nb_modules = *(__MODULES_SIZE);

    #[cfg(feature = "object_pinning")]
    let mut current_pinned_objects = Vec::new();
    for i in 0..nb_modules {
        let edge = modules.offset(i as isize);
        let node = *edge;
        let object = node as *mut Object;
        #[cfg(feature = "object_pinning")]
        {
            let obj_ref = ObjectReference::from_raw_address(Address::from_mut_ptr(node));
            if memory_manager::pin_object::<ScalaNative>(obj_ref) {
                current_pinned_objects.push(obj_ref);
            }
        }
        mmtk_mark_field(object as Field_t, roots_closure)
    }
    #[cfg(feature = "object_pinning")]
    crate::binding().pinned_objects.lock().unwrap().append(&mut current_pinned_objects);
}

pub unsafe fn mmtk_mark_range(
    from: *mut *mut usize,
    to: *mut *mut usize,
    roots_closure: &mut RootsClosure,
) {
    debug_assert!(!from.is_null());
    debug_assert!(!to.is_null());
    
    #[cfg(feature = "object_pinning")]
    let mut current_pinned_objects = Vec::new();
    
    let mut current = from;
    while current <= to {
        let addr = *current;
        if is_word_in_heap(addr) && is_ptr_aligned(addr) {
            #[cfg(feature = "object_pinning")]
            {
                let obj_ref = ObjectReference::from_raw_address(Address::from_mut_ptr(addr));
                if memory_manager::pin_object::<ScalaNative>(obj_ref) {
                    current_pinned_objects.push(obj_ref);
                }
            } 
            mmtk_mark_conservative(addr, roots_closure);
        }
        current = current.offset(1);
    }
    
    #[cfg(feature = "object_pinning")]
    crate::binding().pinned_objects.lock().unwrap().append(&mut current_pinned_objects);
}

pub unsafe fn mmtk_mark_program_stack(tls: VMMutatorThread, roots_closure: &mut RootsClosure) {
    let stack_range = ((*UPCALLS).get_stack_range)(tls);
    let regs_range = ((*UPCALLS).get_regs_range)(tls);
    mmtk_mark_range(stack_range.stack_top, 
        stack_range.stack_bottom, roots_closure);
    mmtk_mark_range(regs_range.regs, 
        regs_range.regs.add(regs_range.regs_size), roots_closure);
}

fn scan_roots_in_all_mutator_threads<F: RootsWorkFactory<ScalaNativeEdge>>(_tls: VMWorkerThread, _factory: &mut F) {
    unsafe {
        let nodes_closure = to_nodes_closure(_factory);
        let mut roots_closure = RootsClosure::new(nodes_closure);
        let mut head = ((*UPCALLS).get_mutator_threads)();
        while !head.is_null() {
            let node = &*head;
            let thread = node.value;
            let tls = VMMutatorThread(VMThread(OpaquePointer::from_address(Address::from_mut_ptr(thread))));
            mmtk_mark_program_stack(tls, &mut roots_closure);
            head = node.next;
        }
    }
}

fn weak_ref_stack_is_empty() -> bool {
    let weak_refs = WEAK_REF_STACK.lock().unwrap();
    weak_refs.is_empty()
}

fn weak_ref_stack_pop() -> Option<ObjectSendPtr> {
    let mut weak_refs = WEAK_REF_STACK.lock().unwrap();
    weak_refs.pop()
}

pub fn mmtk_weak_ref_stack_nullify(closure: &mut impl mmtk::vm::ObjectTracer) {
    VISITED.store(false, Ordering::SeqCst);
    while !weak_ref_stack_is_empty() {
        let weak_ref = weak_ref_stack_pop().unwrap();
        let object = weak_ref.0;
        let field_offset = *WEAK_REF_FIELD_OFFSET;
        if is_word_in_heap(object as *mut usize) {
            let object_ref = ObjectReference::from_raw_address(Address::from_mut_ptr(object));
            if !object_ref.is_reachable() {
                let traced = closure.trace_object(object_ref);
                let traced_obj = Obj::from(traced);
		        let fields = traced_obj.get_fields();
				let edge = unsafe { fields.offset(field_offset as isize) };
                let edge_addr = Address::from_mut_ptr(edge);
                let null_ptr: *mut usize = null_mut();
                unsafe { edge_addr.store(null_ptr) };
                VISITED.store(true, Ordering::SeqCst);
            }
        }
    }
}

pub fn mmtk_weak_ref_stack_call_handlers() {
    let mut handler_fn = HANDLER_FN.lock().unwrap();
    if let Some(handler_fn) = handler_fn.as_mut() {
        handler_fn();
    }
}

impl Scanning<ScalaNative> for VMScanning {
    fn scan_roots_in_mutator_thread(
        _tls: VMWorkerThread,
        _mutator: &'static mut Mutator<ScalaNative>,
        mut _factory: impl RootsWorkFactory<ScalaNativeEdge>,
    ) {
        let tls: VMMutatorThread = _mutator.get_tls();
        // println!("scan_roots_in_mutator_thread, tls: {:?}", tls);
        unsafe {
            let nodes_closure = to_nodes_closure(&mut _factory);
            let mut roots_closure = RootsClosure::new(nodes_closure);
            
            mmtk_mark_program_stack(tls, &mut roots_closure);
        }
    }

    fn scan_vm_specific_roots(_tls: VMWorkerThread, mut _factory: impl RootsWorkFactory<ScalaNativeEdge>) {
        unsafe {
            // scan_roots_in_all_mutator_threads(_tls, &mut _factory);
            let nodes_closure = to_nodes_closure(&mut _factory);
            let mut roots_closure = RootsClosure::new(nodes_closure);
            mmtk_mark_modules(&mut roots_closure);
        }
    }

    fn support_edge_enqueuing(_tls: VMWorkerThread, _object: ObjectReference) -> bool {
        false // Due to the scanning of lock words
    }

    fn scan_object<EV: EdgeVisitor<ScalaNativeEdge>>(
        _tls: VMWorkerThread,
        _object: ObjectReference,
        _edge_visitor: &mut EV,
    ) {
        crate::object_scanning::scan_object(_tls, _object, _edge_visitor);
    }

    fn scan_object_and_trace_edges<OT: mmtk::vm::ObjectTracer>(
            _tls: VMWorkerThread,
            _object: ObjectReference,
            _object_tracer: &mut OT,
    ) {
        crate::object_scanning::scan_object_and_trace_edges(_tls, _object, _object_tracer);
    }

    fn notify_initial_thread_scan_complete(_partial_scan: bool, _tls: VMWorkerThread) {
        // do nothing
    }

    fn supports_return_barrier() -> bool {
        unimplemented!()
    }

    fn prepare_for_roots_re_scanning() {
        unsafe {
            ((*UPCALLS).prepare_for_roots_re_scanning)();
        }
    }

    fn process_weak_refs(
            _worker: &mut mmtk::scheduler::GCWorker<ScalaNative>,
            _tracer_context: impl mmtk::vm::ObjectTracerContext<ScalaNative>,
        ) -> bool {
        #[cfg(feature = "object_pinning")]  
        crate::binding().unpin_pinned_objects();
        debug!("process_weak_refs");
        _tracer_context.with_tracer(_worker, |object_tracer| {
            mmtk_weak_ref_stack_nullify(object_tracer);
        });
        mmtk_weak_ref_stack_call_handlers();
        debug!("process_weak_refs done");
        false
    }

    fn forward_weak_refs(
            _worker: &mut mmtk::scheduler::GCWorker<ScalaNative>,
            _tracer_context: impl mmtk::vm::ObjectTracerContext<ScalaNative>,
    ) {
        panic!("We can't use MarkCompact in Scala Native.");
    }
}
