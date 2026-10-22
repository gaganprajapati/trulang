# Validation & Acceptance Criteria: [Feature Title]

> **Spec:** [requirements.md](requirements.md) • **Plan:** [plan.md](plan.md)  
> **Verified By:** [Name/Agent]  
> **Date:** YYYY-MM-DD  

---

## 1. Acceptance Gates

| Gate | Requirement | Status |
| :--- | :--- | :---: |
| **Workspace Tests** | `cargo test --workspace` passes with 0 failures | ⏳ Pending / ✅ Passed |
| **Linter Quality** | `cargo clippy --workspace -- -D warnings` | ⏳ Pending / ✅ Passed |
| **Formatting** | `cargo fmt --check` has 0 diffs | ⏳ Pending / ✅ Passed |
| **Memory Check** | 0 memory leaks across 10,000 iterations | ⏳ Pending / ✅ Passed |

---

## 2. Test Execution Output

```bash
# Paste the exact test output from cargo test
```

---

## 3. Benchmark & Performance Verification

```bash
# Paste benchmark execution numbers (req/sec, p95 latency, RSS memory)
```

---

## 4. Sign-Off & Merge Readiness
- [ ] All task groups in `plan.md` checked off.
- [ ] No regressions in existing test suite.
- [ ] Ready for commit and release tagging.
