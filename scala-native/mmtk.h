#ifndef EXPERIMENTAL_MMTK_H
#define EXPERIMENTAL_MMTK_H

#include <stdbool.h>
#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void* MMTk_Mutator;

// Initialize an MMTk instance
extern void mmtk_init(size_t heap_size);

// Request MMTk to create a new mutator for the given `tls` thread
extern MMTk_Mutator mmtk_bind_mutator(void* tls);

// Allocate memory for an object
extern void* mmtk_alloc(MMTk_Mutator mutator,
                        size_t size,
                        size_t align,
                        ssize_t offset,
                        int allocator);

// Add any additional function declarations specific to ExperimentalGC below this line

#ifdef __cplusplus
}
#endif

#endif  // EXPERIMENTAL_MMTK_H
