# 01. Review of s1

What s1 is, what s2 can take from it, and what it teaches. Paths are
relative to `../s1/`. Line numbers are from the commit at the top of s1's
log on 2026-10-10 (`bc6f118`) and will drift.

## 1. What s1 is

- About 20,000 lines of Rust in 35 files, plus `scheme/s1-core.scm` (522
  lines of Scheme compiled into the binary).
- A **CEK machine** (`src/eval/cek.rs`, `src/eval/kont.rs`). It interprets
  s-expressions directly: control (an expression or a value), environment (a
  chain of named frames) and continuation (an `Rc`-linked list of `Kont`
  frames). It never recurses on the Rust stack for Scheme calls, which gives
  proper tail calls and first-class continuations.
- It passes chibi's R7RS suite (1,154 tests) apart from complex numbers, and
  its own regression suite (about 1,657 tests including the
  `gc-threshold 1` reruns).
- It has been heavily optimised within the interpreter model. See
  `design/performance.md` and `design/precompilation-design.md`: inline
  evaluation of trivial operands, direct application from an argument stack,
  cached rewrites of binding forms, mimalloc, and smaller `SchemeValue` and
  `Kont` layouts. `precompilation-design.md` concludes that **only full
  pre-analysis ("technique D") can give another large speed-up**, and
  `kont-flat-stack-design.md` defers the continuation-stack redesign to "a
  future bytecode-VM redesign". In other words, s1 has reached the end of
  what an s-expression interpreter can do, and s2 is the next step.

## 2. Module inventory and reuse verdicts

Verdicts:
- **as-is**: copy the file and change little beyond the value type.
- **adapt**: the logic carries over; the interfaces change.
- **rewrite**: the purpose carries over; the code doesn't.
- **drop**: not needed in s2.

| s1 module | Lines | What it does | Depends on | Verdict for s2 |
|---|---|---|---|---|
| `src/tokenizer.rs` | 724 | Characters to tokens. Block comments `#\| \|#`, `#;`, datum labels `#n=`/`#n#`, `\|sym\|`, `#!fold-case` | `io` | **as-is** |
| `src/parser.rs` | 565 | Tokens to a datum on the heap: `parse(heap, port) -> Result<GcRef, ParseError>` (`:37`) | `gc`, `io` | **adapt**: builds `Value`s instead of `GcRef`s. Source locations could be added here (§5) |
| `src/number_syntax.rs` | 357 | Numeric literal syntax to `Int(BigInt) \| Rational \| Float`. Pure, no heap | `num-*` | **as-is** |
| `src/printer.rs` | 596 | `write`/`display`, with cycle and sharing detection | `gc` | **adapt**: new value type |
| `src/io.rs` | 299 | `PortKind`, `FileTable`, string ports | nothing | **as-is** |
| `src/gc/sstring.rs` | 234 | UTF-8 strings with an all-ASCII flag for O(1) indexing | nothing | **as-is** |
| `src/gc/mod.rs`, `objects.rs`, `heap.rs` | 1,789 | `GcRef`, `SchemeValue`, `Callable`, allocation, mark-sweep, the symbol table, the alias and expansion tables | names `eval` types (§3) | **adapt**: the collector's design carries over. The value representation is rewritten (03 §3) |
| `src/env.rs` | 669 | Named frames (`Rc<RefCell<Frame>>`). Top-level `BindingCell`s shared by imports | `gc` | **adapt**: keep `BindingCell` and the top-level table. Local frames disappear (locals become stack slots and closure fields) |
| `src/builtin/*` | ~4,200 | Pure builtins: `fn(&mut GcHeap, &[GcRef]) -> Result<GcRef, String>` | `gc`, `printer` | **adapt**: mechanical port to `Value`. Numbers gain a fixnum fast path |
| `src/syntax_rules.rs` | 987 | `syntax-rules` parsing, matching, instantiation | `eval::identifiers`, `EnvRef` | **adapt**: matcher and instantiator carry over. Identifier comparison goes through a compile-time environment interface |
| `src/eval/identifiers.rs` | 365 | Alias resolution, `free_identifier_eq`, `strip` | `env`, heap alias table | **adapt** into the expander's identifier module |
| `src/libraries.rs` | 993 | Standard library export tables, import sets, `define-library`, search path, `cond-expand`, `features` | `env`, `eval` | **adapt**: tables, import-set parsing, search path and features carry over. `define-library` processing moves into the compiler |
| `src/special_forms.rs` | 1,414 | Core syntax and Rust rewrites of derived forms | the CEK machine | **rewrite**: each becomes an expander or compiler case. The rewrites document the exact R7RS semantics to reproduce |
| `src/eval/*` (cek, kont, mod) | 3,475 | The CEK machine | everything | **rewrite**: replaced by the VM |
| `src/sys_builtins.rs` | 968 | `apply`, `call/cc`, `dynamic-wind`, `eval`, `call-with-values`, ... | the CEK machine | **rewrite**: these become VM instructions or VM-aware builtins |
| `src/eval/exceptions.rs` | 317 | `raise`, `with-exception-handler`, `error`, error objects | the CEK machine | **adapt**: error objects carry over, raising is rewritten |
| `src/ports.rs` | 798 | The port procedures, as sys-builtins | runtime ports, `raise` | **adapt**: mechanical. Most only need the current ports and a way to raise |
| `src/debugger.rs` | 533 | Tracer, stepper, `debug>` prompt | the CEK machine | **drop** (decision U3) |
| `src/utilities.rs` | 290 | Error posting, debug dumps | `eval` | **drop**, apart from small helpers |
| `src/main.rs` | 298 | CLI, startup, REPL | everything | **rewrite**, small |
| `scheme/s1-core.scm` | 522 | Scheme-level library: `map`, `for-each`, derived syntax as `syntax-rules`, `make-parameter`, records, promises | s1's evaluator | **adapt**: most of it is portable Scheme. Drop s1's extras (`push!`, `def`, trace helpers) |

