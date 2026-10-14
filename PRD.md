# Language Design Specification
## Memory-Safe Programming Language & Runtime

**Status:** Initial technical specification  
**Purpose:** Build a programming language that prevents common memory-retention bugs by design, provides predictable memory management, and identifies the exact code responsible for memory growth.

## 1. Vision and Goals

Build a general-purpose programming language optimized initially for backend services, APIs, and long-running applications.

The language must enforce explicit state ownership, controlled mutation, safe memory lifetimes, and bounded resource consumption. Its runtime must monitor memory usage and provide actionable diagnostics when memory grows unexpectedly.

**Primary objectives:**
- Prevent accidental memory retention through shared mutable references, closures, and global state.
- Separate short-lived execution memory from long-lived persistent memory.
- Prevent or contain memory exhaustion.
- Identify growing allocations, their source locations, object types, and retaining owners.
- Minimize runtime overhead and unnecessary object copying.

The objective is to make common memory leaks impossible where statically enforceable and contain or diagnose the remaining cases. An absolute guarantee that applications will never crash is not realistic.

## 2. Core Language Rules

### 2.1 No global variables

- Prohibit mutable global variables and implicit process-wide object storage.
- Permit compile-time constants and immutable metadata where safe.
- Require persistent state to belong to an explicitly declared service, resource, or other managed owner.
- Make dependencies explicit through function parameters, service contexts, or controlled resource access.

### 2.2 Ownership and mutation

Every mutable value must have a clearly defined owner.

- A variable's declaring scope controls its reassignment.
- Other functions receive read-only access by default.
- Nested functions cannot modify variables belonging to enclosing scopes.
- Functions must return updated values when callers need to replace their own variables.
- In-place mutation may be permitted only through an explicit, compiler-checked exclusive-access mechanism.
- Conflicting mutable and read-only access to the same object must be rejected.

### 2.3 Aliasing and read-only propagation

Ordinary assignment must never silently create a mutable alias or duplicate an object.

- Assigning an object to another variable creates read-only sharing by default.
- Read-only access is transitive: nested objects cannot be mutated through a read-only reference.
- The compiler must track aliases and prevent mutation through alternate references while read-only borrows are active.
- When the borrow ends, the owner may regain exclusive mutable access if the lifetime and ownership rules permit it.
- Aliasing must never permit access to freed or invalid memory.

### 2.4 No implicit copying

- Prohibit implicit deep copies and unrestricted general-purpose copying.
- Independent values must be created through explicit construction.
- Ownership may be transferred without duplicating the underlying object.
- The original owner must lose access when ownership is moved.
- Type-specific duplication may be considered later, with explicit allocation semantics.
- External resources such as sockets and file handles must not be copied implicitly.

### 2.5 Lifetime safety

The compiler must prevent references from outliving their owners.

- Local values cannot escape their valid scope through returns, closures, or stored references.
- Request-scoped values cannot silently enter persistent storage.
- Cross-scope or cross-region transfers must be explicitly validated.
- Asynchronous tasks must not outlive data they depend on.
- Invalid ownership transfers must produce compile-time errors whenever statically detectable.

## 3. Memory Architecture

The runtime must maintain two separately accounted memory regions.

### Region A: Execution Memory

Used for temporary allocations associated with a request, operation, or task.

Requirements:
- Allocate temporary data within a defined scope.
- Use arena or region-based allocation where appropriate.
- Support per-operation and aggregate memory budgets.
- Reclaim eligible allocations in bulk when their scopes end.
- Handle exceptions, cancellation, and asynchronous work safely.
- Prevent temporary references from escaping into longer-lived regions without a valid transfer.

### Region B: Persistent Memory

Used for data that must survive individual operations, including caches, service state, and long-lived application objects.

Requirements:
- Every allocation must have an identifiable owner.
- Support explicit memory budgets and collection limits.
- Require appropriate cleanup, expiration, eviction, or reclamation policies.
- Track object counts, allocation sites, live bytes, and object lifetimes.
- Prevent request-local references from being retained accidentally.
- Use a suitable reclamation strategy, such as ownership-based destruction or tracing garbage collection, for supported object types.

