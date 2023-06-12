use std::collections::VecDeque;
use std::marker::PhantomData;

use mmtk::Plan;
use mmtk::vm::ActivePlan;
use mmtk::util::opaque_pointer::*;
use mmtk::Mutator;
use crate::MutatorClosure;
use crate::ScalaNative;
use crate::SINGLETON;
use crate::UPCALLS;

struct ScalaNativeMutatorIterator<'a> {
    mutators: VecDeque<&'a mut Mutator<ScalaNative>>,
    phantom_data: PhantomData<&'a ()>,
}

impl<'a> ScalaNativeMutatorIterator<'a> {
    fn new() -> Self {
        let mut mutators = VecDeque::new();
        unsafe {
            ((*UPCALLS).get_mutators)(MutatorClosure::from_rust_closure(&mut |mutator| {
                mutators.push_back(mutator);
            }));
        }
        Self {
            mutators,
            phantom_data: PhantomData,
        }
    }
}

impl<'a> Iterator for ScalaNativeMutatorIterator<'a> {
    type Item = &'a mut Mutator<ScalaNative>;

    fn next(&mut self) -> Option<Self::Item> {
        self.mutators.pop_front()
    }
}

pub struct VMActivePlan<> {}

impl ActivePlan<ScalaNative> for VMActivePlan {
    fn global() -> &'static dyn Plan<VM=ScalaNative> {
        SINGLETON.get_plan()
    }

    fn number_of_mutators() -> usize {
        unsafe { ((*UPCALLS).number_of_mutators)() }
    }

    fn is_mutator(_tls: VMThread) -> bool {
        unsafe { ((*UPCALLS).is_mutator)(_tls) }
    }

    fn mutator(_tls: VMMutatorThread) -> &'static mut Mutator<ScalaNative> {
        unsafe {
            let m = ((*UPCALLS).get_mmtk_mutator)(_tls);
            &mut *m
        }
    }

    fn mutators<'a>() -> Box<dyn Iterator<Item = &'a mut Mutator<ScalaNative>> + 'a> {
        Box::new(ScalaNativeMutatorIterator::new())
    }

}
