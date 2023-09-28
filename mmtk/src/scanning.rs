use std::panic;
use std::ptr::null;

use crate::EdgesClosure;
use crate::NewBuffer;
use crate::NodesClosure;
use crate::ScalaNative;
use crate::abi::Field_t;
use crate::abi::Obj;
use crate::abi::Object;
use crate::abi::field_alligned_lock_ref;
use crate::abi::field_is_inflated_lock;
use crate::abi::word_t;
use crate::edges::ScalaNativeEdge;
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
use crate::UPCALLS;
use lazy_static::lazy_static;

pub struct VMScanning {}

const WORK_PACKET_CAPACITY: usize = 4096;

use std::sync::Mutex;

pub struct ObjectSendPtr(*mut Object);
unsafe impl Send for ObjectSendPtr {}

pub struct UsizeSendPtr(*mut usize);
unsafe impl Send for UsizeSendPtr {}

lazy_static! {
    pub static ref STACK: Mutex<Vec<ObjectSendPtr>> = Mutex::new(Vec::new());
    pub static ref WEAK_REF_STACK: Mutex<Vec<ObjectSendPtr>> = Mutex::new(Vec::new());
}

lazy_static! {
    static ref ALLOCATION_ALIGNMENT_LAZY: Mutex<usize> = Mutex::new(unsafe {
        ((*UPCALLS).get_allocation_alignment)()
    });
    static ref ALLOCATION_ALIGNMENT_INVERSE_MASK: Mutex<usize> = Mutex::new(
        !(*ALLOCATION_ALIGNMENT_LAZY.lock().unwrap() - 1)
    );
    static ref __modules: Mutex<UsizeSendPtr> = Mutex::new(unsafe {
        UsizeSendPtr(((*UPCALLS).get_modules)())
    });
    static ref __modules_size: Mutex<i32> = Mutex::new(unsafe {
        ((*UPCALLS).get_modules_size)()
    });
}

