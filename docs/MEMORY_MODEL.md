# TruLang Memory Model: The Dual-Region Allocation Engine 🧠

> **How TruLang decides whether to allocate data in Region A (transient) or Region B (persistent), eliminates garbage collection pauses, and enforces memory safety.**

---

## 1. The Core Philosophy

Traditional backend runtimes force an agonizing trade-off:
- **Tracing GC Languages (Go, Java, Node.js):** Developers don't think about memory, but the runtime incurs unpredictable Stop-the-World GC pauses (15–120 ms) and high memory overhead under load.
- **Manual / Borrow-Checked Systems (Rust, C++):** Zero GC pauses, but developers must constantly annotate lifetimes (`'a`), manage reference counters (`Arc<Mutex<T>>`), and battle the compiler.

**TruLang's Dual-Region Architecture eliminates this trade-off.**  
In 95%+ of backend microservice operations, data has a strictly predictable lifecycle: it is either **ephemeral to the current request** or **persistent across the service**.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       TRULANG DUAL-REGION ARCHITECTURE                      │
├──────────────────────────────────────┬──────────────────────────────────────┤
│    Region A (Per-Request Arena)      │      Region B (Long-Lived State)     │
├──────────────────────────────────────┼──────────────────────────────────────┤
│ • Ingress wire frame buffers         │ • Persistent database connection pool│
│ • Query parameter strings & tokens   │ • Service actor fields & state       │
│ • Serialized JSON response chunks    │ • In-memory cache entries            │
│ • 0 pointer tracing / 0 GC sweeps    │ • Acyclic Directed Graph (DAG)       │
│ • O(1) Instant bulk reset on finish  │ • Atomic Reference Counting (ARC)    │
└──────────────────────────────────────┴──────────────────────────────────────┘
```

---

## 2. The Golden Rule of Allocation

> **By default, *everything* begins in Region A.**  
> Data is *only* allocated in or promoted to Region B when it escapes into long-lived actor state or is explicitly preserved via `persist()`.

```
                        New Value Instantiated
                                   │
                                   ▼
                       Allocated in REGION A
                     (Sequential Bump Pointer)
                                   │
                ┌──────────────────┴──────────────────┐
                │                                     │
         Request Completes                    Does it escape to
                │                             Persistent State?
                ▼                                     │
       O(1) Arena Reset                               ▼
       (Pointer offset = 0)                   PROMOTED TO REGION B
       All memory freed at once               via:
       [0.0 ms GC Pause]                      1. persist(val)
                                              2. Actor field assignment
                                              3. Compiler Escape Analysis