Persistent memory is not exempt from leak prevention. Unbounded caches and long-lived reference chains must remain detectable and controllable.

## 4. Memory Supervisor

Implement a dedicated monitoring process separate from the application process.

### Responsibilities

1. Receive allocation metrics and runtime telemetry.
2. Monitor execution and persistent memory independently.
3. Identify abnormal growth rates and sustained memory pressure.
4. Attribute allocations to source locations, variable or allocation labels, object types, and owning scopes.
5. Identify retaining references when detailed diagnostics are needed.
6. Generate actionable alerts and support protective actions.

### Required diagnostics

Each memory report should include, where available:

- Application, worker, and memory region.
- Source file, line number, and allocation site.
- Object type and object count.
- Current live bytes and growth over time.
- Allocation and reclamation rates.
- Owning scope or persistent resource.
- Object retention information and relevant reference paths.
- Memory budget and utilization.
- Suggested investigation or protective action.

The runtime must distinguish allocated bytes, live bytes, reachable objects, and retained memory. Shared objects must not be double-counted.

Use lightweight counters continuously. Collect expensive heap or retention snapshots selectively when thresholds are exceeded.

## 5. Memory Protection and Failure Isolation

Monitoring alone cannot guarantee application survival. The runtime must enforce limits.

- Configure per-operation, per-region, per-worker, and aggregate memory budgets.
- Apply backpressure when memory or concurrency limits are approached.
- Reject or safely abort operations that exceed their budgets.
- Evict eligible persistent cache entries where configured.
- Support graceful load shedding and worker replacement.
- Preserve useful diagnostics when a worker becomes unhealthy.
- Reserve sufficient resources for critical runtime operations.
- Use operating-system or container memory limits as an additional safety boundary.

The supervisor must continue operating independently when an application worker fails, subject to its own resource limits and failure handling.

## 6. Resource Lifecycle Management

Memory safety alone does not prevent leaks of external resources.

The language and runtime must provide explicit lifecycle management for:

- Event listeners and subscriptions.
- Timers and scheduled callbacks.
- Network sockets and connections.
- File handles and streams.
- Background tasks and queues.
- Native resources and foreign-function interfaces.

Resources must have an identifiable owner and deterministic cleanup or cancellation behavior. Unbounded queues and subscriptions must support capacity limits and backpressure.

Native libraries and external runtimes must be included in process-level monitoring, even when their allocations cannot be attributed directly to language variables.

## 7. Concurrency and Asynchronous Execution

The language must define safe behavior for concurrent tasks.

- Shared data is read-only by default.
- Mutable access requires exclusive ownership or another explicitly safe synchronization mechanism.
- Cancellation must trigger appropriate resource cleanup.
- Memory cannot be reclaimed while active operations still depend on it.
- Background tasks must declare their ownership and lifetime.
- Task creation and concurrency must support configurable limits.
- Cross-worker communication must use controlled message passing or other explicitly defined interfaces.

## 8. Concrete Compiler and Runtime Architecture Specification

Following the architectural design review, the technical stack and system design are formalized as follows:

### 8.1 Execution Engine
- **Stack-based Bytecode Virtual Machine** written in Rust.
- Instruction set includes explicit opcodes for stack frames, memory region switching, telemetry counters, and exception handling unwinding.
- Enables precise allocation attribution, clean frame-level memory boundaries, and continuous telemetry streaming to the supervisor without native code complexity.

### 8.2 Grammar and Syntax Conventions
The language adopts a modern **TypeScript/Rust hybrid syntax**:
- **Services (Actors):** `service UserService { ... }`
- **Functions:** `fn handle(request: Request) -> Response { ... }`
- **Bindings:** `let x: i64 = 10;` (immutable), `let mut counter: i64 = 0;` (exclusive mutable)
- **Nullable Types:** `T?` (e.g. `User?`), with safe navigation `user?.name` and explicit null checks.
- **Error Handling:** `try { ... } catch (err: Exception) { ... }` with VM call-frame unwinding and deterministic ARC/arena cleanup.
- **Resource Cleanup:** Scoped RAII auto-disposal and explicit `defer expr` statements executed in LIFO order upon function exit or exception unwinding.
- **Cross-Region Promotion:** Explicit `persist(value)` keyword that relocates or deep-copies objects from Region A (Execution) to Region B (Persistent).

