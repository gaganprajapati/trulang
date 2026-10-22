# Validation & Acceptance Criteria: Phase 7

> **Spec:** [requirements.md](requirements.md) • **Plan:** [plan.md](plan.md)  
> **Target Release:** `v1.1.0`  

---

## 1. Acceptance Gates

| Gate | Requirement | Status |
| :--- | :--- | :---: |
| **Workspace Tests** | `cargo test --workspace` passes with 0 failures | ⏳ Pending |
| **Linter Quality** | `cargo clippy --workspace -- -D warnings` | ⏳ Pending |
| **Formatting** | `cargo fmt --check` has 0 diffs | ⏳ Pending |
| **TLS Verification** | < 1.0ms TLS 1.3 handshake overhead | ⏳ Pending |
| **WebSocket Scale** | 10,000 concurrent idle WebSockets < 25 MB RSS | ⏳ Pending |
| **REST API Benchmark** | > 75,000 req/sec per core on parameterized routes | ⏳ Pending |

---

## 2. Test Execution Output

```bash
# Output from cargo test --workspace will be recorded here
```

---

## 3. Benchmark Verification

```bash
# Benchmark output (TLS handshake, k6 WebSocket, and REST throughput) will be recorded here
```

---

## 4. Sign-Off Checklist
- [ ] All 6 task groups in `plan.md` checked off.
- [ ] Zero regressions in existing 106 unit & integration tests.
- [ ] Release notes prepared for `v1.1.0`.
