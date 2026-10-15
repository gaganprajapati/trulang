//! Embedded Native C Runtime for TruLang.
//!
//! Provides the core Region A bump arena allocator (with elastic burst expansions),
//! Region B persistent heap manager, tagged value representations, checked arithmetic,
//! and standard service dispatch.

pub const TRULANG_RUNTIME_C: &str = r#"
#ifndef TRULANG_RUNTIME_H
#define TRULANG_RUNTIME_H

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <inttypes.h>
#include <math.h>

/* ========================================================================= */
/*                         Region A Bump Arena Allocator                     */
/* ========================================================================= */

typedef struct {
    uint8_t* memory;
    size_t offset;
    size_t budget_bytes;
    size_t max_budget_bytes;
    uint64_t burst_count;
} TruLangRegionA;

static TruLangRegionA g_region_a = {0};

static void trulang_region_a_init(size_t budget_bytes, size_t max_budget_bytes) {
    if (budget_bytes == 0) {
        budget_bytes = 16 * 1024 * 1024; // 16 MB default
    }
    if (g_region_a.memory) {
        free(g_region_a.memory);
    }
    g_region_a.memory = (uint8_t*)malloc(budget_bytes);
    if (!g_region_a.memory) {
        fprintf(stderr, "Fatal: Failed to allocate Region A memory arena of %zu bytes\n", budget_bytes);
        exit(1);
    }
    g_region_a.offset = 0;
    g_region_a.budget_bytes = budget_bytes;
    g_region_a.max_budget_bytes = max_budget_bytes;
    g_region_a.burst_count = 0;
}

static void* trulang_region_a_alloc(size_t size) {
    // 8-byte alignment
    size_t aligned_size = (size + 7) & ~7;
    if (g_region_a.offset + aligned_size > g_region_a.budget_bytes) {
        // Check if elastic burst expansion is permitted
        if (g_region_a.max_budget_bytes > 0) {
            size_t needed = g_region_a.offset + aligned_size;
            if (needed <= g_region_a.max_budget_bytes) {
                size_t new_budget = g_region_a.budget_bytes * 2;
                if (new_budget < needed) new_budget = needed;
                if (new_budget > g_region_a.max_budget_bytes) new_budget = g_region_a.max_budget_bytes;

                uint8_t* new_mem = (uint8_t*)realloc(g_region_a.memory, new_budget);
                if (!new_mem) {
                    fprintf(stderr, "\nVM Runtime Error: Memory budget exceeded: failed to reallocate elastic arena\n");
                    exit(1);
                }
                g_region_a.memory = new_mem;
                g_region_a.budget_bytes = new_budget;
                g_region_a.burst_count++;
            } else {
                fprintf(stderr, "\nVM Runtime Error: Memory budget exceeded: request attempted to allocate %zu bytes, exceeding elastic ceiling of %zu bytes (HTTP 413 Payload Too Large)\n",
                        needed, g_region_a.max_budget_bytes);
                exit(1);
            }
        } else {
            fprintf(stderr, "\nVM Runtime Error: Memory budget exceeded: arena requested %zu bytes exceeding budget of %zu bytes\n",
                    g_region_a.offset + aligned_size, g_region_a.budget_bytes);
            exit(1);
        }
    }

    void* ptr = (void*)(g_region_a.memory + g_region_a.offset);
    g_region_a.offset += aligned_size;
    return ptr;
}

static void trulang_region_a_reset(void) {
    g_region_a.offset = 0;
}

/* ========================================================================= */
/*                         Region B Persistent Allocator                     */
/* ========================================================================= */

typedef struct {
    size_t live_bytes;
    size_t live_objects;
} TruLangRegionB;

static TruLangRegionB g_region_b = {0};

static void* trulang_region_b_alloc(size_t size) {
    size_t aligned_size = (size + 7) & ~7;
    void* ptr = malloc(aligned_size);
    if (!ptr) {
        fprintf(stderr, "Fatal: Out of memory in Region B persistent allocation\n");
        exit(1);
    }
    g_region_b.live_bytes += aligned_size;
    g_region_b.live_objects++;
    return ptr;
}

