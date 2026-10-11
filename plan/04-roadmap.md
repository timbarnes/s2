# 04. Roadmap

The implementation, in phases. Each phase:

- **ends with a working system** and all tests passing, including every
  earlier phase's tests;
- is **small enough to understand on its own**: it introduces one main
  idea, and maybe a couple of supporting ones;
- **settles specific decisions** from 03, listed under "Decides", so the
  architecture grows from choices that were made deliberately, at the point
  they were needed;
- ends with **a short phase note**, `plan/phases/Pn.md`. It records what was
  built, the decisions made and why, what surprised us, and the measurements.
  These notes are the record of how the architecture came about.

Rough sizes are new or changed lines of Rust, excluding forked code, to give
a sense of scale.

```
P0 skeleton ─► P1 first compiler+VM ─► P2 lambda/closures/tail calls ─► P3 GC
   ─► P4 set!/letrec ─► P5 expander ─► P6 data types+builtins ─► P7 control
   ─► P8 libraries/eval ─► P9 conformance ─► P10 performance ─► P11 bytecode files
   ─► P12 comparison with other Schemes ─► P13 compiler in Scheme
```

P0-P4 build the core compiler and VM on a tiny language. P5-P9 grow it to
full R7RS. P10-P11 make it fast and complete the picture. P12-P13 were added
after P10 (06): see how s2 compares, then make it self-hosting.

---

## P0. Skeleton: read and print

**Goal:** `s2` starts a REPL that reads a datum and prints it back.

**Teaches:** the value representation, and the boundary between the reader
and the rest of the system.

**Build:**
- the Cargo project and the module layout from 03 §16;
- `value.rs`: the `Value` enum and its accessor methods (D2);
- `heap.rs`: allocation, pairs, symbols (interned), strings, vectors, and a
  bignum object. No collection yet; everything is leaked;
- fork `tokenizer.rs`, `parser.rs`, `number_syntax.rs`, `io.rs`,
  `sstring.rs` and `printer.rs` from s1, and adapt them to `Value`;
- `main.rs`: the read-print loop, and `s2 -e '<datum>'` for tests.

**Decides:** D1 (confirmed by doing it), D2 (the enum, the fixnum range,
whether floats are unboxed), the object layout with `Cell`s (03 §3).

**Exit tests:** s1's reader unit tests, forked along with the reader, all
pass. Round-trip tests: `read` then `write` gives the same text for a corpus
covering every lexical form, including datum labels and block comments.

**Size:** about 600 new lines, plus about 2,800 forked.

**Reading:** *Crafting Interpreters*, ch. 14-15 (chunks, the VM), for the
shape of what comes next.

---

## P1. The first compiler and VM

**Goal:** `(define x 10)`, `(if (< x 20) (+ x 1) 0)` and `(* 6 7)` compile
to bytecode and run.

**Teaches:** the whole pipeline end to end, in the simplest form that can
work: constants, the stack discipline, globals, jumps, calling a builtin.

**Build:**
- `compile/ast.rs`: the core AST (D11), with only `Const`, `GlobalRef`,
  `GlobalDefine`, `If`, `Seq` and `Call` for now;
- a minimal hard-coded "expander" that recognises `quote`, `if`, `define`
  and `begin`, and treats everything else as a call. The real expander is
  P5;
- `compile/codegen.rs`: the AST to `Vec<Op>`, with jump patching for `if`;
- `vm/op.rs` and `vm/template.rs`: the instruction enum (D5) and
  templates (code, constants, arity, name);
- `vm/machine.rs`: the dispatch loop, value stack, `Call` to primitives,
  `Return` from the top-level thunk;
- `globals.rs`: binding cells (D7) and one top-level environment;
- builtins: `+ - * < = >`, `car`, `cdr`, `cons`, `display`, `newline`,
  with fixnums only at first;
- `compile/disasm.rs` and the `--dis` flag.

**Decides:** D4 (stack VM), D5 (`enum Op`), D7 (cells in the constant
pool); how the top-level form becomes a thunk; the builtin signature and how
errors from builtins are reported (no `raise` yet: an error aborts the form
and prints).

**Exit tests:**
- golden disassembly tests: the source of a form and its expected
  disassembly (05);
