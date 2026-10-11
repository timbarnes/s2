# 05. Testing and measurement

How each phase is verified, and how speed is measured. The guiding idea is
that s1 already defines correct behaviour for nearly everything s2 will do,
so s2 can be checked against it continuously.

## 1. Test layers

| Layer | What it checks | Where | From |
|---|---|---|---|
| Unit tests | Reader, number syntax, printer, syntax-rules matcher, analysis results | inline `#[cfg(test)]` modules | Forked with the s1 modules. New ones for analysis |
| Golden disassembly | The compiler emits the expected code | `tests/golden/*.scm` with expected `.dis` output | New, from P1 |
| Expansion snapshots | The expander's output for each core and derived form | `tests/expand/*.scm` with expected core AST | New, from P5 |
| Scheme behaviour tests | Results of running programs | `tests/scheme/*.scm`, using the s1 harness | s1's harness, plus ported test files |
| Differential tests | s2 and s1 print the same thing for the same program | `tests/diff/*.scm` and a runner | New, from P1 |
| GC stress | Everything above, collecting at every safepoint, with poison-sweep | a flag or environment variable on all runs | s1's practice, from P3 |
| Conformance | chibi's R7RS suite | `tests/r7rs/` | Forked from s1, new shim in P9 |
| Process tests | CLI, exit status, scripts, `command-line` | `tests/process.rs` | Modelled on s1's |
| Benchmarks | Speed against s1 and earlier s2 builds | `bench/` | Forked from s1 |

`cargo test` runs the unit, golden, expansion, Scheme, differential and
process layers. A second command runs them all again under GC stress (P3 on).
The conformance suite and benchmarks have their own scripts, as in s1.

## 2. Golden disassembly tests

Each test is a source form and the exact disassembly expected from it:

```
;; tests/golden/if-global.scm
(if (< x 20) (+ x 1) 0)
;; expect:
;;   0  GlobalRef k0   ; <
;;   1  GlobalRef k1   ; x
;;   ...
```

These tests are the main way to *see* what the compiler does, which suits
the learning goal. They also catch changes to code generation that tests
on results would miss: for example, a variable boxed when it needn't be.
They break whenever codegen changes deliberately, so the runner should have
an `--update` mode that rewrites the expected output for review in the diff.

## 3. Differential testing against s1

s1 is frozen and is believed correct for R7RS. For any program that both
support:

```
s1 -q prog.scm > a.out ; s2 prog.scm > b.out ; diff a.out b.out
```

- A small runner (a shell script, or a Rust test that runs both binaries)
  goes over `tests/diff/*.scm`. It finds s1 through `S1_BIN`, defaulting to
  `../s1/target/release/s1`.
- Good programs for this: the derived-form semantics (R7RS §7.3), numeric
  edge cases (exactness, bignum boundaries, rationals), string and char
  procedures, `equal?` on cyclic data, printer output.
- Known, intended differences are listed in a file and skipped, for example
  macro use before definition (03 §12) or error message wording.

This is especially useful in P6, where most of the work is porting builtins:
any program that prints values exercises both implementations.

## 4. Porting s1's tests

The harness, `scheme/test-harness.scm` (85 lines: `test-equal`,
`test-true`, …), is portable and is copied as-is.

The test files, by portability (counts are the number of `(test-` forms):