/* ========================================================================= */
/*                         Tagged Value Representation                       */
/* ========================================================================= */

typedef enum {
    VAL_NULL,
    VAL_BOOL,
    VAL_INT,
    VAL_FLOAT,
    VAL_STRING,
    VAL_MAP,
    VAL_HANDLE
} TruLangValType;

typedef struct TruLangMapEntry {
    char* key;
    struct TruLangVal* value;
    struct TruLangMapEntry* next;
} TruLangMapEntry;

typedef struct TruLangMap {
    size_t capacity;
    size_t count;
    TruLangMapEntry** buckets;
    bool is_persistent;
} TruLangMap;

typedef struct TruLangVal {
    TruLangValType type;
    union {
        bool b;
        int64_t i;
        double f;
        struct {
            char* data;
            size_t len;
            bool is_persistent;
        } s;
        TruLangMap* map;
        struct {
            uint64_t id;
            const char* type_name;
            void* resource;
        } handle;
    } as;
} TruLangVal;

static inline TruLangVal trulang_val_null(void) {
    TruLangVal v;
    v.type = VAL_NULL;
    return v;
}

static inline TruLangVal trulang_val_bool(bool b) {
    TruLangVal v;
    v.type = VAL_BOOL;
    v.as.b = b;
    return v;
}

static inline TruLangVal trulang_val_int(int64_t i) {
    TruLangVal v;
    v.type = VAL_INT;
    v.as.i = i;
    return v;
}

static inline TruLangVal trulang_val_float(double f) {
    TruLangVal v;
    v.type = VAL_FLOAT;
    v.as.f = f;
    return v;
}

static TruLangVal trulang_val_string(const char* str, bool persistent) {
    size_t len = strlen(str);
    char* data = (char*)(persistent ? trulang_region_b_alloc(len + 1) : trulang_region_a_alloc(len + 1));
    memcpy(data, str, len);
    data[len] = '\0';

    TruLangVal v;
    v.type = VAL_STRING;
    v.as.s.data = data;
    v.as.s.len = len;
    v.as.s.is_persistent = persistent;
    return v;
}

static uint32_t trulang_hash_str(const char* s) {
    uint32_t hash = 2166136261u;
    while (*s) {
        hash ^= (uint8_t)(*s++);
        hash *= 16777619;
    }
    return hash;
}

static TruLangMap* trulang_map_create(bool persistent) {
    TruLangMap* m = (TruLangMap*)(persistent ? trulang_region_b_alloc(sizeof(TruLangMap)) : trulang_region_a_alloc(sizeof(TruLangMap)));
    m->capacity = 16;
    m->count = 0;
    m->is_persistent = persistent;
    size_t bsize = sizeof(TruLangMapEntry*) * m->capacity;
    m->buckets = (TruLangMapEntry**)(persistent ? trulang_region_b_alloc(bsize) : trulang_region_a_alloc(bsize));
    memset(m->buckets, 0, bsize);
    return m;
}

static inline TruLangVal trulang_val_map(bool persistent) {
    TruLangVal v;
    v.type = VAL_MAP;
    v.as.map = trulang_map_create(persistent);
    return v;
}

static TruLangVal trulang_map_get(TruLangVal map_val, const char* key) {
    if (map_val.type != VAL_MAP || !map_val.as.map) return trulang_val_null();
    TruLangMap* m = map_val.as.map;
    uint32_t bucket = trulang_hash_str(key) % m->capacity;
    TruLangMapEntry* e = m->buckets[bucket];
    while (e) {
        if (strcmp(e->key, key) == 0) {
            return *(e->value);
        }
        e = e->next;
    }
    return trulang_val_null();
}

