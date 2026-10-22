# Spec-Driven Development (SDD) at TruLang 📐

Welcome to TruLang's Spec-Driven Development hub. Every non-trivial feature, runtime change, and architectural upgrade begins here before any code is written.

---

## 1. Directory Structure

```text
specs/
├── README.md               <-- Workflow instructions and guidelines (this file)
├── mission.md              <-- Product North Star, core philosophy, and non-goals
├── tech-stack.md           <-- Compiler toolchain, crate topology, and memory rules
├── roadmap.md              <-- Milestone index mapping phases to spec folders
├── active.md               <-- Pointer to the currently active feature & next step
│
├── templates/              <-- Standard templates for new features
│   ├── requirements.md     <-- Scope, decisions, APIs, memory allocation rules
│   ├── plan.md             <-- Numbered task groups with atomic checkboxes
│   └── validation.md       <-- Acceptance gates, test commands, benchmark logs
│
└── YYYY-MM-DD-feature-name/<-- Feature-specific spec folders
    ├── requirements.md
    ├── plan.md
    └── validation.md
```

---

## 2. The 3-Step Lifecycle for Every Feature

1. **Step 1: Requirements First (`requirements.md`)**
   - Create a new directory: `specs/YYYY-MM-DD-feature-name/`.
   - Copy `specs/templates/requirements.md` and define the problem, proposed syntax/API, and memory model impact.
   - Align and approve the design before coding.
2. **Step 2: Implementation Plan (`plan.md`)**
   - Copy `specs/templates/plan.md` and break the work into numbered task groups with small, atomic checkboxes.
   - Update `specs/active.md` to point to this directory.
   - Check off tasks as each commit or step passes tests.
3. **Step 3: Validation & Sign-Off (`validation.md`)**
   - Copy `specs/templates/validation.md`.
   - Execute test suites, clippy, and benchmarks.
   - Paste verification output into `validation.md` before merging.