- result tests for arithmetic, `if`, `define` and nested calls;
- an unbound global gives an error, and the REPL continues.

**Size:** about 1,200 lines.

**Reading:** *Crafting Interpreters*, ch. 15, 17, 21, 23; Ghuloum, §2.1-2.4.

---

## P2. `lambda`, local variables, closures and tail calls

**Goal:** `fib` runs, and a million-iteration loop written with a tail call
runs in constant space.

**Teaches:** frames and calling conventions, how local variables become
stack slots, free-variable analysis, flat closures, and how proper tail
calls are implemented.

**Build:**
- AST: `Lambda`, `LocalRef`, `Let`; `VarId`s for locals, assigned by the
  minimal expander, which now handles `lambda` and `let` with lexical scope;
- `compile/analyze.rs`: free variables of each lambda, and tail positions;
- instructions `LocalRef`, `FreeRef`, `MakeClosure`, `Call` to closures,
  `TailCall`, `Return`, and the frame stack (03 §5, §9);
- arity checks and rest arguments;
- `(define (f x) …)` shorthand.

**Decides:** D6 (flat closures; boxing comes in P4), the frame layout, the
calling convention, and how `TailCall` moves arguments.

**Exit tests:**
- closures capture correctly: counters, currying, `compose`;
- `(define (loop n) (if (= n 0) 'done (loop (- n 1))))` with `(loop 1000000)`
  doesn't grow the stack (the VM's maximum stack depth is checked in a test);
- wrong argument counts give errors;
- s1's `tail_tests.scm` cases that use only these forms.

**Benchmark checkpoint:** `fib 25` and `fib 30` against s1. This is the
first real measurement of what compilation buys, even with no GC and no
inlining. Record it in the phase note.

**Size:** about 800 lines.

**Reading:** Dybvig's thesis, ch. 4 (the stack-based model), up to
assignment.

---

## P3. Garbage collection

**Goal:** long-running programs run in bounded memory, and the GC is proven
safe under stress.

**Teaches:** root sets, safepoints, and why a VM's GC is easier to get right
than an interpreter's, if the VM is designed for it.

This comes this early deliberately: s1's hardest bugs were GC rooting bugs
found late (01 §6). Every later phase is then developed with the collector
running.

**Build:**
- mark-sweep in `heap.rs`, adapted from s1's `gc/heap.rs`: epoch marks, the
  worklist, scheduling in proportion to the live heap (D3);
- the root set: value stack, frame stack, globals, symbol table, VM
  registers (03 §4);
- safepoints at `Call`/`TailCall` and on return from a builtin;
- a stress mode (collect at every safepoint) and a poison-sweep mode.

**Decides:** D3; the exact list of safepoints; whether the symbol table is
a root or weak (simplest: a root).

**Exit tests:**
- the whole test suite passes in stress mode and in poison mode;
- a loop that allocates a list every iteration runs a million times in flat
  memory;
- deep non-tail recursion (100,000 deep) completes, and collection time
  stays linear in it.

**Size:** about 500 lines.

**Reading:** *Crafting Interpreters*, ch. 26; s1's
`design/nested-evaluation.md` and `design/gc-tail-loop.md`.

---

## P4. Assignment, `letrec` and internal definitions

**Goal:** `set!` on local and captured variables works, and so do internal
`define`s and mutually recursive local procedures.

**Teaches:** assignment conversion (why captured mutable variables need
boxes) and the semantics of `letrec*`.

**Build:**
- AST: `LocalSet`, `GlobalSet`, `Letrec`;
- analysis: assigned variables, captured variables, and which variables
  need boxes (03 §7);
- instructions `MakeBox`, `BoxRef`, `BoxSet`, `LocalSet`, `GlobalSet`;
- `letrec*` as "bind to undefined, assign in order", with an error for a
  reference before initialisation;
- body handling in the minimal expander: internal `define`s collected into
  a `Letrec`.

