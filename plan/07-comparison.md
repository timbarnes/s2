# 07. How s2 compares with other Schemes

Written 2026-10-11, in P12 (06 §3). Everything measured here was measured
on one machine, an Apple M2 Max running macOS 27.0.1, with the scripts in
`bench/r7rs/`, so it can be run again.

In short:

- **Performance.** Of these ten systems, only chibi and s1 are slower than
  s2. Over the benchmarks both finish, chibi takes 2.9 times as long as s2, and s1
  4.8 times. Gauche, the closest design to s2's, takes 0.55 of s2's time.
  The native-code compilers (Chez, Gambit, Racket) take a twentieth to a
  tenth.
- **Completeness.** s2 passes everything in chibi's R7RS suite that it
  reaches, as do the others. The one gap is complex numbers, which every
  other system has.
- **Features.** s2 has none of the large systems' features beyond
  R7RS-small (FFI, threads, low-level macros, source locations), but it
  does have compiled files.
- **Architecture.** s2 is a textbook bytecode VM, in the family of chibi
  and Gauche.

## The systems

| System | Version | Installed from | Design in one line |
|---|---|---|---|
| **s2** | P11 (8452ec4 + P12) | this repository | bytecode compiler and stack VM, in Rust |
| **s1** | its last build | `../s1` | CEK-machine interpreter, in Rust; s2's oracle |
| chibi | 0.12.0 | Homebrew | small C bytecode VM; R7RS's reference implementation |
| Guile | 3.0.11 | Homebrew | compiler written in Scheme; bytecode VM with a JIT |
| Gauche | 0.9.15 | Homebrew | C bytecode VM, compiler written in Scheme |
| Chicken | 6.0.0 | Homebrew | compiles Scheme to C |
| Gambit | 4.9.8 | Homebrew | compiles Scheme to C |
| Chez | 10.4.1 | Homebrew | compiles Scheme to native code |
| Racket | 9.3 (CS) | Homebrew `minimal-racket`, and its `r7rs` package | Racket's expander over Chez |
| s7 | 11.9 (9-Oct-2026) | built from source, `clang -O2` | embeddable C interpreter |

## Architecture

How each system answers the questions in 03. Rows marked † were checked on
the installed systems here (what they link, what they print); the rest are
from each system's documentation and published papers.

| | Compiles to | Compiler written in | Garbage collector | Continuations | Macros |
|---|---|---|---|---|---|
| **s2** | bytecode for a stack VM (an `enum` per instruction) | Rust | precise, non-moving mark-sweep | copy the VM's stacks (D8) | `syntax-rules`, hygiene by renaming |
| **s1** | nothing: it interprets the code as a CEK machine | — | precise mark-sweep | heap-allocated continuation frames: capture is O(1) | `syntax-rules`, and a non-hygienic `macro` |
| chibi | bytecode for a stack VM | C, with libraries in Scheme | its own precise, non-moving collector † (Boehm is an option, off) | copies of the VM stack | syntactic closures; `syntax-rules` and explicit renaming built on them |
| Guile | bytecode for a register VM, JIT-compiled to machine code | Scheme: Tree-IL, then CPS, then bytecode | Boehm, conservative † (links `libgc`) | delimited continuations (prompts) are primitive; `call/cc` copies the stack | `syntax-case` (psyntax) |
| Gauche | bytecode for a stack VM | Scheme, precompiled to C | Boehm, conservative † (bundled) | frames moved to the heap when captured | `syntax-rules`, explicit renaming, traditional macros |
| Chicken | C, after CPS conversion | Scheme | generational: the C stack is the nursery ("Cheney on the M.T.A.") | free: after CPS, every continuation is a closure | `syntax-rules`, explicit and implicit renaming |
| Gambit | C, through its own virtual machine (GVM) | Scheme | copying | heap-allocated frames, with a stack cache | `syntax-rules`, `define-macro`, `syntax-case` |
| Chez | native machine code | Scheme (nanopass) | generational, copying | segmented stack: capture is O(1) | `syntax-case` |
| Racket | native code, through Chez | Racket (the expander), then Chez's compiler | Chez's | Chez's, under Racket's delimited control | sets of scopes; `syntax-case`, `syntax-parse` |
| s7 | nothing: an interpreter that rewrites calls into specialised operations as it runs them | — | mark-sweep | copies of the stack | `define-macro` (not hygienic), first-class environments |

