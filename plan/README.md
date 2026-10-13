# s2: a bytecode-compiled R7RS-small Scheme

Planning documents, written 2026-10-10, before any code exists. They follow
the practice s1 already uses (`../s1/design/*.md`): work out the design on
paper, measure where possible, then implement in small steps.

For using s2, see the [user guide](https://timbarnes.github.io/s2/) (source
in `docs/`).

**Status:** P0-P7 done 2026-10-10, P8-P12 2026-10-11 ([P0](phases/P0.md), [P1](phases/P1.md), [P2](phases/P2.md), [P3](phases/P3.md), [P4](phases/P4.md), [P5](phases/P5.md), [P6](phases/P6.md), [P7](phases/P7.md), [P8](phases/P8.md), [P9](phases/P9.md), [P10](phases/P10.md), [P11](phases/P11.md), [P12](phases/P12.md)). P13, planned in [08](08-compiler-in-scheme.md), is in progress: [P13.0-P13.4](phases/P13.md) done 2026-10-12 (P13.2 folded into P13.1).

## Goals

1. **A fully R7RS-small system** at the level s1 already reaches: it passes
   chibi's R7RS suite, with complex numbers left out unless that decision is
   revisited.
2. **Fast, by compiling to bytecode.** As a target to check against, roughly
   5-10x faster than s1 on call-heavy code (`fib`, the "typical program") by
   the end of the performance phase. Each phase that should change speed has
   a checkpoint against s1.
3. **Learning.** Understand how a Scheme compiler and VM are built and wired
   together. Each phase should be small enough to understand on its own,
   and each phase records the decisions it makes and why.
4. **Incremental.** Every phase ends with a working system and passing
   tests. No phase depends on a later one to make sense.

## Non-goals (for now)

- s1's extras: the debugger and tracer, the procedural `macro` form, `help`
  and docstrings. The core stays "as pure as possible". A disassembler and a
  simple instruction trace are part of the plan anyway, as learning tools
  (see 03, §15).
- Complex numbers. P9 keeps a slot open for them if this changes.
- A native-code compiler or JIT.
- Changes to s1. It stays frozen as the reference implementation and test
  oracle.

## How to read these documents

| File | What it answers |
|---|---|
| [01-s1-review.md](01-s1-review.md) | What s1 contains, what can be reused, how tightly it is coupled, and the lessons it teaches |
| [02-approaches.md](02-approaches.md) | Project-level strategies: extend s1, separate executive and compiler, shared crate, fork, self-hosting, and non-bytecode alternatives |
| [03-architecture.md](03-architecture.md) | The compiler pipeline and each core design decision: value representation, GC, VM model, closures, continuations, macros, libraries |
| [04-roadmap.md](04-roadmap.md) | The incremental phases, each with goals, what it teaches, decisions, exit tests and benchmarks |
| [05-testing.md](05-testing.md) | How each phase is verified: differential testing against s1, golden disassembly, GC stress, conformance, benchmarks |
| [06-directions.md](06-directions.md) | Written after P10: the compiler in Scheme here or in a fork, a reduced runtime, comparing with other Schemes, embedded targets, native code |
| [07-comparison.md](07-comparison.md) | P12: s2 against eight other Schemes and s1, in architecture, completeness, features and performance |
| [08-compiler-in-scheme.md](08-compiler-in-scheme.md) | The plan for P13: the decisions to settle, sub-phases P13.0-P13.6 and their exit tests, and the risks |

Read them in order the first time. During implementation, 04 is the working
document, and 03 is the reference for why things are shaped as they are.

## Decisions

Already made by the user:

| # | Decision | Choice |
|---|---|---|
| U1 | Language of the compiler | Rust first; porting it to Scheme is a late phase (P13) |
| U2 | Compilation mode | In-process first; bytecode files later (P11) |
| U3 | s1 extras | None at first. The core stays pure, and s1 stays available for testing code |
| U4 | How much of s1 to reuse | D1 accepted 2026-10-10: fork and adapt into a new crate |
| U5 | Speed target | 5-10x faster than s1 on `fib` and the typical program; accepted for now |
| U6 | Complex numbers | Out. Confirmed at P9 |
| U7 | Where the compiler in Scheme is built | In this repository, tagged first; not a fork (06 §1) |
| U8 | Phases after P11 | P12 comparison with other Schemes, P13 the compiler in Scheme |
| U9 | Embedded targets (ESP32) | Not planned (06 §4) |
| U10 | Native code | Not planned for now (06 §5) |
| C1-C11 | The compiler in Scheme | Settled before P13 (08): environments stay in Rust, the Scheme compiler must produce the Rust compiler's code exactly, back end first |