```text
service UserService {
    memory_budget request = 16 MB;
    memory_budget persistent = 512 MB;

    let mut cache: Map<String, User> = Map::new();

    fn handle(request: Request) -> Response {
        defer log_request_completed(request.id);

        try {
            let user = database.find_user(request.user_id);
            if user != null {
                // Promote user data to persistent service cache
                cache.insert(request.user_id, persist(user));
            }
            return Response.json(user);
        } catch (err: DatabaseError) {
            return Response.error(500, err.message);
        }
    }
}
```

### 8.3 Concurrency & Service Actor Architecture
- **Service-as-Actor:** Each `service` represents an isolated actor with its own Persistent Memory (Region B) and a single-threaded message mailbox (Erlang-style).
- Messages/requests are processed sequentially from the mailbox; all I/O is non-blocking (async).
- Internal service state requires no mutexes or locks because execution within a service actor is single-threaded. Concurrency and parallel throughput scale across multiple service actor instances.
- Region A (Execution Memory) is instantiated as a fast bump-allocated arena per request/message and wiped in bulk upon completion.

### 8.4 Memory Supervisor & IPC
- **Multi-process architecture:** The application worker and supervisor run as separate OS processes.
- **Telemetry transport:** Shared memory (`mmap`) lock-free ring buffers. The worker writes lightweight allocation counters and heartbeats without syscalls. If the worker is OOM-killed by the OS, the supervisor retains diagnostic data.
- **Control channel:** Unix Domain Sockets (UDS) used for supervisor-initiated commands (e.g. requesting detailed heap retention snapshots or initiating graceful worker restarts).

### 8.5 Modular Cargo Workspace
- `crates/syntax`: Lexer, source spans, AST definitions, parser, error diagnostics.
- `crates/compiler`: Symbol resolution, type checker, lexical borrow checker, bytecode compiler.
- `crates/vm`: Stack-based bytecode VM, Region A arena allocator, Region B ARC runtime, exception unwinding.
- `crates/supervisor`: Shared memory ring buffer reader, telemetry metrics, and diagnostic reporting.
- `crates/cli`: Command-line tool (`vlang`) providing `run`, `check`, and `monitor` commands.

## 9. Development Milestones

**Phase 1 — Language foundation**
- Define grammar, types, variables, functions, and scopes.
- Implement parsing, AST construction, and basic interpretation.
- Add source-location-aware errors.

**Phase 2 — Ownership and immutability**
- Implement read-only aliasing and transitive immutability.
- Enforce ownership transfer and exclusive-access rules.
- Reject invalid lifetime escapes and conflicting aliases.
- Test nested functions and closures.

**Phase 3 — Memory regions**
- Implement execution and persistent allocators.
- Add region budgets, safe reclamation, and allocation accounting.
- Validate cross-region transfers and asynchronous lifetimes.

**Phase 4 — Memory Supervisor**
- Build the independent monitoring process.
- Add allocation-site tracking, growth detection, alerts, and diagnostic snapshots.
- Test supervisor behavior during worker failure and memory pressure.

**Phase 5 — Production safeguards**
- Add resource lifecycle management, queue limits, cancellation, backpressure, and worker isolation.
- Integrate process-level memory limits and recovery mechanisms.
- Benchmark overhead under representative workloads.

**Phase 6 — Advanced capabilities**
- Consider native compilation, optimization, improved type inference, debugging tools, and a standard library.

## 10. Acceptance Criteria

The initial implementation is not complete until it passes the following tests.