**Decides:** D6 completed; how internal definitions are collected (the
real body rules come with P5's expander).

**Exit tests:**
- a counter closure (`make-counter`) with `set!` on a captured variable;
- two closures sharing one mutable variable;
- `even?`/`odd?` as internal definitions;
- golden disassembly showing boxing only where needed.

**Size:** about 500 lines.

**Reading:** Dybvig's thesis, ch. 4 (assignment and boxes); Waddell,
Sarkar and Dybvig, *Fixing Letrec*, §1-3.

---

## P5. The macro expander

**Goal:** the real expander replaces the minimal one. `syntax-rules` works
hygienically, and the derived forms are defined as macros.

**Teaches:** syntactic environments, hygiene by renaming, R7RS body rules,
and the split between core syntax and derived syntax.

**Build:**
- `expand/`: the syntactic environment and `Binding` (03 §12), with
  identifiers and aliases forked from s1's `eval/identifiers.rs`, and
  `syntax_rules.rs` forked from s1;
- core forms as expander cases: `quote`, `lambda`, `define`, `set!`, `if`,
  `begin`, `let`, named `let`, `define-syntax`, `let-syntax`,
  `letrec-syntax`, `syntax-rules`, `quasiquote` (from s1's lowering);
- body expansion: expand heads to find definitions, splice `begin`, handle
  internal `define-syntax`;
- `scheme/core.scm`, started: `cond`, `case`, `and`, `or`, `when`,
  `unless`, `let*`, `letrec`, `letrec*`, `do`, `let-values`,
  `let*-values`, `define-values`, as `syntax-rules` (many from s1's
  `s1-core.scm`, others from R7RS §7.3);
- the `--expand` flag, which prints the core AST.

**Decides:** D10; which forms are Rust cases and which are macros; how
`core.scm` is loaded at startup (compiled in, like s1's `include_str!`).

**Exit tests:**
- s1's `syntax_rules_tests.scm` and the hygiene tests from
  `macro_tests.scm` that don't use `macro` (05);
- the classic hygiene cases: `my-or` with a user variable named `t`, a
  user rebinding `if` locally around a macro that uses `if`;
- R7RS §7.3 derived forms checked against s1 by differential tests.

**Size:** about 1,500 lines, plus about 1,300 forked.

**Reading:** Clinger and Rees, *Macros That Work*; s1's
`design/hygiene-design.md`; R7RS §4.3 and §7.3.

---

## P6. Data types and builtins

**Goal:** all of R7RS's data types and their procedures, except the
control features.

**Teaches:** mostly porting rather than new ideas. One real design topic:
the numeric tower with a fixnum fast path, and normalising bignums back to
fixnums.

**Build:**
- port the rest of s1's `builtin/*` to `Value`: numbers (fixnum, bignum,
  rational, flonum, with exactness rules), characters, strings, symbols,
  lists, vectors, bytevectors, `eq?`/`eqv?`/`equal?`;
- records: `define-record-type` as a macro over record primitives (as in
  s1);
- ports and I/O: port objects, string ports, file ports, `read`,
  `write`/`display` and their relatives, and `current-*-port`;
- `scheme/core.scm`: `map`, `for-each`, `vector-map`, `string-for-each`,
  `member`, `assoc`, and so on (from s1);
- the errors builtins report become proper error objects; raising comes in
  P7.

**Decides:** fixnum overflow and normalisation rules; how ports are
represented; the list of builtins written in Rust versus Scheme.

**Exit tests:** the chibi suite's sections for numbers, booleans, lists,
symbols, characters, strings, vectors, bytevectors and I/O, run with a
temporary simple shim (the full syntax-rules shim is P9). Also s1's
`data_tests.scm` and `port_tests.scm`, ported.

**Size:** about 1,000 new lines, plus about 5,000 forked.

---

## P7. Continuations and dynamic control

**Goal:** `call/cc` (re-entrant), `dynamic-wind`, `values`,
`call-with-values`, exceptions, `guard`, `parameterize`, `delay` and
`force`.

**Teaches:** what a continuation is, concretely: a copy of the two stacks.
Also how much of Scheme's control can be built in Scheme from one VM
primitive.

**Build:**
- the `Continuation` object; `%call/cc` (copy the stacks) and resuming
  (copy them back), D8; `call/ec`, escape-only;
- `VmPrimitive` and `Action` (03 §11) for `apply`, `%call/cc`, `values`,
  `call-with-values` and `eval`;
- in `scheme/core.scm`: `dynamic-wind` with a wind list; the user-visible
  `call/cc`, which runs the wind thunks; `with-exception-handler`,
  `raise`, `raise-continuable`, `error`, `guard`; `make-parameter`,
  `parameterize`; `delay`, `delay-force`, `make-promise`, `force`;
- builtin errors become calls to `raise`, so `guard` catches them;
- uncaught errors: message, irritants and a backtrace from the frame stack.

**Decides:** D8 (the copying strategy, and whether `call/ec` is enough for
`guard`); D9's VM-primitive design; the multiple-values representation;
what is in Rust versus Scheme for the control features.

**Exit tests:**
- s1's `call_cc_tests.scm` (re-entry, generators, the re-entrant
  `dynamic-wind` order test in `advanced_tests.scm`) and
  `exception_tests.scm`, ported;
- the chibi sections for control and exceptions;
- `guard` nested 8,000 deep runs in linear time (s1 once had a cubic bug
  here);
- all of the above under GC stress.

**Size:** about 1,000 lines, half of them Scheme.

**Reading:** Clinger, Hartheimer and Ost; Dybvig's thesis, ch. 4
(continuations in the stack model); R7RS §6.10-6.11.

---

## P8. Libraries, `eval` and programs

**Goal:** `define-library`, `import`, `.sld` files on a search path,
`cond-expand`, `include`, `eval` with environments, and R7RS programs.

**Teaches:** separate top levels, compile-time versus run-time environments,
and why a compiler for Scheme has to live inside the runtime.

**Build:**
- top-level environments with both cells and macros (03 §13); the standard
  library tables forked from s1;
- `import` with `only`, `except`, `prefix` and `rename`, forked;
- `define-library`, with exports of variables *and* macros;
- the library search path, `include`, `include-ci`,
  `include-library-declarations`, `cond-expand`, `features` (forked);
- `eval`, `environment`, `interaction-environment`,
  `scheme-report-environment`, `null-environment`;
- programs: `s2 file.scm args`, `command-line`, `exit`,
  `emergency-exit`; the rest of the system interface from s1's phase 10.

**Decides:** how macros are exported and imported; when a library is
compiled (on first import, in the same process); the REPL's redefinition
rules for imported names (s1's: a fresh cell).

**Exit tests:** s1's `library_tests.scm` (ported) with its `.sld` fixtures;
the chibi sections for environments and `eval`; and process tests like s1's
`tests/process.rs` (exit status, `command-line`, shebang scripts).

**Size:** about 1,000 new lines, plus about 700 forked.

**Reading:** R7RS §5; s1's `design/libraries-design.md`.

---

## P9. Conformance

**Goal:** the full chibi R7RS suite passes, at the level s1 reaches: 1,154
tests, everything except complex numbers.

**Teaches:** where the corners of the standard are. This phase mostly
finds and fixes bugs.

**Build:**
- a `syntax-rules` version of `tests/r7rs/shim.scm` (s1's uses `macro`);
- `run.sh` and the baseline mechanism, forked;
- fixes for whatever fails;
- the remaining audit items from s1's phase 11: tail calls in every R7RS
  tail context, auxiliary syntax bindings (`else`, `=>`, `_`, `...`),
  `syntax-error`;
- optionally: source locations in error messages (03 §14);
- if decided: complex numbers.

**Decides:** whether complex numbers come in; whether source locations
are worth it now.

**Exit tests:** the chibi suite baseline matches s1's, and s1's portable
regression files pass (05).

---

## P10. Performance

**Goal:** make s2 fast, by measurement. Each item is tried, measured on the
benchmark set, and kept only if it pays.

**Teaches:** where a bytecode VM actually spends its time, and which
classic optimisations matter in practice.

Candidate items, in a likely order of value:

1. **Inlined primitives** (03 §11): `car`, `cdr`, `cons`, `null?`,
   `pair?`, `eq?`, fixnum `+ - < = >` with a fallback. Decides D9's REPL
   question.
2. **`letrec` of lambdas without boxes** (*Fixing Letrec*, 03 §7).
3. **Superinstructions:** fused sequences chosen from instruction-count
   statistics, such as `LocalRef`+`Fixnum`+`Sub`, or `JumpIfNotLt`.
4. **Calls to known local procedures:** direct jumps for named-`let` loops
   with no closure created.
5. **Multiple values on the stack** instead of a `Values` object.
6. **The tagged-word `Value`** (option b in 03 §3), if memory bandwidth
   shows up in profiles.
7. **A segmented stack** for continuations, if a generator benchmark shows
   that copying hurts (D8).
8. **Constant folding** and other simple AST optimisations, perhaps with an
   ANF intermediate form.

**Exit tests:** every item keeps all tests green, including GC stress. Each
kept item records its before and after numbers in the phase note,
measured as s1 did: interleaved runs and medians.

**Benchmark target:** 5-10x faster than s1 on `fib` and the "typical
program" (README goal 2).

---

## P11. Bytecode files and separate binaries

**Goal:** compile a program or library to a file, and load it without
recompiling. Offer `s2c` (compile) and `s2` (run) as separate binaries if
that is still wanted. This is option B from 02.

**Teaches:** serialisation: what a compiled procedure depends on, and how
to write and read constants, symbols, nested templates, cell references and
bignums.

**Build:** a byte encoding of `Op`s and a file format with a header,
version, constant pools, templates and the imports needed. Libraries are
cached as compiled files next to their `.sld`, with a staleness check.

**Decides:** D5's second half (the byte encoding); whether the VM runs from
bytes or decodes to `Op` on load (measure it); how macros exported by a
compiled library are stored (as `syntax-rules` source, re-parsed on load).

The format should also serve P13's boot image (06): a whole set of
libraries in one file, loadable before any Scheme has run. `core.scm` is
the first user: shipped compiled, so startup doesn't compile it.

---

## P12. How s2 compares with other Schemes

**Goal:** see where s2 stands against established Schemes in completeness,
features, architecture and performance, and get a performance baseline
for P13 (06 §3).

**Teaches:** how other systems answered the questions in 03, and what
their answers buy them.

**Build:**
- install chibi, Guile, Chicken, Gambit, Chez, Racket, Gauche and s7
  locally (most are in Homebrew), alongside s1;
- port ecraven's `r7rs-benchmarks` runner to include s1 and s2, and run
  the suite on every system on this machine. Medians of interleaved runs,
  as in P10. Programs s2 can't run are listed with the reason, and fixed
  if the missing feature is R7RS;
- run chibi's R7RS suite on the systems where it runs;
- `plan/07-comparison.md`: tables of completeness and features (numeric
  tower, macro systems, SRFIs, Unicode, threads, FFI, debugging, compiled
  files), of architecture (value representation, GC, continuations,
  compiler language and target), and of the benchmark results.

**Decides:** nothing in s2's design by itself. It records the baseline
that P13 is measured against, and may suggest work for later phases.

**Exit tests:** the benchmark suite runs from one script; s2's results are
reproducible within the noise found in P10.

---

## P13. The compiler in Scheme

**Goal:** rewrite the compiler passes, from the expander to codegen, in
Scheme, run them on s2, and have s2 compile itself.

**Teaches:** bootstrapping, as real Scheme systems do it (02, option E).

Done in this repository, not a fork (06 §1). Before it starts, the
repository is tagged `rust-compiler`. During it, a cargo feature keeps the
Rust compiler buildable as the oracle.

The Rust passes work on plain data with simple interfaces between them, so
they can be ported one at a time. For example, analysis in Scheme can feed
codegen in Rust, with each pass checked against the Rust version's output.
The finished compiler ships as a boot image in P11's format. The
expander can then load library files directly, which removes P8's
`pending_load`.

**Benchmark:** compile speed and run-time speed against P12's baseline.

**Decides:** whether the Rust compiler is kept, behind a feature, or
removed; whether to offer a runtime without a compiler (06 §2).

---

## Decisions by phase

| Decision | Settled in |
|---|---|
| D1 project strategy | before P0 |
| D2 value representation | P0 (revisited in P10) |
| D3 GC | P3 |
| D4 VM model, D5 encoding (first half) | P1 |
| D6 closures | P2, P4 |
| D7 globals | P1 |
| D8 continuations | P7 (revisited in P10) |
| D9 primitives | P7 (VM primitives), P10 (inlining) |
| D10 expander | P5 |
| D11 core AST | P1, grown through P5 |
| D5 encoding (second half) | P11 |
| U7-U10 directions after P10 | 06, after P10 |
