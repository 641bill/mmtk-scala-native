#include "mmtk.h"

void invoke_MutatorClosure(MutatorClosure* closure, MMTk_Mutator mutator) {
    closure->func(mutator, closure->data);
}

NewBuffer invoke_EdgeClosure(EdgesClosure* closure, void** buf, size_t size, size_t capa) {
    return closure->func(buf, size, capa, closure->data);
}