#ifndef MMTK_H
#define MMTK_H

#include <stdbool.h>
#include <stddef.h>
#include <sys/types.h>
#include <stdint.h>
#include <stdio.h>
#include "../../scala-native/nativelib/src/main/resources/scala-native/gc/immix_commix/headers/ObjectHeader.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef void* MMTk_Mutator;

// This has the same layout as mmtk::util::alloc::AllocationError
typedef enum {
    HeapOutOfMemory,
    MmapOutOfMemory,
} MMTkAllocationError;

extern const uintptr_t GLOBAL_SIDE_METADATA_BASE_ADDRESS;
extern const uintptr_t GLOBAL_SIDE_METADATA_VM_BASE_ADDRESS;
extern const uintptr_t VO_BIT_ADDRESS;
extern const size_t MMTK_MARK_COMPACT_HEADER_RESERVED_IN_BYTES;
extern const uintptr_t FREE_LIST_ALLOCATOR_SIZE;

extern const char* get_mmtk_version();

// Initialize an MMTk instance
extern void mmtk_init(size_t min_heap_size, size_t max_heap_size);

extern size_t mmtk_get_bytes_in_page();

// Request MMTk to create a new mutator for the given `tls` thread
extern MMTk_Mutator mmtk_bind_mutator(void* tls);

// Reclaim mutator that is no longer needed
extern void mmtk_destroy_mutator(MMTk_Mutator mutator);

// Flush mutator local state
extern void mmtk_flush_mutator(MMTk_Mutator mutator);

// Allocate memory for an object
extern void* mmtk_alloc(MMTk_Mutator mutator,
                        size_t size,
                        size_t align,
                        ssize_t offset,
                        int allocator);

// Perform post-allocation hooks or actions such as initializing object metadata
extern void mmtk_post_alloc(MMTk_Mutator mutator,
                            void* refer,
                            int bytes,
                            int allocator);

extern void mmtk_initialize_collection(void* tls);

// This type declaration needs to match AllocatorSelector in mmtk-core
typedef struct {
    uint8_t tag;
    uint8_t index;
} AllocatorSelector;

#define TAG_BUMP_POINTER              0
#define TAG_LARGE_OBJECT              1
#define TAG_MALLOC                    2
#define TAG_IMMIX                     3
#define TAG_MARK_COMPACT              4
#define TAG_FREE_LIST                 5

extern AllocatorSelector get_allocator_mapping(int allocator);
extern size_t get_max_non_los_default_alloc_bytes();

/**
 * Finalization
 */
extern void mmtk_add_finalizer(void* obj);
extern void* mmtk_get_finalized_object();
extern void mmtk_gc_init(size_t heap_size);
// Return if object pointed to by `object` will never move
extern bool mmtk_will_never_move(void* object);
// Process an MMTk option. Return true if option was processed successfully
extern bool mmtk_process(char* name, char* value);
// Process MMTk options. Return true if all options were processed successfully
extern bool mmtk_process_bulk(char* options);
// Sanity only. Scan heap for discrepancies and errors
extern void mmtk_scan_region();
// Trigger a garbage collection as requested by the user.
extern void mmtk_handle_user_collection_request(void *tls);

extern void mmtk_start_control_collector(void *tls, void *context);
extern void mmtk_start_worker(void *tls, void* worker);

extern bool mmtk_is_mmtk_object(void* addr);

extern void release_buffer(void** buf, size_t size, size_t capa);
extern void* mmtk_starting_heap_address();
extern void* mmtk_last_heap_address();

extern void mmtk_append_pinned_objects(uintptr_t* const *data, size_t len);
extern bool mmtk_pin_object(uintptr_t* addr);

/**
 * VM Accounting
 */
extern size_t free_bytes();
extern size_t total_bytes();

typedef struct {
    void** buf;
    size_t cap;
} NewBuffer;

typedef struct {
    void (*func)(MMTk_Mutator mutator, void* data);
    void* data;
} MutatorClosure;

typedef struct {
    NewBuffer (*func)(void** buf, size_t size, size_t capa, void* data);
    void* data;
} EdgesClosure;

typedef struct {
    NewBuffer (*func)(void** buf, size_t size, size_t capa, void* data);
    void* data;
} NodesClosure;

void invoke_MutatorClosure(MutatorClosure* closure, MMTk_Mutator mutator);
NewBuffer invoke_EdgesClosure(EdgesClosure* closure, void** buf, size_t size, size_t capa);
NewBuffer invoke_NodesClosure(NodesClosure* closure, void** buf, size_t size, size_t capa);
extern void invoke_mutator_closure(MutatorClosure* closure, MMTk_Mutator mutator);
extern void visit_edge(void* edge_visitor, void* edge);

typedef struct {
    int kind;
    void *gc_context;
} MMTk_GCThreadTLS;
typedef MMTk_GCThreadTLS* MMTk_VMWorkerThread;

typedef struct {
    uintptr_t **stackTop;
    uintptr_t **stackBottom;
} StackRange;

typedef struct {
    uintptr_t **regs;
    size_t regsSize;
} RegsRange;

typedef struct {
    void* ptr;
} SendCtxPtr;

typedef struct {
    void (*stop_all_mutators) (void *tls, bool scan_mutators_in_safepoint, MutatorClosure closure);
    void (*resume_mutators) (void *tls);
    void (*block_for_gc) (void *tls);
    void (*out_of_memory) (void* tls, MMTkAllocationError err_kind);
    void (*schedule_finalizer) ();

    int (*get_object_array_id) ();
    int (*get_weak_ref_ids_min) ();
    int (*get_weak_ref_ids_max) ();
    int (*get_weak_ref_field_offset) ();
    int (*get_array_ids_min) ();
    int (*get_array_ids_max) ();
    size_t (*get_allocation_alignment) ();

    StackRange (*mmtk_get_stack_range) (void* thread);
    RegsRange (*mmtk_get_regs_range) (void* thread);
    word_t* (*mmtk_get_modules)();
    int (*mmkt_get_modules_size)();

    void (*scan_roots_in_all_mutator_threads) (NodesClosure closure);
    void (*scan_roots_in_mutator_thread) (NodesClosure closure, void* tls);
    void (*scan_vm_specific_roots) (NodesClosure closure);
    void (*prepare_for_roots_re_scanning) ();
    void (*mmtk_obj_iterate) (const Object* obj, void* closure);
    void (*mmtk_array_iterate) (const ArrayHeader* obj, void* closure);
    void (*weak_ref_stack_nullify) ();
    void (*weak_ref_stack_call_handlers) ();

    void (*get_mutators) (MutatorClosure closure);
    bool (*is_mutator) (void* tls);
    size_t (*number_of_mutators) ();
    void* (*get_mmtk_mutator) (void* tls);

    void (*init_gc_worker_thread) (MMTk_GCThreadTLS *gc_worker_tls, SendCtxPtr ctx_ptr);
    MMTk_GCThreadTLS* (*get_gc_thread_tls) ();
    void (*init_synchronizer_thread) ();
} ScalaNative_Upcalls;

extern void scalanative_gc_init(ScalaNative_Upcalls *calls);
extern void mmtk_init_binding(const ScalaNative_Upcalls *upcalls);

#ifdef __cplusplus
}
#endif

#endif  // MMTK_H
