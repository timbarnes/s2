# 02. Approaches

How to structure the project. These are the project-level choices; the
design inside the compiler and VM is in 03. Each option is judged on:

- **Size:** how much code the project holds, and how manageable it stays.
- **Learning:** how clearly it shows how a compiler and VM work and fit
  together.
- **Risk:** the chance of getting stuck, or of breaking working code.
- **Speed ceiling:** how fast the result can eventually be.
- **Reuse:** how much of s1 it uses, and at what cost.

## A. Extend s1 in place

Add the compiler and VM to s1. Begin with s1's own "technique D"
(`design/precompilation-design.md`): pre-analyse forms into a node tree,
then compile nodes to bytecode, then retire the CEK machine.

- **For:** every feature keeps working throughout. The conformance and
  regression suites run unchanged at every step. There is no duplicated
  code.
- **Against:**
  - The codebase grows from 20k to perhaps 30k lines before the old
    evaluator can be deleted, with two evaluators sharing one heap, one set
    of builtins and one set of special forms. This is the "large and
    unmanageable" concern.
  - Every compiler decision is constrained by s1's existing representation:
    `GcRef` as a raw pointer, boxed integers, named local frames, the
    `Rc<Kont>` chain, and the runtime expansion caches. Fixnums alone touch
    every builtin (01 §4).
  - It is harder to learn from. The new design is always mixed with the old,
    so it is hard to see the compiler as a whole.
  - s1 loses its value as a stable reference, because the oracle changes
    along with the thing it checks.
- **Speed ceiling:** limited by the value representation unless that is also
  replaced, at which point this is a rewrite inside s1.

## B. A standalone executive and a standalone compiler

Two programs. A compiler reads Scheme and writes bytecode files, and an
executive (the VM and runtime) loads and runs them. They share the reader
and data structures. This is option 2 in the request.

- **For:**
  - The boundary is very clear: the bytecode format is the contract between
    the two halves. This is how many production systems are structured
    (Java and the JVM, Python's `.pyc`, Guile's `.go` files).
  - Each half is a manageable size.
  - The bytecode format must be specified exactly, which is educational.
- **Against:**
  - **R7RS needs the compiler at runtime.** `eval`, `environment`, the REPL
    and `load` all compile code while the program runs. So the executive has
    to contain the compiler anyway, and "standalone" applies only to batch
    compilation.
  - **Macros need the runtime at compile time.** A library's macros must be
    available to the code that imports it, so compiling one library needs
    the compiled form of the libraries it imports.
  - It needs a serialization format before anything runs: for constants,
    symbols, nested procedures and bignums. That is a lot of work before the
    first `(+ 1 2)`, against the incremental goal.
- **Speed ceiling:** the same as any VM. Startup is faster with precompiled
  files.

Decision U2 already places bytecode files later. This option is therefore
best treated as a **late phase of D** (P11 in 04): once the compiler and VM
work in one process, add a serializer and offer `s2c` (compile) and `s2`
(run) as separate binaries, built from the same crate.

## C. A Cargo workspace with a shared core crate

Move s1's reader, values, GC and builtins into a library crate,
`scheme-core`, used by both s1 and s2.

- **For:** no duplicated code. Fixes to the reader or builtins reach both
  systems.
- **Against:**
  - The shared crate freezes the value representation. s2 needs `Value` with
    fixnums (01 §4, 03 §3), and making that change in the shared crate means
    porting all of s1's evaluator to it, a large change to a system that
    works and is meant to stay a stable oracle.
  - The coupling points in 01 §3 (sys-builtin types, continuation data, GC
    roots naming `CEKState`) have to be made generic first. That is
    refactoring work that teaches nothing about compilers.
  - Changes to the core must keep two systems working, which slows down
    experimentation in the phase where s2's design is still moving.
- **Speed ceiling:** the same as D if the value representation changes;
  lower if it doesn't.

This only makes sense if both systems are going to be maintained long term.
s1 has done its job as an interpreter; its future role is as a reference.

## D. A new crate that forks and adapts parts of s1 (recommended)

A new crate, `s2`. Copy the s1 modules that carry over (01 §2, verdicts
"as-is" and "adapt") into it, and change them freely. Write the value
representation, compiler and VM new. s1 stays unchanged and is used as the
reference for differential testing.

- **For:**
  - It reuses the parts that are tedious and already correct: the reader
    with all its R7RS lexical details, number syntax, the printer's cycle
    handling, ports, about 4,200 lines of builtins, the syntax-rules matcher,
    and the library tables and features.
  - It writes new exactly the parts the project is meant to teach: the value
    representation, expander, compiler passes, instruction set, VM loop,
    closures, continuations.
  - s2 picks its own value representation from day one.
  - s1 stays a fixed oracle: any program can be run on both, and the outputs
    compared (05).
  - The size stays manageable. A plausible estimate for the finished system
    is 15-20k lines, of which perhaps 8k are new.
- **Against:**
  - Code is duplicated. A bug found in a forked module may need fixing in
    two places. That is acceptable with s1 frozen.
  - The forked builtins need a mechanical port to the new `Value` type.
    That is a few days of routine work, and it is spread across phases (P1
    needs a handful of builtins, P6 brings the rest).