extern "C" fn report_edges_and_renew_buffer<F: RootsWorkFactory<ScalaNativeEdge>>(
    ptr: *mut Address,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let buf = unsafe { Vec::<Address>::from_raw_parts(ptr, length, capacity) };
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_edge_roots_work(buf);
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
    ptr: *mut Address,
    length: usize,
    capacity: usize,
    factory_ptr: *mut libc::c_void,
) -> NewBuffer {
    if !ptr.is_null() {
        let address_buf = unsafe { Vec::<Address>::from_raw_parts(ptr, length, capacity) };
        let buf: Vec<ObjectReference> = address_buf.into_iter().map(|addr| ObjectReference::from_raw_address(addr)).collect();
        let factory: &mut F = unsafe { &mut *(factory_ptr as *mut F) };
        factory.create_process_pinning_roots_work(buf);
    }
    let (ptr, _, capacity) = {
        use std::mem::ManuallyDrop;
        let new_vec = Vec::with_capacity(1);
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
    buffer: Vec<*mut Object>,
    capacity: usize,
    nodes_closure: NodesClosure,
}

impl RootsClosure {
    pub fn new(nodes_closure: NodesClosure, capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            nodes_closure,
        }
    }

    pub fn do_work(&mut self, p: *mut Object) {
        self.buffer.push(p);
        if self.buffer.len() >= self.capacity {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if !self.buffer.is_empty() {
            let buf = std::mem::replace(&mut self.buffer, Vec::with_capacity(self.capacity));
            let new_buffer = (self.nodes_closure.func)(
                buf.as_ptr() as *mut Address,
                buf.len(),
                self.capacity,
                self.nodes_closure.data,
            );
            self.buffer = unsafe { Vec::from_raw_parts(new_buffer.ptr as *mut *mut Object, 0, new_buffer.capacity) };
        }
    }
}

impl Drop for RootsClosure {
    fn drop(&mut self) {
        self.flush(); // Ensure flush is called when the object is dropped
    }
}

pub(crate) fn is_word_in_heap(address: *mut usize) -> bool {
    let address_num = address as usize;
    address_num >= starting_heap_address().as_usize() && 
        address_num <= last_heap_address().as_usize()
}

pub(crate) fn is_ptr_aligned(address: *mut usize) -> bool {
    let address_num = address as usize;
    let aligned = address_num & *ALLOCATION_ALIGNMENT_INVERSE_MASK.lock().unwrap();
    aligned == address_num
}

pub fn mmtk_mark_object(
    object: *mut Object,
    roots_closure: &mut RootsClosure,
) {
    unsafe {
        assert!(!(*object).rtti.is_null());
        mmtk_mark_lock_words(object, roots_closure);
        if (*object).is_weak_reference() {
            WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
        }

        assert!((*object).size() != 0);
        // Create the work packets here
        roots_closure.do_work(object);
        // STACK.lock().unwrap().push(ObjectSendPtr(object));
    }
}

pub fn mmtk_scan_object(
    object: *mut Object,
    edge_visitor: &mut impl EdgeVisitor<ScalaNativeEdge>,
) {
    unsafe {
        assert!(!(*object).rtti.is_null());
        mmtk_scan_lock_words(object, edge_visitor);
        if (*object).is_weak_reference() {
            WEAK_REF_STACK.lock().unwrap().push(ObjectSendPtr(object));
        }

        assert!((*object).size() != 0);
        // Create the work packets here
        edge_visitor.visit_edge(Address::from_mut_ptr(object));
        // STACK.lock().unwrap().push(ObjectSendPtr(object));
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

#[inline]
pub fn mmtk_scan_field(
    field: Field_t,
    edge_visitor: &mut impl EdgeVisitor<ScalaNativeEdge>,
) {
    if is_word_in_heap(field) {
        let field_addr = Address::from_mut_ptr(field);
        if is_mmtk_object(field_addr) {
            mmtk_scan_object(field as *mut Object, edge_visitor);
        }
    }
}

pub fn mmtk_mark_conservative(
    address: *mut usize,
    roots_closure: &mut RootsClosure,
) {
    assert!(is_word_in_heap(address));
    let object: *mut Object = address as *mut Object;
    let object_addr = Address::from_mut_ptr(((address as usize) & *ALLOCATION_ALIGNMENT_INVERSE_MASK.lock().unwrap()) as *mut usize);
    if is_mmtk_object(object_addr) {
        mmtk_mark_object(object, roots_closure);
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

#[inline]
pub fn mmtk_scan_lock_words(
    object: *mut Object,
    edge_visitor: &mut impl EdgeVisitor<ScalaNativeEdge>,   
) {
    #[cfg(feature = "uses_lockword")] 
    {
        if !object.is_null() {
            let rtti_lock: Field_t = unsafe { (*((*object).rtti)).rt.lock_word };
            if field_is_inflated_lock(rtti_lock) {
                mmtk_scan_field(field_alligned_lock_ref(rtti_lock), edge_visitor);
            }

            let object_lock = unsafe { (*object).lock_word };
            if field_is_inflated_lock(object_lock) {
                mmtk_scan_field(field_alligned_lock_ref(object_lock), edge_visitor);
            }
        }
    }
}

pub unsafe fn mmtk_mark_modules(
    roots_closure: &mut RootsClosure,  
) {
    let modules: *mut *mut word_t = &mut (__modules.lock().unwrap().0);
    let nb_modules = *__modules_size.lock().unwrap();

    #[cfg(feature = "object_pinning")]
    let mut current_pinned_objects = Vec::new();
    for i in 0..nb_modules {
        let object = *modules.offset(i as isize);
        #[cfg(feature = "object_pinning")]
        {
            if mmtk_pin_object(addr) {
                current_pinned_objects.push(addr);
            }
        }
        mmtk_mark_field(object, roots_closure)
    }
    #[cfg(feature = "object_pinning")]
    mmtk_append_pinned_objects(current_pinned_objects.as_ptr(), current_pinned_objects.len());
}

pub unsafe fn mmtk_mark_range(
    from: *mut *mut usize,
    to: *mut *mut usize,
    roots_closure: &mut RootsClosure,
) {
    assert!(!from.is_null());
    assert!(!to.is_null());
    
    #[cfg(feature = "object_pinning")]
    let mut current_pinned_objects = Vec::new();
    
    let mut current = from;
    while current <= to {
        let addr = *current;
        if is_word_in_heap(addr) && is_ptr_aligned(addr) {
            #[cfg(feature = "object_pinning")]
            {
                if mmtk_pin_object(addr) {
                    current_pinned_objects.push(addr);
                }
            }
            
            mmtk_mark_conservative(addr, roots_closure);
        }
        current = current.offset(1);
    }
    
    #[cfg(feature = "object_pinning")]
    mmtk_append_pinned_objects(current_pinned_objects.as_ptr(), current_pinned_objects.len());
}

impl Scanning<ScalaNative> for VMScanning {
//     const SCAN_MUTATORS_IN_SAFEPOINT: bool = true;
//     const SINGLE_THREAD_MUTATOR_SCANNING: bool = true;

    // fn scan_roots_in_all_mutator_threads(_tls: VMWorkerThread, mut _factory: impl RootsWorkFactory<ScalaNativeEdge>) {
    //     unsafe {
    //         ((*UPCALLS).scan_roots_in_all_mutator_threads)(to_nodes_closure(&mut _factory));
    //     }
    // }

    fn scan_roots_in_mutator_thread(
        _tls: VMWorkerThread,
        _mutator: &'static mut Mutator<ScalaNative>,
        mut _factory: impl RootsWorkFactory<ScalaNativeEdge>,
    ) {
        let tls = _mutator.get_tls();
        unsafe {
            let stack_range = ((*UPCALLS).get_stack_range)(tls);
            let regs_range = ((*UPCALLS).get_regs_range)(tls);
        
            let nodes_closure = to_nodes_closure(&mut _factory);
            let mut roots_closure = RootsClosure::new(nodes_closure, 0);
            
            mmtk_mark_range(stack_range.stack_top, 
                stack_range.stack_bottom, &mut roots_closure);
            mmtk_mark_range(regs_range.regs, 
                regs_range.regs.add(regs_range.regs_size), &mut roots_closure);
        }
    }

    fn scan_vm_specific_roots(_tls: VMWorkerThread, mut _factory: impl RootsWorkFactory<ScalaNativeEdge>) {
        unsafe {
            let nodes_closure = to_nodes_closure(&mut _factory);
            let mut roots_closure = RootsClosure::new(nodes_closure, 0);
            mmtk_mark_modules(&mut roots_closure);
        }
    }

    fn scan_object<EV: EdgeVisitor<ScalaNativeEdge>>(
        _tls: VMWorkerThread,
        _object: ObjectReference,
        _edge_visitor: &mut EV,
    ) {
        crate::object_scanning::scan_object(_tls, _object, _edge_visitor);
    }

    // fn scan_object_with_klass(
    //         _tls: VMWorkerThread,
    //         _object: ObjectReference,
    //         _edge_visitor: &mut impl EdgeVisitor<<ScalaNative as mmtk::vm::VMBinding>::VMEdge>,
    //         _klass: mmtk::util::Address,
    //     ) {
    //     unimplemented!()
    // }
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
        crate::binding().unpin_pinned_objects();
        unsafe {
            ((*UPCALLS).weak_ref_stack_nullify)();
            ((*UPCALLS).weak_ref_stack_call_handlers)();
        }
        false
    }
}