static void trulang_map_set(TruLangVal map_val, const char* key, TruLangVal val) {
    if (map_val.type != VAL_MAP || !map_val.as.map) return;
    TruLangMap* m = map_val.as.map;
    bool p = m->is_persistent;
    uint32_t bucket = trulang_hash_str(key) % m->capacity;
    TruLangMapEntry* e = m->buckets[bucket];
    while (e) {
        if (strcmp(e->key, key) == 0) {
            *(e->value) = val;
            return;
        }
        e = e->next;
    }
    // New entry
    TruLangMapEntry* new_entry = (TruLangMapEntry*)(p ? trulang_region_b_alloc(sizeof(TruLangMapEntry)) : trulang_region_a_alloc(sizeof(TruLangMapEntry)));
    size_t klen = strlen(key);
    new_entry->key = (char*)(p ? trulang_region_b_alloc(klen + 1) : trulang_region_a_alloc(klen + 1));
    memcpy(new_entry->key, key, klen + 1);

    new_entry->value = (TruLangVal*)(p ? trulang_region_b_alloc(sizeof(TruLangVal)) : trulang_region_a_alloc(sizeof(TruLangVal)));
    *(new_entry->value) = val;
    new_entry->next = m->buckets[bucket];
    m->buckets[bucket] = new_entry;
    m->count++;
}

static inline bool trulang_is_truthy(TruLangVal v) {
    switch (v.type) {
        case VAL_NULL: return false;
        case VAL_BOOL: return v.as.b;
        case VAL_INT: return v.as.i != 0;
        case VAL_FLOAT: return v.as.f != 0.0;
        case VAL_STRING: return v.as.s.len > 0;
        case VAL_MAP: return v.as.map && v.as.map->count > 0;
        case VAL_HANDLE: return true;
    }
    return false;
}

/* ========================================================================= */
/*                         Checked Arithmetic & Ops                          */
/* ========================================================================= */

static inline TruLangVal trulang_op_add(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) {
        int64_t res;
        if (__builtin_add_overflow(a.as.i, b.as.i, &res)) {
            fprintf(stderr, "\nVM Runtime Error: Integer overflow during addition: %" PRId64 " + %" PRId64 "\n", a.as.i, b.as.i);
            exit(1);
        }
        return trulang_val_int(res);
    }
    if (a.type == VAL_FLOAT || b.type == VAL_FLOAT) {
        double da = (a.type == VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return trulang_val_float(da + db);
    }
    if (a.type == VAL_STRING || b.type == VAL_STRING) {
        char buf_a[64], buf_b[64];
        const char* sa = (a.type == VAL_STRING) ? a.as.s.data : (snprintf(buf_a, sizeof(buf_a), "%" PRId64, a.as.i), buf_a);
        const char* sb = (b.type == VAL_STRING) ? b.as.s.data : (snprintf(buf_b, sizeof(buf_b), "%" PRId64, b.as.i), buf_b);
        size_t total_len = strlen(sa) + strlen(sb);
        char* concat = (char*)trulang_region_a_alloc(total_len + 1);
        strcpy(concat, sa);
        strcat(concat, sb);
        TruLangVal res;
        res.type = VAL_STRING;
        res.as.s.data = concat;
        res.as.s.len = total_len;
        res.as.s.is_persistent = false;
        return res;
    }
    return trulang_val_null();
}

static inline TruLangVal trulang_op_sub(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) {
        int64_t res;
        if (__builtin_sub_overflow(a.as.i, b.as.i, &res)) {
            fprintf(stderr, "\nVM Runtime Error: Integer overflow during subtraction\n");
            exit(1);
        }
        return trulang_val_int(res);
    }
    if (a.type == VAL_FLOAT || b.type == VAL_FLOAT) {
        double da = (a.type == VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return trulang_val_float(da - db);
    }
    return trulang_val_null();
}

static inline TruLangVal trulang_op_mul(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) {
        int64_t res;
        if (__builtin_mul_overflow(a.as.i, b.as.i, &res)) {
            fprintf(stderr, "\nVM Runtime Error: Integer overflow during multiplication: %" PRId64 " * %" PRId64 "\n", a.as.i, b.as.i);
            exit(1);
        }
        return trulang_val_int(res);
    }
    if (a.type == VAL_FLOAT || b.type == VAL_FLOAT) {
        double da = (a.type == VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return trulang_val_float(da * db);
    }
    return trulang_val_null();
}