```

---

## 3. How TruLang Decides: The 3 Decision Mechanisms

### Mechanism 1: Default Allocation in Region A (Transient Execution Arena)
Any local variable, parsed payload, database query result, or intermediate calculation created inside a function or message handler is placed into Region A:

```trulang
receive(msg: HttpPostRequest) -> HttpResponse {
    // Both 'payload' and 'token' are allocated in Region A
    let payload = json::parse(msg.body);
    let token = extract_bearer_token(msg.headers);

    let user_dto = UserDTO {
        id: payload.user_id,
        name: payload.name,
        role: "standard"
    };

    return HttpResponse::json(200, user_dto);
} // <--- Request exits: Region A offset resets to 0 in 1 CPU cycle.
```

- **Allocation Speed:** Sequential pointer increment (`offset += size`). Zero malloc overhead.
- **Deallocation Speed:** When the handler completes, `arena.offset = 0`. The entire arena is reclaimed in **a single CPU instruction ($O(1)$)**.
- **GC Overhead:** Exactly **0.0 ms**.

---

### Mechanism 2: Explicit Promotion via `persist(value)`
When a developer intentionally wants an object to survive across requests (e.g., caching a record in an actor-level map), they call `persist()`:

```trulang
service UserService {
    let mut cache: Map<int, User> = Map::new(); // Resides in Region B

    receive(msg: GetUser) -> User {
        let user = db.query_user(msg.id); // Allocated in Region A
        
        // Explicit Promotion: Deep-copied from Region A into Region B
        let cached_user = persist(user);
        cache.insert(msg.id, cached_user);

        return user;
    }
}
```

#### What the Runtime Does During `persist()`:
1. **Deep Promotion:** Copies the value and its child fields into Region B's reference-counted heap.
2. **DAG Cycle Detection:** The runtime walks the reference graph using `detect_value_cycles`. If circular references are found, it halts with `VmError::CyclicPersistentData`.
3. **Deterministic ARC:** Because Region B is mathematically guaranteed to be an Acyclic Directed Graph (DAG), it uses lightweight Atomic Reference Counting. It **never needs a mark-sweep GC**, and memory frees deterministically when unreferenced.

---

### Mechanism 3: Compiler Escape Analysis & Auto-Promotion
If a developer assigns a transient Region A variable directly to an actor field without explicitly writing `persist()`, the **TruLang Compiler’s Escape Analysis** detects the scope crossing:

```trulang
service SessionService {
    let mut active_tokens: Set<string> = Set::new(); // Region B

    receive(msg: LoginRequest) {
        let token = msg.session_token; // Originally in Region A
        
        // Compiler Escape Analysis detects assignment to actor state:
        // Automatically emits `OP_PROMOTE_REGION_B` bytecode instruction!
        active_tokens.insert(token);
    }
}
```

The compiler guarantees safety: developers don't have to remember `persist()` for basic assignments, yet memory safety is mathematically preserved.

---

## 4. Fundamental Memory Invariants

TruLang enforces three strict architectural invariants:

| Invariant | Rule | Enforcement |
|---|---|:---:|
| **1. Directional Safety** | Region B pointers **can NEVER point to Region A**.<br>(Region A is wiped on request completion; dangling pointers are impossible). | Compiler + Runtime |
| **2. Upward Reading** | Region A **CAN freely point to Region B**.<br>(Reading persistent caches from transient code is zero-copy and zero-cost). | Permitted |
| **3. Acyclic DAG** | Region B **can NEVER contain cyclical references**.<br>(Eliminates the need for a cycle-collecting garbage collector). | `detect_value_cycles` |

---

## 5. Elastic Request Budgets & Spikes

What happens if an unexpected traffic spike sends a 32 MB payload into an 8 MB Region A arena?

TruLang uses **Elastic Burst Arenas**:
```trulang
// Declared at the service level:
memory_budget request = 16 MB max 64 MB;
```

1. **Base Arena (16 MB):** Fast pre-allocated contiguous buffer.
2. **Burst Arena (Up to 64 MB):** If a request exceeds 16 MB, TruLang elastically expands into a burst buffer without failing.
3. **Hard Ceiling (64 MB):** If memory exceeds 64 MB, the request is immediately dropped with `HTTP 413 Payload Too Large` to protect the host node from OOM killers.
4. **Instant Reclamation:** Upon request completion, the burst buffer is dropped and Region A resets cleanly back to its 16 MB baseline.

---

## 6. Summary Comparison: Allocation Decision Matrix

| Data Type / Scenario | Target Region | Decision Mechanism | Memory Reclamation |
|---|:---:|---|---|
| **Local variables & arguments** | **Region A** | Default runtime rule | $O(1)$ single-cycle arena reset |
| **HTTP request bodies & JSON ASTs** | **Region A** | Default runtime rule | $O(1)$ single-cycle arena reset |
| **Database query records & rows** | **Region A** | Default runtime rule | $O(1)$ single-cycle arena reset |
| **Actor root fields (`let mut x`)** | **Region B** | Declared at actor root scope | Reference-counted DAG (freed on actor shutdown) |
| **Objects passed to `persist(x)`** | **Region B** | Explicit developer primitive | Reference-counted DAG with cycle detection |
| **Transients assigned to Actor fields** | **Region B** | Compiler Escape Analysis | Auto-promoted with cycle check |