Roughly: **4,000 lines carry over nearly unchanged** (reader, number syntax,
io, strings), **8,000 lines carry over with a mechanical port** (builtins,
printer, ports, syntax-rules, library tables), and **8,000 lines are
replaced** by the compiler and VM. The replaced part is exactly the part this
project is meant to teach.

## 3. Coupling points

s1's data layer is mostly independent of the evaluator. The exceptions, all
in `src/gc`:

1. `Callable::SysBuiltin` and `Callable::SpecialForm` name `RunTime`,
   `CEKState` and `KontRef` in their function types (`gc/mod.rs:152,158`).
2. `ContinuationData.kont` is a `KontRef` (`gc/mod.rs:314`), and
   `DynamicWind` lives in `eval`.
3. `GcHeap::collect_garbage` takes `&CEKState` as a root (`heap.rs:475`), and
   marking calls `KontRef::mark` (`heap.rs:712`).
4. `Callable::SyntaxRules` pulls in `syntax_rules.rs`, which uses
   `eval::identifiers`.

Builtins return `Err(String)`, which `utilities::post_error` turns into a
raised error object. That convention is easy to keep in a VM.

## 4. The value representation, s2's biggest constraint

- `GcRef = *mut GcObject` (`gc/mod.rs:122`), a raw pointer. Every object is
  its own `Box`, and `GcHeap.objects: Vec<GcRef>` lists them all. Objects
  never move.
- **There are no fixnums.** Every integer is a heap-allocated `BigInt`
  (`objects.rs:275`), and so is every character and float. Only `'()`, `#t`,
  `#f`, void, eof and undefined are shared singletons. Because equal numbers
  can be different objects, `eq?` compares number values.
- `SchemeValue` is pinned at ≤ 32 bytes and `GcObject` at 40 by `const`
  assertions.
- `heap.get_value_mut(&self)` hands out `&mut` from `&self`
  (`heap.rs:237`). This is unsound aliasing under Rust's rules, and it is
  used everywhere.

For an interpreter whose cost is dominated by continuation frames and
lookups, boxed integers cost little: s1 measured interning small integers at
about 1% on `fib`. **For a bytecode VM they become the dominant cost.** Once
dispatch is cheap, allocating on every `(+ n 1)` is the largest remaining
overhead. This is the main reason the recommendation in 02 is to fork and
use a new `Value` type, rather than share s1's data crate.

## 5. Front-end facts that shape the compiler

- **There is no expansion pass.** A `syntax-rules` form evaluates to a
  transformer value bound in the runtime environment, and a macro use is
  expanded when the CEK machine reaches it (`cek.rs:340`), with the result
  cached by use form (`heap.rs:411`). A compiler has to expand completely
  before it generates code, which needs a **compile-time syntactic
  environment** that s1 doesn't have.