static inline TruLangVal trulang_op_div(TruLangVal a, TruLangVal b) {
    if (b.type == VAL_INT && b.as.i == 0) {
        fprintf(stderr, "\nVM Runtime Error: Division by zero\n");
        exit(1);
    }
    if (a.type == VAL_INT && b.type == VAL_INT) {
        if (a.as.i == INT64_MIN && b.as.i == -1) {
            fprintf(stderr, "\nVM Runtime Error: Integer overflow during division\n");
            exit(1);
        }
        return trulang_val_int(a.as.i / b.as.i);
    }
    if (a.type == VAL_FLOAT || b.type == VAL_FLOAT) {
        double da = (a.type == VAL_FLOAT) ? a.as.f : (double)a.as.i;
        double db = (b.type == VAL_FLOAT) ? b.as.f : (double)b.as.i;
        return trulang_val_float(da / db);
    }
    return trulang_val_null();
}

static inline TruLangVal trulang_op_mod(TruLangVal a, TruLangVal b) {
    if (b.type == VAL_INT && b.as.i == 0) {
        fprintf(stderr, "\nVM Runtime Error: Division by zero in modulo operation\n");
        exit(1);
    }
    if (a.type == VAL_INT && b.type == VAL_INT) {
        return trulang_val_int(a.as.i % b.as.i);
    }
    return trulang_val_null();
}

static inline TruLangVal trulang_op_neg(TruLangVal a) {
    if (a.type == VAL_INT) return trulang_val_int(-a.as.i);
    if (a.type == VAL_FLOAT) return trulang_val_float(-a.as.f);
    return trulang_val_null();
}

static inline TruLangVal trulang_op_not(TruLangVal a) {
    return trulang_val_bool(!trulang_is_truthy(a));
}

static inline TruLangVal trulang_op_eq(TruLangVal a, TruLangVal b) {
    if (a.type != b.type) return trulang_val_bool(false);
    switch (a.type) {
        case VAL_NULL: return trulang_val_bool(true);
        case VAL_BOOL: return trulang_val_bool(a.as.b == b.as.b);
        case VAL_INT: return trulang_val_bool(a.as.i == b.as.i);
        case VAL_FLOAT: return trulang_val_bool(a.as.f == b.as.f);
        case VAL_STRING: return trulang_val_bool(strcmp(a.as.s.data, b.as.s.data) == 0);
        case VAL_MAP: return trulang_val_bool(a.as.map == b.as.map);
        case VAL_HANDLE: return trulang_val_bool(a.as.handle.id == b.as.handle.id);
    }
    return trulang_val_bool(false);
}

static inline TruLangVal trulang_op_ne(TruLangVal a, TruLangVal b) {
    TruLangVal eq = trulang_op_eq(a, b);
    return trulang_val_bool(!eq.as.b);
}

static inline TruLangVal trulang_op_lt(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) return trulang_val_bool(a.as.i < b.as.i);
    if (a.type == VAL_FLOAT && b.type == VAL_FLOAT) return trulang_val_bool(a.as.f < b.as.f);
    return trulang_val_bool(false);
}

static inline TruLangVal trulang_op_le(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) return trulang_val_bool(a.as.i <= b.as.i);
    if (a.type == VAL_FLOAT && b.type == VAL_FLOAT) return trulang_val_bool(a.as.f <= b.as.f);
    return trulang_val_bool(false);
}

static inline TruLangVal trulang_op_gt(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) return trulang_val_bool(a.as.i > b.as.i);
    if (a.type == VAL_FLOAT && b.type == VAL_FLOAT) return trulang_val_bool(a.as.f > b.as.f);
    return trulang_val_bool(false);
}

static inline TruLangVal trulang_op_ge(TruLangVal a, TruLangVal b) {
    if (a.type == VAL_INT && b.type == VAL_INT) return trulang_val_bool(a.as.i >= b.as.i);
    if (a.type == VAL_FLOAT && b.type == VAL_FLOAT) return trulang_val_bool(a.as.f >= b.as.f);
    return trulang_val_bool(false);
}