| s1 file | Tests | Blocking extensions | Plan |
|---|---|---|---|
| `call_cc_tests.scm` | — | none (R7RS core only, per its header) | Copy unchanged, P7 |
| `tail_tests.scm` | 31 contexts | none known | Copy unchanged; parts in P2, all in P7 |
| `basic_tests.scm` | 421 | `type-of`, `nil`, `float?` (about 7 uses) | Port by replacing those uses; P1-P6 |
| `advanced_tests.scm` | 148 | `trace-procedure` | Port, removing tracing; P4-P7 |
| `derived_tests.scm` | 48 | `gc-threshold` | Port; s2's own stress flag replaces `gc-threshold`; P5 |
| `data_tests.scm` | 66 | `gc-threshold` | Port; P6 |
| `exception_tests.scm` | 38 | `gc-threshold` | Port; P7 |
| `port_tests.scm` | 42 | `gc-threshold`, `procedure-source` | Port; P6 |
| `syntax_rules_tests.scm` | 62 | `macro`, `gc-threshold` | Port the syntax-rules parts; P5 |
| `library_tests.scm` | 78 | `help`, `(s1)`, `type-of`, `features` values | Port; P8 |
| `macro_tests.scm` | 84 | `macro`, `expand`, `help`, `push!` | Keep only the hygiene tests; P5 |
| `system_tests.scm` | 16 | none of the above; may assume s1's CLI or environment | Review in P8 |
| `gc_stress_tests.scm` | 16 | `gc-threshold`, `with-timer`, `eval-string`, `macro` | Rewrite the ideas (deep capture under GC, many closures), P3/P7 |
| `help_tests.scm` | 18 | `help` | Drop |

Rule for porting: change a test only to remove an s1 extension, never to
change what it expects. When a ported test fails on s2, s1's behaviour is
the reference unless R7RS says otherwise.

## 5. The R7RS conformance suite

- `tests/r7rs/r7rs-tests.scm` (chibi's suite, unmodified), `run.sh` (which
  splits it into 20 sections and compares counts with `baseline.txt`), and
  `LICENSE-chibi` are copied from s1.
- `shim.scm` must be rewritten: s1's version stands in for `(chibi test)`
  using s1's procedural `macro`. A `syntax-rules` version is about 100-150
  lines (`test`, `test-begin`, `test-end`, `test-assert`, `test-error`, and
  so on).
- From P6 on, run whichever sections the system supports. A simple
  temporary shim is enough until P9.
- The target is s1's baseline: 1,154 passing, 0 failing, 0 errors, with
  only the complex-number tests excluded.

## 6. GC stress

From P3 on, every test layer runs a second time with:

- **collect at every safepoint** (the equivalent of s1's
  `(gc-threshold 1)`), as a flag or environment variable, never as Scheme
  code, so test files stay portable;
- **poison-sweep:** freed objects are overwritten and leaked rather than
  freed, so a use after free produces garbage that the tests notice instead
  of silently working (s1's `S1_GC_POISON`).

s1 found its worst bugs only once both of these existed (01 §6).

## 7. Benchmarks

The workloads, from `../s1/bench/`:

| Name | What it stresses |
|---|---|
| `fib.scm` (fib 25 and fib 30) | Calls and fixnum arithmetic |
| `program.scm`, the "typical program" | Records, closures, internal definitions, `map`/`for-each`, sorting, string ports |
| `macro-loop.scm` | A loop using small `syntax-rules` macros (in s2, macros cost nothing at run time, so this measures the loop) |
| tail loop, 300k | Tail calls |
| deep recursion, 500k | Deep non-tail recursion and GC with a deep stack |
| list/map | Allocation and `map` |

Added for s2:
- a **generator benchmark** using `call/cc` re-entry at some depth, which
  decides whether D8's copying strategy needs upgrading (P10);
- a **`guard`-heavy loop**, which measures `call/ec` and the exception
  machinery written in Scheme.

**Method, following s1's practice** (`design/performance.md`):
- release builds;
- interleaved runs of the two binaries being compared, never back to back;
- medians of 7-11 runs, because laptop CPU boost makes consecutive runs
  drift;
- the same machine within a table.

`bench/bench.sh` (forked) prints a table of s1 against s2. Each phase note
from P2 on records the numbers.

**Instrumentation for P10:** a Cargo feature that counts executed
instructions by opcode and by pair of consecutive opcodes. The pairs show
which superinstructions are worth adding.

## 8. What "done" means for a phase

1. All tests from this and earlier phases pass, normally and under GC
   stress.
2. Golden disassembly is updated and reviewed for any codegen change.
3. Differential tests pass, apart from listed intended differences.
4. Benchmark numbers are recorded, where the phase lists a checkpoint.
5. The phase note, `plan/phases/Pn.md`, is written: what was built, the
   decisions made and why, what was surprising, the measurements.