Where s2 sits: in the family of chibi and Gauche (a compiler to bytecode
for a stack VM, no JIT), with the compiler in its implementation language
like chibi, not in Scheme like Gauche and Guile. P13 moves it to Guile's
and Gauche's side.

## Completeness

### Chibi's R7RS suite

`bench/r7rs/conformance.sh` runs the suite as `tests/r7rs/run.sh` does: one
section at a time, through the same stand-in for `(chibi test)`
(`tests/r7rs/shim.scm`). Here every system runs each section as a program,
so a top-level error ends the section: one unsupported feature can cost
every test after it. s2's own runner uses `-f`, which carries on, so its
1,154 is higher than its 794 here.

| section | s2 | s1 | chibi | Guile | Gauche | Chicken | Gambit | Racket |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 4.1 Primitive expression types | 27 | 27 | 27 | 27 | 27 | 27 | 27 | 27 |
| 4.2 Derived expression types | 74 | 74 | 74 | 68 | 74 | 74 | 74 | 74 |
| 4.3 Macros | 25 | 25 | 25 | stopped | 25 | stopped | stopped | stopped |
| 5 Program structure | 15 | 15 | 15 | 15 | 15 | 15 | stopped | 15 |
| 6.1 Equivalence predicates | 25 | 25 | 25 | 25 | 25 | 25 | 25 | 25 |
| 6.2 Numbers | stopped | stopped | stopped | 215 | 218 | 214 | 217 | 217 |
| 6.3 Booleans | 18 | 18 | 18 | 18 | 18 | 18 | 18 | 18 |
| 6.4 Lists | 65 | 65 | 65 | 63 | 65 | 65 | 65 | 65 |
| 6.5 Symbols | 17 | 17 | 17 | 17 | 17 | 17 | 17 | 17 |
| 6.6 Characters | 79 | 79 | 79 | 79 | 79 | 74 | 79 | 79 |
| 6.7 Strings | 130 | 130 | 130 | 129 | 130 | 126 | 130 | 130 |
| 6.8 Vectors | 43 | 43 | 43 | 41 | 43 | 43 | 43 | 43 |
| 6.9 Bytevectors | 39 | 39 | 39 | 39 | 39 | 37 | 39 | 39 |
| 6.10 Control features | 34 | 34 | 34 | 31 | stopped | 33 | 34 | 34 |
| 6.11 Exceptions | 30 | 30 | 30 | 29 | 30 | 30 | stopped | 29 |
| 6.12 Environments and evaluation | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| 6.13 Input and output | 63 | 63 | 63 | 62 | 63 | stopped | 58 | 63 |
| Read syntax | 93 | 93 | 93 | stopped | 93 | 83 | 87 | 90 |
| Numeric syntax | stopped | stopped | 220 | 201 | 201 | 209 | 206 | 214 |
| 6.14 System interface | 13 | 13 | 13 | 12 | 13 | 13 | 12 | 12 |
| **total** | **794** | **794** | **1,014** | **1,075** | **1,179** | **1,107** | **1,131** | **1,195** |

Why sections stopped:

- **s2 and s1**: a complex-number literal (`3+4i`, `1+2i`), in Numbers and
  Numeric syntax. The two lack only complex numbers (P9).
- **chibi**: its reader rejects a character literal in its own Numbers
  section ("invalid numeric syntax: #\\n").
- **Guile**: Macros, at a body that defines a name and then uses
  `let-syntax` ("invalid or duplicate identifier in definition"); Read
  syntax, at a string escape it doesn't accept.
- **Gauche**: `(test-error (apply +))`. Gauche checks `apply`'s argument
  count while compiling, so the error ends the section instead of being
  caught by the test.
- **Chicken**: Macros, at an ellipsis after a nested ellipsis pattern
  ("too many ellipses"); Input and output produced nothing within the
  60-second limit.
- **Gambit**: Macros, at an ill-formed special form; Program structure, at
  `except` in an import set; Exceptions produced nothing within the
  limit.
- **Racket**: Macros, at a `define-values` that binds a name twice.

Read this table with care:

- **It is chibi's suite.** s2 was tested against it from P6 to P9, so
  s2's numbers flatter it more than the others'.
- **It counts sections that ran.** It doesn't measure what the systems do
  beyond R7RS-small, which is most of what the larger ones are for.