/* ========================================================================= */
/*                         Region B Promotion (Persist)                      */
/* ========================================================================= */

static TruLangVal trulang_persist(TruLangVal val) {
    switch (val.type) {
        case VAL_STRING: {
            if (val.as.s.is_persistent) return val;
            return trulang_val_string(val.as.s.data, true);
        }
        case VAL_MAP: {
            if (!val.as.map || val.as.map->is_persistent) return val;
            TruLangMap* old_map = val.as.map;
            TruLangMap* new_map = trulang_map_create(true);
            for (size_t i = 0; i < old_map->capacity; i++) {
                TruLangMapEntry* e = old_map->buckets[i];
                while (e) {
                    TruLangVal pval = trulang_persist(*(e->value));
                    trulang_map_set(trulang_val_null(), e->key, pval); // Helper
                    uint32_t bucket = trulang_hash_str(e->key) % new_map->capacity;
                    TruLangMapEntry* ne = (TruLangMapEntry*)trulang_region_b_alloc(sizeof(TruLangMapEntry));
                    size_t klen = strlen(e->key);
                    ne->key = (char*)trulang_region_b_alloc(klen + 1);
                    memcpy(ne->key, e->key, klen + 1);
                    ne->value = (TruLangVal*)trulang_region_b_alloc(sizeof(TruLangVal));
                    *(ne->value) = pval;
                    ne->next = new_map->buckets[bucket];
                    new_map->buckets[bucket] = ne;
                    new_map->count++;
                    e = e->next;
                }
            }
            TruLangVal res;
            res.type = VAL_MAP;
            res.as.map = new_map;
            return res;
        }
        default:
            return val;
    }
}

/* ========================================================================= */
/*                         Printing & Diagnostic Output                      */
/* ========================================================================= */

static void trulang_val_print(TruLangVal val) {
    switch (val.type) {
        case VAL_NULL: printf("null"); break;
        case VAL_BOOL: printf("%s", val.as.b ? "true" : "false"); break;
        case VAL_INT: printf("%" PRId64, val.as.i); break;
        case VAL_FLOAT: printf("%g", val.as.f); break;
        case VAL_STRING: printf("%s", val.as.s.data); break;
        case VAL_MAP: {
            printf("{");
            if (val.as.map) {
                bool first = true;
                for (size_t i = 0; i < val.as.map->capacity; i++) {
                    TruLangMapEntry* e = val.as.map->buckets[i];
                    while (e) {
                        if (!first) printf(", ");
                        printf("%s: ", e->key);
                        trulang_val_print(*(e->value));
                        first = false;
                        e = e->next;
                    }
                }
            }
            printf("}");
            break;
        }
        case VAL_HANDLE: printf("<Handle: %s #%" PRIu64 ">", val.as.handle.type_name, val.as.handle.id); break;
    }
}

static void trulang_val_println(TruLangVal val) {
    trulang_val_print(val);
    printf("\n");
}

static void trulang_print_diagnostics(void) {
    printf("--- Memory Diagnostics ---\n");
    if (g_region_a.max_budget_bytes > 0) {
        printf("Region A (Execution):  %zu bytes used (base: %zu bytes, max: %zu bytes)\n",
               g_region_a.offset, g_region_a.budget_bytes, g_region_a.max_budget_bytes);
    } else {
        printf("Region A (Execution):  %zu bytes used (budget: %zu bytes)\n",
               g_region_a.offset, g_region_a.budget_bytes);
    }
    if (g_region_a.burst_count > 0) {
        printf("                       (Elastic bursts recorded: %" PRIu64 ")\n", g_region_a.burst_count);
    }
    printf("Region B (Persistent): %zu bytes retained (%zu live objects)\n",
           g_region_b.live_bytes, g_region_b.live_objects);
}

#endif /* TRULANG_RUNTIME_H */
"#;