1. Global mutable state and implicit persistent storage are rejected.
2. A child function cannot mutate a variable owned by its parent.
3. Read-only aliases cannot mutate shared objects through nested references or alternate aliases.
4. Conflicting mutable and read-only access is rejected.
5. Implicit copying is prohibited and ownership transfer is validated.
6. Request-scoped references cannot escape into persistent state unsafely.
7. Repeated request workloads reclaim eligible temporary memory rather than accumulating it indefinitely.
8. Deliberately growing caches are identified and constrained by configured policies.
9. Growing allocation sites can be traced to source locations and owners.
10. Listener, timer, queue, and resource leaks are detected or prevented through lifecycle controls.
11. Memory-budget violations trigger the documented protective behavior.
12. A failed application worker does not automatically terminate the independent supervisor.
13. Memory pressure, cancellation, exceptions, and concurrent workloads are tested.
14. Benchmark results quantify runtime overhead, allocation throughput, and diagnostic costs.

## 11. Design Principles That Must Not Be Compromised

- No implicit global mutable state.
- No implicit copying.
- Read-only access by default.
- Explicit ownership and controlled mutation.
- Compiler-enforced lifetime safety.
- Separate accounting for execution and persistent memory.
- Runtime-enforced resource budgets.
- Source-attributed memory diagnostics.
- Safe cleanup for asynchronous operations and external resources.
- Graceful failure containment instead of relying on alerts alone.

## 12. Production Hardening, Security, & Resource Boundary Guarantees

The VLang runtime enforces strict security, resource exhaustion, and concurrency boundaries:

1. **Shared Memory Directory Isolation & Permissions (P0)**:
   - Shared memory segments are never stored in world-writable `/tmp`.
   - VLang targets `$XDG_RUNTIME_DIR/vlang` (fallback: `~/.vlang/shm/` or `/tmp/vlang_runtime_{uid}/`).
   - Directories enforce POSIX permissions `0700` (user-only), and memory-mapped telemetry files enforce `0600` permissions.
2. **Atomic Seqlock Telemetry Ring Buffer (P0)**:
   - Multi-process metrics avoid torn reads across memory boundaries via a 64-bit sequence lock (Seqlock).
   - Writers acquire an odd sequence counter before writing 56-byte records, issuing memory barriers, and publishing an even sequence upon completion.
   - Readers verify sequence consistency before and after reading without holding OS mutex locks.
3. **Checked Arithmetic & Panic Elimination (P0)**:
   - Arithmetic operations (`+`, `-`, `*`, `/`, `%`, `neg`) use checked operations.
   - Host Rust process crashes on edge cases like `i64::MIN / -1` or integer overflow are caught and converted to language-level `VmError::ArithmeticOverflow` or `VmError::DivisionByZero`.
4. **Denial-of-Service (DoS) Recursion & Stack Bounds (P1)**:
   - Parser enforces a strict recursion depth limit (`MAX_RECURSION_DEPTH = 64`) preventing recursive AST stack blowouts.
   - Virtual Machine bounds operand stack (`MAX_STACK_DEPTH = 65,536`) and call stack (`MAX_CALL_FRAMES = 4,096`), terminating runaway recursions with `VmError::StackOverflow`.
5. **Bounded Mailboxes & Load-Shedding Backpressure (P1)**:
   - `ServiceActor` enforces a configurable `mailbox_capacity` (default: 10,000 requests).
   - Exceeding the queue capacity rejects incoming requests immediately with `ActorError::MailboxFull` (enabling HTTP 429/503 fast rejection) rather than buffering into unbounded latency.
6. **Region B Reference Cycle Detection (P2)**:
   - Object promotion via `persist()` executes a cycle detection graph walk (`detect_value_cycles`).
   - Self-referential cyclic structures are rejected with `VmError::CyclicPersistentData` to prevent permanent leaks in the ARC persistent allocator.
7. **Cooperative Preemption & Instruction Quotas (P2)**:
   - Virtual Machine execution loops support optional step limits (`max_instructions`), preempting runaway user loops and avoiding worker starvation with `VmError::TimeLimitExceeded`.

**Expected outcome:** A language and runtime in which common memory-retention bugs are prevented wherever possible, remaining growth is observable and attributable, and memory exhaustion is contained before it causes wider service failure.