Within those limits: every system passes nearly everything in the
sections it reaches. Where s2 falls short, it is complex numbers, which
every other system here has.

**Chez is not in the table.** It is an R6RS system: its reader rejects
R7RS syntax such as `#u8(...)`, and it has no R7RS libraries. The layer in
`bench/r7rs/chez/` provides just what the benchmarks need, which is far
from a complete R7RS. **s7** has no `import`.

### Probes

A short program, `bench/r7rs/probe.scm`, asked each system for bignums,
exact ratios, complex numbers, Unicode case mapping, the shortest float
that reads back, and R7RS's `(features)`:

| | bignums | ratios | complex | `(char-upcase #\λ)` | `(display 0.1)` |
|---|---|---|---|---|---|
| s2, s1 | yes | yes | **no** | `#\Λ` | `0.1` |
| chibi, Guile, Gauche, Chicken, Gambit, Racket, Chez | yes | yes | yes | `#\Λ` | `0.1` (Gambit `.1`; Chicken prints 15 digits, not the shortest) |

## Features beyond R7RS-small

From each system's documentation:

| | Standards beyond R7RS-small | Low-level macros | Threads | FFI | Debugging | Compiled files |
|---|---|---|---|---|---|---|
| **s2** | none | no | no | no | backtrace of procedure names; no source locations (P9) | yes, `.s2o` (P11) |
| **s1** | none | `macro` (not hygienic) | no | no | debugger, tracer, `help` | no |
| chibi | many SRFIs, R7RS-large libraries | syntactic closures, ER | yes (green) | C | REPL | images |
| Guile | R6RS, many SRFIs | `syntax-case` | yes (OS) | C (`libffi`) | debugger, profiler, source locations | yes, `.go` |
| Gauche | many SRFIs | ER, traditional | yes (OS) | C (generated stubs) | REPL, profiler | precompiled modules |
| Chicken | many SRFIs, eggs | ER, IR | yes (green) | C, inline | REPL | native executables |
| Gambit | many SRFIs | `define-macro`, `syntax-case` | yes (green, SMP optional) | C, inline | REPL debugger | native executables |
| Chez | R6RS | `syntax-case` | yes (OS) | C | inspector, debugger, profiler | `.so` boot and object files |
| Racket | its own large language and libraries | `syntax-case`, `syntax-parse` | yes (green and OS) | C | debugger, profiler, DrRacket | `.zo` |
| s7 | its own | `define-macro` | no | C (designed for embedding) | REPL | no |

s2 sits at the small end on every count but compiled files. That is by
design (README non-goals: the core stays pure), and these are the
features that would make it practical rather than educational:

- source locations in errors (deferred in P9);
- complex numbers (left out since P9);
- low-level macros, which P13's expander in Scheme makes possible (06
  §2);
- an FFI.

## Performance

ecraven's [r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks),
57 programs, run by `bench/r7rs/run.sh`:

```
bench/r7rs/run.sh -n 3 -t 120 "s2 chibi guile gauche s7 chicken gambit chez racket"
bench/r7rs/run.sh -n 1 -t 120 "s2 s1"
```

**How it was run:**

- **Scale.** Each benchmark's iteration count is divided by 10. The suite
  sizes its inputs for native-code Schemes, about a second each; at full
  size s2 would need an hour per round. Benchmarks that already do one
  iteration (`tak`, `takl`, `ntakl`, `cpstak`, `fib`, `ack`) run at full
  size.
- **Timing.** Each program times itself, so startup and compiling aren't
  counted: Chicken's and Gambit's C compiles, and Guile's and Racket's
  compiles to their own formats, all happen beforehand.
- **Medians.** The systems take turns, three rounds, and the table shows
  each one's median. s1 was run in a separate round of its own, against
  s2.
- **Limit.** A run that takes more than 120 seconds is stopped (TIMEOUT).
- **Compiler settings.** Each system runs as the suite's own scripts run
  it (`guild compile -O3`, `csc -O3`, `gsc -exe`, `raco make`, Chez at
  `--optimize-level 2`). Guile's and Chez's settings are those of the
  suite's published results.

**Relative to s2.** This is the geometric mean, over the benchmarks both
systems finished, of each system's time divided by s2's. Below 1 is
faster.