- **Speed ceiling:** the highest of the options here, because nothing
  constrains the representation.

## E. A compiler written in Scheme, bootstrapped on s1

The classic route for Scheme systems (Chez, Gambit, Chicken, Guile's
compiler tower): write the compiler in Scheme, run it on s1 to compile
itself to bytecode, and from then on the system compiles itself.

- **For:**
  - It is very educational, and shows the bootstrapping process that real
    Schemes use.
  - Scheme is a pleasant language for writing compilers: pattern matching
    on s-expressions, quasiquote for generating code.
  - It exercises the system on a real, large program (the compiler itself).
- **Against:**
  - There are two moving targets at once: debugging the compiler means
    debugging Scheme code running on a VM that is also new.
  - Bootstrapping complicates the build. Changing the bytecode format means
    rebuilding the compiler with the old compiler.
  - The first runnable result comes later.
- **Speed ceiling:** the same at run time; compilation is slower until the
  VM is fast.

Decision U1 puts this as an optional late phase (P12). The plan keeps it
possible: the compiler passes work on plain s-expressions and a small core
AST, with no Rust-specific tricks, so they could be rewritten in Scheme
pass by pass.

## F. Alternatives to bytecode

The goal is speed through bytecode, but it is worth knowing what else could
be done and why bytecode is a good fit for this project.

| Technique | How it works | Speed vs a CEK interpreter | Why not (here) |
|---|---|---|---|
| **AST pre-analysis** (s1's technique D) | Convert forms to a tree of resolved nodes, and interpret the tree | 1.5-2x (s1's own estimate) | Still pays for tree walking and per-node dispatch. A good stepping stone, but bytecode goes further, and the core AST in 03 §2 already includes it |
| **Closure compilation** | Convert each node into a Rust closure (`Box<dyn Fn(&mut Vm) -> Value>`), and call the closures | 2-5x | Fast and simple, but proper tail calls and first-class continuations are awkward: closures recurse on the Rust stack, so you need trampolining and CPS. It also teaches little about instruction sets |
| **CPS conversion plus a trampoline** | Convert the program to continuation-passing style, so every call is a tail call; continuations become ordinary closures | Varies | Makes `call/cc` trivial but allocates a closure per non-tail call unless optimised heavily. A good topic for a later experiment (03 §10) |
| **Transpile to C or Rust** | Generate source and run the host compiler | 10-50x | Needs an external compiler at run time for `eval`; tail calls and continuations need tricks (Cheney on the MTA). Far from the goal |
| **Bytecode VM** (this plan) | Compile to a compact instruction sequence and interpret it in a tight loop | 3-10x | — |

Bytecode gives most of the available speed for an interpreter, keeps tail
calls and continuations simple (the VM controls its own stack), has a clear
boundary between compiler and runtime, and is the thing the project is meant
to teach.

## Comparison

| | A. extend s1 | B. separate binaries | C. shared crate | **D. fork** | E. self-hosted |
|---|---|---|---|---|---|
| Size and manageability | poor | good | fair | **good** | good |
| Learning value | fair | good | fair | **good** | very good |
| Risk | medium | high (format first) | medium | **low** | high |
| Speed ceiling | limited | high | depends | **high** | high |
| Reuse of s1 | total | partial | total | **selective** | front end only, via s1 |
| Time to first running code | short | long | long | **short** | long |

## Recommendation (decision D1)

**D: a new crate, `s2`, that forks the reusable parts of s1.**

- B becomes phase P11 (bytecode files and separate binaries, from the same
  crate).
- E becomes optional phase P12 (port the compiler to Scheme).
- A and C are rejected: A because of size and the representation
  constraint, C because of the representation constraint and the cost of
  keeping s1 working.

**What is forked, and when:**

| s1 source | Into s2 | Phase |
|---|---|---|
| `tokenizer.rs`, `parser.rs`, `number_syntax.rs`, `io.rs`, `gc/sstring.rs` | `reader/`, `io.rs`, `sstring.rs` | P0 |
| `printer.rs` | `printer.rs` | P0 |
| `env.rs` `BindingCell` and top-level table only | `globals.rs` | P1 |
| a handful of `builtin/number.rs` and `builtin/list.rs` functions | `builtins/` | P1 |
| collector design from `gc/heap.rs` | `heap.rs` | P3 |
| `syntax_rules.rs`, `eval/identifiers.rs` | `expand/` | P5 |
| portable parts of `scheme/s1-core.scm` | `scheme/core.scm` | P5-P7 |
| the rest of `builtin/*`, `ports.rs`, `eval/exceptions.rs` error objects | `builtins/` | P6-P7 |
| `libraries.rs` tables, import sets, search path, features | `libraries.rs` | P8 |

**Single crate, modules not crates.** One crate with a library target and one
binary is enough until P11. Splitting into several crates would add
boundaries that nothing needs yet.

Copied files should keep a header line naming their origin
(`// Forked from s1/src/parser.rs at bc6f118`), so differences can be traced.