Recommended in these documents, each to be confirmed:

| # | Decision | Recommendation | Where |
|---|---|---|---|
| D1 | Project strategy and reuse | **Accepted.** A new crate. Fork the reader, printer, number syntax, io, the syntax-rules matcher and builtin logic from s1, and adapt them. Write a new value representation, heap interface, compiler and VM | 02 |
| D2 | Value representation | An opaque `Value` type with unboxed fixnums, chars and immediates. Start as a 16-byte `Copy` enum, keep it behind methods, and move to a tagged word later if measurement says so. **After P12:** NaN-boxed, one 64-bit word: flonums stay unboxed, fixnums are 48-bit. 1.11-1.41x faster on `bench/`, 1.24x (geometric mean) on the 56 r7rs benchmarks in 07's rerun, none slower; only `value.rs` changed | 03 §3 |
| D3 | Garbage collector | s1's model: non-moving mark-sweep, epoch marks, collection only at safepoints, explicit roots | 03 §4 |
| D4 | VM model | Stack-based, with frames on a single contiguous value stack | 03 §5 |
| D5 | Instruction encoding | A Rust `enum Op` ("wordcode") first, a byte encoding when bytecode files arrive. **P11:** files encode each instruction as a tag byte and LEB128 operands, decoded into `Op`s on load | 03 §6 |
| D6 | Closures and variables | Flat closures, and assignment conversion: mutated captured variables live in boxes | 03 §7 |
| D7 | Globals | Binding cells, as in s1, referenced directly from the constant pool | 03 §8 |
| D8 | Continuations | Full copy of the stack on `call/cc`, and copy back on invoke. A segmented stack is an upgrade in P10 if benchmarks need it | 03 §10 |
| D9 | Primitives | Builtins called through the normal call path at first. Inline opcodes for hot primitives in P10, only where R7RS makes the binding immutable | 03 §11 |
| D10 | Macro expander | A separate expansion pass with a compile-time syntactic environment. Hygiene by renaming, adapted from s1's alias scheme. Reuse s1's syntax-rules matcher | 03 §12 |
| D11 | Intermediate form | A small core AST that every derived form expands into, with analysis passes over it before code generation | 03 §2 |

## Reading list

The sources below are the ones that matter most for this design. Each phase
in 04 names the ones relevant to it.

- R. Kent Dybvig, *Three Implementation Models for Scheme* (PhD thesis,
  1987). The heap, stack and string models. Its stack model (flat closures,
  boxes for assigned variables, continuations by copying the stack) is very
  close to what this plan recommends.
- William Clinger, Anne Hartheimer, Eric Ost, *Implementation Strategies for
  First-Class Continuations* (1999). Compares the stack, heap, segmented and
  incremental strategies. The background for D8.
- Abdulaziz Ghuloum, *An Incremental Approach to Compiler Construction*
  (2006). Builds a Scheme compiler in many small working steps. This is the
  model for the roadmap's structure, though it targets x86 rather than a VM.
- Robert Nystrom, *Crafting Interpreters*, part III (clox). A clear
  walkthrough of a stack VM, closures with upvalues, and a mark-sweep GC.
- Roberto Ierusalimschy, Luiz Henrique de Figueiredo, Waldemar Celes, *The
  Implementation of Lua 5.0* (2005). The case for a register VM, for
  comparison with D4.
- Oscar Waddell, Dipanwita Sarkar, R. Kent Dybvig, *Fixing Letrec* (2005).
  How to compile `letrec`/`letrec*` and internal definitions efficiently.
- William Clinger, Jonathan Rees, *Macros That Work* (1991), and the
  explicit-renaming / syntactic-closure literature. The background for s1's
  alias hygiene and for D10.
- The Guile VM documentation (Guile 3's instruction set and compiler tower)
  and Chibi Scheme's VM source. Real bytecode Schemes to compare decisions
  against.
- *Revised⁷ Report on the Algorithmic Language Scheme*, especially §5
  (program structure and libraries), §6.10 (control) and §7.3 (derived
  forms as macros).