| system | time relative to s2 | benchmarks |
|---|---:|---:|
| Chez | 0.054 (18x faster) | 56 |
| Gambit | 0.063 (16x) | 56 |
| Racket | 0.095 (11x) | 56 |
| Guile | 0.148 (7x) | 55 |
| s7 | 0.232 (4x) | 51 |
| Chicken | 0.236 (4x) | 54 |
| Gauche | 0.550 (2x) | 56 |
| **s2** | **1** | 56 |
| chibi | 2.89 (3x slower) | 50 |
| s1 | 4.80 (5x slower) | 47 |

**Medians, in seconds** (at a tenth of the suite's iteration counts):

| benchmark | s2 | s1 | chibi | Guile | Gauche | s7 | Chicken | Gambit | Chez | Racket |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| browse | 3.089 | 23.682 | 75.780 | 0.289 | 1.303 | 0.917 | 0.662 | 0.096 | 0.081 | 0.189 |
| deriv | 3.522 | 20.475 | 5.052 | 0.387 | 1.858 | 0.909 | 0.360 | 0.108 | 0.077 | 0.107 |
| destruc | 3.662 | 23.731 | 5.183 | 0.212 | 1.528 | 0.691 | 0.695 | 0.160 | 0.116 | 0.193 |
| diviter | 2.581 | 21.018 | 3.163 | 0.382 | 1.034 | 0.305 | 0.198 | 0.168 | 0.099 | 0.143 |
| divrec | 2.968 | 23.546 | 2.974 | 0.616 | 1.381 | 0.485 | 0.485 | 0.181 | 0.122 | 0.178 |
| puzzle | 6.617 | 36.777 | 14.959 | 0.464 | 2.013 | 0.779 | 1.071 | 0.190 | 0.163 | 0.202 |
| triangl | 4.410 | 30.357 | 5.076 | 0.482 | 1.485 | 1.287 | 1.131 | 0.127 | 0.172 | 0.157 |
| tak | 32.811 | TIMEOUT | 35.585 | 2.323 | 13.432 | 20.547 | 10.997 | 1.337 | 1.263 | 1.344 |
| takl | 111.879 | TIMEOUT | TIMEOUT | 3.993 | 58.207 | 7.076 | 10.795 | 1.598 | 1.847 | 3.112 |
| ntakl | 85.934 | TIMEOUT | 58.334 | 3.810 | 50.137 | 6.923 | 10.251 | 1.505 | 1.759 | 2.923 |
| cpstak | 74.034 | TIMEOUT | 118.550 | 8.306 | 58.028 | 32.697 | 9.788 | 3.020 | 1.651 | 2.133 |
| ctak | 25.003 | 48.718 | TIMEOUT | 42.016 | 12.650 | 8.660 | 2.254 | 1.709 | 0.429 | 1.287 |
| fib | 11.937 | 79.029 | 9.278 | 0.989 | 5.400 | 0.858 | 4.997 | 0.388 | 0.486 | 0.514 |
| fibc | 2.160 | 6.388 | 5.219 | 2.206 | 1.087 | 0.600 | 0.201 | 0.107 | 0.032 | 0.102 |
| fibfp | 3.374 | 6.378 | 2.224 | 0.634 | 0.662 | 0.078 | 0.560 | 0.085 | 0.144 | 0.164 |
| sum | 5.262 | 47.954 | 5.783 | 0.237 | 2.136 | 0.390 | 1.632 | 0.164 | 0.212 | 0.248 |
| sumfp | 5.904 | 11.033 | 4.158 | 1.172 | 0.848 | 0.177 | 0.577 | 0.234 | 0.207 | 0.239 |
| fft | 1.972 | 17.071 | 3.350 | 0.249 | 0.585 | 0.806 | 0.265 | 0.150 | 0.134 | 0.149 |
| mbrot | 6.912 | 28.973 | 9.694 | 1.214 | 1.302 | 0.907 | 0.587 | 0.293 | 0.416 | 0.400 |
| mbrotZ | ERROR | ERROR | 21.655 | 1.178 | 1.966 | 0.600 | 1.421 | 0.500 | 0.300 | 0.322 |
| nucleic | 2.620 | 10.068 | 4.112 | 0.321 | 1.056 | 0.691 | 0.283 | 0.090 | 0.120 | 0.136 |
| pi | 0.039 | 0.042 | 32.431 | 0.006 | 0.230 | ERROR | 0.036 | 0.043 | 0.031 | 0.042 |
| pnpoly | 7.778 | 23.940 | 13.626 | 0.928 | 1.451 | 1.091 | 1.077 | 0.185 | 0.294 | 0.329 |
| ray | 3.321 | 14.214 | 10.579 | 0.402 | 1.176 | 0.888 | 0.359 | 0.094 | 0.154 | 0.286 |
| simplex | 4.917 | 51.162 | 10.379 | 0.395 | 2.200 | 1.957 | 0.608 | 0.099 | 0.138 | 0.168 |
| ack | 26.639 | TIMEOUT | 22.304 | 3.462 | 26.689 | 3.717 | 11.822 | 0.858 | 0.753 | 1.081 |
| array1 | 4.932 | 27.082 | 3.986 | 0.341 | 1.520 | 0.197 | 0.836 | 0.128 | 0.286 | 0.306 |
| string | 0.054 | 0.063 | 0.476 | 0.013 | 0.077 | 0.017 | 0.025 | 0.043 | 0.077 | 0.116 |
| sum1 | 0.105 | 0.164 | 3.809 | 0.273 | 0.065 | 0.016 | 0.712 | 0.029 | 0.043 | 0.725 |
| cat | 2.072 | 57.608 | 3.484 | 0.979 | 0.868 | 0.067 | 1.755 | 0.170 | 0.196 | 0.416 |
| tail | 0.239 | 0.925 | 0.539 | 0.405 | 0.342 | 0.034 | 0.219 | 0.190 | 0.199 | 0.213 |
| wc | 2.473 | 17.139 | 23.250 | 0.601 | 1.236 | 0.393 | 0.839 | 0.104 | 0.093 | 0.250 |
| read1 | 0.082 | 0.085 | 12.102 | 0.414 | 0.157 | 0.018 | 1.843 | 0.077 | 0.052 | 1.256 |
| compiler | 2.222 | 15.541 | 5.883 | 0.253 | 1.461 | 1.185 | 0.388 | 0.213 | 0.105 | 0.157 |
| conform | 5.223 | 28.377 | 7.852 | 0.436 | 2.670 | 2.454 | 0.782 | 0.190 | 0.123 | 0.309 |
| dynamic | 2.267 | 13.700 | 13.329 | 0.397 | 1.117 | ERROR | TIMEOUT | 0.270 | 0.123 | 0.984 |
| earley | 25.583 | TIMEOUT | TIMEOUT | 4.548 | 7.113 | 26.832 | 6.615 | 2.617 | 1.888 | 5.850 |
| graphs | 29.676 | TIMEOUT | TIMEOUT | 2.215 | 18.529 | 17.977 | 2.652 | 2.364 | 0.436 | 0.592 |
| lattice | 10.987 | 87.910 | 64.834 | 0.690 | 3.874 | 5.472 | 1.176 | 0.307 | 0.218 | 0.270 |
| matrix | 3.732 | 28.320 | 11.816 | 0.295 | 2.012 | 1.688 | 0.487 | 0.112 | 0.075 | 0.104 |
| maze | 1.958 | 15.147 | 4.527 | 0.175 | 1.176 | 0.909 | 0.415 | 0.036 | 0.054 | 0.064 |
| mazefun | 3.517 | 17.471 | 5.888 | 0.341 | 2.192 | 0.780 | 0.863 | 0.144 | 0.130 | 0.189 |
| nqueens | 8.160 | 99.082 | 9.494 | 0.672 | 4.459 | 2.149 | 1.912 | 0.396 | 0.343 | 0.395 |
| paraffins | 0.946 | 8.274 | TIMEOUT | 0.434 | 1.149 | 0.291 | 1.077 | 0.364 | 0.332 | 0.624 |
| parsing | 3.996 | 58.758 | 107.014 | 0.468 | 7.383 | 1.630 | 0.782 | 0.140 | 0.175 | 0.153 |
| peval | 4.918 | 37.225 | 9.882 | 0.440 | 2.575 | INCORRECT | 0.661 | 0.143 | 0.128 | 0.185 |
| primes | 1.278 | 9.823 | 1.168 | 0.211 | 0.809 | 0.294 | 0.318 | 0.060 | 0.055 | 0.085 |
| quicksort | 5.042 | 35.211 | 16.180 | 0.569 | 2.582 | 4.109 | 1.076 | 0.274 | 0.263 | 0.728 |
| scheme | 3.780 | 14.826 | 6.501 | 0.580 | 1.913 | INCORRECT | 1.030 | 0.280 | 0.149 | 0.260 |
| slatex | 1.439 | 5.517 | 7.632 | 1.285 | 1.352 | 0.984 | 1.057 | 0.853 | 0.763 | 1.225 |
| chudnovsky | 0.110 | 0.183 | 11.965 | 0.009 | 0.074 | ERROR | 0.020 | 0.019 | 0.013 | 0.020 |
| nboyer | 22.642 | TIMEOUT | 21.779 | 2.108 | 8.472 | 17.011 | 6.417 | 1.929 | 1.197 | 3.153 |
| sboyer | 23.724 | TIMEOUT | 22.434 | 1.857 | 9.852 | 15.334 | 3.964 | 0.732 | 0.569 | 0.900 |
| gcbench | 36.802 | 83.124 | TIMEOUT | 1.034 | 12.774 | 7.562 | 3.026 | 4.452 | 0.483 | 1.018 |
| mperm | 7.935 | 49.577 | 16.427 | 0.888 | 2.810 | 2.018 | 1.918 | 0.602 | 0.603 | 1.834 |
| equal | 0.632 | 0.751 | 23.678 | TIMEOUT | 7.793 | 0.449 | TIMEOUT | 0.184 | 0.414 | 1.625 |
| bv2string | 0.941 | 7.158 | 0.627 | 0.111 | 0.609 | 0.360 | 0.387 | 0.088 | 0.056 | 0.086 |

Failures:

- **s2 and s1**: `mbrotZ` needs complex numbers.
- **s1** timed out on the nine slowest benchmarks.
- **chibi** timed out on six.
- **s7**:
  - failed `pi` and `chudnovsky` (its bignums);
  - failed `dynamic`;
  - gave wrong answers for `peval` and `scheme`.
- **Guile** timed out on `equal`.
- **Chicken** timed out on `dynamic` and `equal`.

These match the suite's published results for those systems. On the
benchmarks both sets of results cover, the times here are close to the
published ones: Chicken's `tak` takes 11.0s here and 10.8s there.

**Reproducibility** (the phase's exit test):

- Within a run, s2's three times for a benchmark differ by at most 10%
  (`primes`); most are within 3%.
- Between the two runs, s2's medians agree within 0.94-1.10x, 1.007 on
  average. That is the same noise as in P10.

### What the numbers say about s2

**Where s2 stands.** s2 is between chibi and Gauche, the two systems built
the same way:

- **chibi** is a C bytecode VM that keeps its interpreter simple, and s2
  beats it on most benchmarks. The difference is largest where chibi's
  strings and symbols are slow (`browse`, `parsing`, `read1`, `wc`).
- **Gauche** has had twenty years of tuning: specialised instructions,
  inlined primitives, and a compiler that optimises before generating code.
  It is twice as fast as s2 overall.
- **Guile** compiles through CPS with real optimisation, and JIT-compiles
  its bytecode to machine code. It is 7 times faster.
- **The native-code compilers** are 10-18 times faster. That is the
  usual factor between a good bytecode VM and native code.

**Where s2 does well.** It beats Gauche on:

- `equal` (12x);
- `pi` (6x), on Rust's bignums;
- `read1`, `parsing`, `string` and `tail`, on its reader and string
  operations.

**Where it does worst.** Against Gauche, the biggest gaps are:

- **floating-point code**: `sumfp` (7x slower), `pnpoly`, `mbrot` and
  `fibfp` (5x). s2's flonums are unboxed (D2), but its arithmetic
  instructions have an inline path only for fixnums
  (`src/vm/machine.rs`). Every flonum operation leaves it for the general
  numeric code;
- `earley`, `fft`, `puzzle` and `array1` (3.2-3.6x): vector access, and
  loops of generic arithmetic;
- `gcbench` (2.9x): allocation and collection.

These suggest where s2's time goes:

- flonums on the slow path of every arithmetic instruction;
- the cost of a generic instruction even on fixnums;
- a non-generational collector.

The first is a small change. The others are larger, and none of them is in
the plan yet. P13 is
measured against this table: s2 running a compiler written in Scheme
should produce code as fast as this, and be no slower to compile than 06
estimates.
