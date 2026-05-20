# AI Agent Workflow for the Rust JavaScriptCore Rewrite

This rewrite should be breadth-first before depth-first.

The goal is not to translate JavaScriptCore from C++ to Rust file by file or line
by line. JavaScriptCore relies on C++ ownership, raw pointers, manual lifetime
rules, placement allocation, type punning, macro-heavy dispatch, and runtime/JIT
coupling. Rust requires a different architecture. Agents must first understand
the responsibilities and invariants of the existing engine, then design Rust
components around those responsibilities.

## Core Rule

Agents may fill in components, but they must not discover the engine
architecture while implementing.

Implementation work should begin only after the relevant ownership model,
module boundary, public API, invariants, and test expectations are documented.

The main agent must manage priority before assigning work. At every stage it
should ask what the most important missing building block is, what depends on
it, what can run in parallel, and whether a proposed task is only local tuning.

The main agent is the architect and lead reviewer. Large implementation work
belongs to sub-agents with clear ownership and verification requirements. The
main agent should personally implement only trivial glue, small corrections, or
already-started low-risk fixes.

## Why Not Start With a Tiny Executable Path?

A small JavaScript program such as:

```js
let x = 1 + 2;
x;
```

looks like a narrow milestone, but it immediately forces decisions about:

- value representation
- heap ownership
- rooting
- object allocation
- lexical environments
- bytecode or AST execution
- error handling
- global object shape
- string and identifier interning
- call frame layout

If those decisions are not made first, an agent will tend to chase local compiler
errors and make accidental architectural choices. In Rust this commonly leads to
temporary ownership workarounds, repeated interface churn, or designs that cannot
scale to the full engine.

The first milestone should therefore be a coherent design skeleton, not a tiny
interpreter path.

## Recommended Workflow

1. Choose the current priority.

   Decide which engine gap matters most now. Prefer missing shared
   infrastructure over one failing local path. Identify serial dependencies and
   parallelizable work before editing code.

2. Inventory the JavaScriptCore subsystem.

   Read the relevant C++ source and document what the subsystem owns, mutates,
   assumes, and exposes. The output should be a design note, not code.

3. Translate responsibilities into Rust design.

   Define the Rust modules, structs, traits, ownership relationships, mutation
   rules, and unsafe boundaries that replace the C++ design.

4. Create skeleton APIs before implementation.

   Add files, modules, public types, method names, and comments that explain
   what each part is responsible for. Placeholder methods are acceptable when
   the contract is clear.

5. Freeze component contracts before assigning implementation.

   Agent implementation tasks should be bounded by documented interfaces.
   Agents should not redesign neighboring systems while filling in one module.

6. Integrate only after enough pieces have stable contracts.

   The first executable JavaScript path should come after core contracts for
   values, heap ownership, runtime objects, bytecode boundaries, and VM frames
   are already sketched.

7. Record sparse progress.

   Add one line to `progress.md` only after a major task is complete and gated.
   Do not turn the progress log into a second design document.

## Good Agent Tasks

- Document how a JavaScriptCore subsystem works and identify its Rust ownership
  implications.
- Define skeleton Rust modules and public types for one subsystem.
- Implement one component behind an already documented API.
- Add tests for a narrow component whose contract is already known.
- Fill in tracing or property lookup behavior without changing public VM APIs.

## Bad Agent Tasks

- Make a JavaScript program execute end-to-end without prior architecture.
- Rewrite a whole subsystem from C++ directly into Rust.
- Port files one by one.
- Fix compiler errors by changing ownership boundaries opportunistically.
- Introduce broad `Rc<RefCell<_>>` usage to bypass unresolved design questions.
- Spend a long time making one small test pass when the missing dependency is a
  larger subsystem contract.
- Expand a local feature while GC, handles, jobs, modules, or other shared
  infrastructure remains the higher priority.

## Documentation Expectations

Each subsystem design note should answer:

- Which JavaScriptCore files and classes were reviewed?
- What is this subsystem responsible for?
- What does it own?
- What can mutate it?
- Which invariants does the C++ implementation rely on?
- Which Rust types replace the C++ concepts?
- Where is `unsafe` allowed, if anywhere?
- What tests will prove the design works?

## Design Direction

The Rust engine should be designed around JavaScriptCore's responsibilities, not
JavaScriptCore's file layout.

The ownership model should be explicit before code is written. For example, if
the design uses a garbage-collected heap, the heap should be the owner of
JavaScript-managed cells, while Rust stack references should go through a
rooting or handle discipline. That decision affects values, objects, bytecode,
the VM, builtins, and tests, so it cannot be discovered piecemeal.

Breadth-first design keeps agents from getting trapped in a small local path
that repeatedly changes the global architecture. Depth-first implementation
should happen only after the surrounding contracts are clear.
