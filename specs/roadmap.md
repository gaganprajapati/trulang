# TruLang Specifications Roadmap 🗺️

> **Overview:** Master index mapping technical phases to their detailed Spec-Driven Development (SDD) feature folders.

---

## 1. Release Milestones & Phase Overview

| Phase | Milestone Name | Target Version | Status | Spec Folder |
| :---: | :--- | :---: | :---: | :--- |
| **1–6** | **TruLang 1.0 General Availability (GA)** | `v1.0.0` | **✅ Completed** | [01-ROADMAP-1.0-GA.md](../roadmaps/completed/01-ROADMAP-1.0-GA.md) |
| **7** | **Native TLS/HTTPS, WebSockets, Web Framework & DBs** | `v1.1.0` | **🎯 Active / Next** | `specs/2026-10-23-phase7-tls-and-web/` |
| **8** | **WebAssembly (WASM & WASI) & Live Browser Playground** | `v1.2.0` | **📋 Planned** | `specs/upcoming/phase8-wasm-wasi/` |
| **9** | **Distributed Actor Clustering & Raft Consensus** | `v1.3.0` | **📋 Planned** | `specs/upcoming/phase9-clustering-raft/` |
| **10** | **Central Package Registry & TPM Infrastructure** | `v1.4.0` | **📋 Planned** | `specs/upcoming/phase10-package-registry/` |
| **11** | **Machine-Code JIT & Cranelift Native Backend** | `v1.5.0` | **📋 Planned** | `specs/upcoming/phase11-cranelift-jit/` |
| **12** | **Enterprise Operational Governance & 2.0 LTS** | `v2.0.0` | **📋 Planned** | `specs/upcoming/phase12-enterprise-lts/` |

---

## 2. SDD Feature Directory Standards

Every feature in the roadmap follows this directory naming and file layout:

```text
specs/YYYY-MM-DD-feature-name/
├── requirements.md     # Scope, architectural background, API designs, memory model impact
├── plan.md             # Series of numbered task groups with atomic checkboxes
└── validation.md       # Exact test commands, benchmark targets, and verification logs
```

---

## 3. Reference Documents

- **North Star & Philosophy:** [specs/mission.md](mission.md)
- **Technical Architecture & Rules:** [specs/tech-stack.md](tech-stack.md)
- **Living Pointer to Active Task:** [specs/active.md](active.md)
- **Go-To-Market & Growth Playbook:** [roadmaps/GROWTH-AND-ADOPTION.md](../roadmaps/GROWTH-AND-ADOPTION.md)