- **Hygiene is renaming to aliases** (`design/hygiene-design.md`). An alias
  is a fresh, uninterned symbol with the same name, recorded in the heap's
  ephemeron table as `alias -> {original, env}`. Lookup tries the plain name
  first, then follows the alias into the macro's definition environment
  (`identifiers.rs:30`). This design carries over well to compile time: the
  "env" becomes a compile-time scope, and resolution happens once, during
  expansion, rather than on every evaluation.
- **Derived forms:** some are Rust rewrites (`let` and named `let`, `let*`,
  `letrec`, `do`, `guard`, `quasiquote`, internal defines to `letrec`). Some
  are `syntax-rules` in `s1-core.scm` (`when`, `unless`, `case`, `letrec*`,
  `let-values`, `let*-values`, `define-values`, `parameterize`,
  `define-record-type`, `delay`, `delay-force`). s2 can move nearly all of
  them into syntax-rules, or into a few Rust expander cases where error
  messages or speed justify it.
- **There are no source locations.** Tokens and data carry no positions.
  Error messages with line numbers need a side table from pairs to
  positions, filled by the reader.

## 6. Lessons s1 learned the hard way

These come from s1's design notes and commit history. s2 should start out
following them rather than rediscover them.

1. **A Rust local is not a GC root.** `design/nested-evaluation.md` records
   five places where s1 held `GcRef`s in Rust locals across a nested
   evaluation. The results were segfaults and silent corruption. s1's fix was
   a rule: *allocation never collects*. Collection happens only at a few
   safepoints, where every live value is reachable from the machine state.
   s2's VM must have the same property: collect only at instruction
   boundaries chosen for it, with all live values on the VM stack or in
   registered roots.
2. **"Evaluate this, then resume" is a frame, never a nested call to the
   evaluator.** In the VM this means a builtin that needs to call Scheme code
   (`apply`, `dynamic-wind`, the handler for `raise`) has to set up a VM
   frame and return to the dispatch loop. It must not call `vm.run()`
   recursively, or both continuations and GC break.
3. **Stress the GC from the start.** s1 found its rooting bugs only once
   `gc-threshold 1` and poison-sweep (`S1_GC_POISON`) testing existed. s2
   should have both from the phase that introduces the GC (P3).
4. **Top-level bindings need an identity apart from their name and value:**
   a cell (`env.rs:19-37`). Imports share cells, `define` and `set!` change a
   cell's contents, and compiled code can hold a cell directly. s1 built this
   ahead of its library phase precisely so that compiled code could use it.
5. **Measure before building.** s1's notes repeatedly found that sampled
   profiles misattributed cost (the "61% malloc" figure that turned out to be
   30%), and that duplicating a piece of work and timing the difference was a
   more reliable measurement. Interleaved A/B runs, using medians, were
   needed on a laptop CPU with boost.
6. **Escape-only versus re-entrant continuations matter for speed.** s1 has
   both: `call/cc`, which captures by sharing the frame chain and snapshots
   the argument stack, and `%call/ec`, which is escape-only and used by
   `guard`. On a contiguous stack, the escape-only form is far cheaper, so s2
   should keep the distinction (03 §10).

## 7. Tests and benchmarks that carry over

- **The chibi R7RS suite** (`tests/r7rs/r7rs-tests.scm`, 2,523 lines,
  unmodified) is portable. The runner `run.sh` splits it into 20 sections and
  compares counts with `baseline.txt`. The shim `shim.scm` stands in for
  `(chibi test)`, but it is written with s1's procedural `macro`, so s2 needs
  a `syntax-rules` version (P9).
- **The s1 regression suite** (`scheme/regression.scm` loads 14 files) leans
  on s1 extensions: `gc-threshold`, `macro`, `help`, `trace-procedure`,
  `type-of`, `expand`, `push!`, `procedure-source`. The harness
  (`scheme/test-harness.scm`, 85 lines) is portable. `call_cc_tests.scm` and
  `tail_tests.scm` are close to portable, and much of the basic, advanced,
  derived, data and exception tests can be ported by deleting extension
  uses. 05 has the details.
- **Benchmarks:** `bench/fib.scm`, `bench/program.scm` (the "typical
  program": records, closures, sorting, string ports), `bench/macro-loop.scm`
  and `bench/bench.sh`. They are portable R7RS and become s2's benchmark set,
  with s1 as the baseline.
- **Rust integration tests** (`tests/process.rs`, `tests/debugger.rs`) test
  s1's CLI. `process.rs` (exit status, `command-line`, shebang scripts) is a
  model for s2's equivalent.
