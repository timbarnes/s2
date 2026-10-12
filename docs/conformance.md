# Standards Conformance

s2 implements [R7RS-small](https://standards.scheme.org/corrected-r7rs/r7rs.html) except for complex numbers. This page lists where s2 departs from the report, and how it settles the questions the report leaves to each implementation. The other pages note the same points in context.

## Test suite

s2 is tested against [chibi-scheme's R7RS test suite](https://github.com/ashinn/chibi-scheme/blob/master/tests/r7rs-tests.scm), vendored in `tests/r7rs/`. Every test passes except those that use complex numbers. In the Numbers and Numeric syntax sections, which hold nearly all of those, the remaining tests don't run, because the reader rejects complex literals.

```bash
tests/r7rs/run.sh      # run the suite and compare with the recorded baseline
```

`cargo test` runs s2's own tests: the compiler and VM on small programs, golden listings of compiled code, tail calls in every tail context, the error messages, programs run as processes, compiled files, and the R7RS suite. The compiler and VM tests run with the garbage collector collecting at every safepoint. [How s2 compares with other Schemes](design/07-comparison.md) runs the same suite on eight other systems.

## Not implemented

* **Complex numbers.** The reader rejects complex literals such as `1+2i`, `sqrt` and `expt` raise an error where the result would be complex (`(sqrt -4)`, `(expt -8 1/3)`), and `log`, `asin` and `acos` return `+nan.0` (`(log -1)`). `complex?` is the same as `real?`. `(scheme complex)` exists but exports none of its six procedures (`make-rectangular`, `make-polar`, `real-part`, `imag-part`, `magnitude`, `angle`), and `(features)` doesn't include `exact-complex`. See [Numbers](./numbers.md).
* **`transcript-on` and `transcript-off`**, which were in R5RS but are not in R7RS or `(scheme r5rs)`.

`(%library-unimplemented '(scheme complex))` lists the names a standard library should export but s2 doesn't define.

## Departures from the report

These are places where s2 accepts something R7RS calls an error, or behaves differently from what it requires.

| Behaviour | R7RS | s2 |
| --- | --- | --- |
| Program with no `import` | must start with `import` | allowed; every standard binding is already visible ([Program Structure](./program-structure.md#programs)) |
| `[a b c]` | not defined (many Schemes read it as a list) | a vector literal ([Lexical Syntax](./lexical-syntax.md#s2-differences)) |
| Modifying a literal constant | error | allowed, and changes the constant |
| `current-second` | seconds in TAI | seconds in UTC from the system clock, 37 behind TAI in 2026 ([System Interface](./system-interface.md#current-second)) |
| Errors in macro uses | not specified when they are reported | reported when the top-level form containing the use is compiled, even if that code never runs ([Macros](./macros.md#errors-appear-when-a-form-is-compiled)) |

## Implementation choices

R7RS leaves these to the implementation.

| Question | s2's answer |
| --- | --- |
| Order of evaluating a call's operator and operands | left to right, operator first |
| Order `map` applies its procedure | first element to last |
| Value of `define` | the symbol defined (the REPL prints `name`) |
| Unspecified values (`(if #f #f)`, `set!`, ...) | a value that prints as `#<unspecified>`; the REPL shows nothing for it |
| `letrec` | evaluates its inits left to right, like `letrec*` |
| `eq?` on numbers and characters | the same as `eqv?` for characters, fixnums and flonums; bignums and ratios are `eq?` only to themselves |
| Numbers | exact integers of any size, exact rationals, and 64-bit IEEE flonums ([Numbers](./numbers.md)) |
| Characters and strings | all Unicode scalar values; `string-ref` is constant time for ASCII strings and linear for others ([Strings](./strings.md)) |
| Jiffies | nanoseconds since the first call of `current-jiffy` ([System Interface](./system-interface.md#current-jiffy)) |
| Exit status of `(exit obj)` | 0 for no `obj` or `#t`, 1 for `#f`, `obj` itself for an exact integer ([System Interface](./system-interface.md#exit)) |
| Feature identifiers | `r7rs exact-closed ieee-float full-unicode ratios s2`, plus the OS, architecture and byte order ([Libraries](./libraries.md#cond-expand-and-features)) |
| Library file names | `(foo bar)` is `foo/bar.sld`, searched on `S2_LIBRARY_PATH`, then `.`, then the installation's library directory ([Libraries](./libraries.md#library-files)) |
| Uncaught errors | reported on standard error, with a backtrace; a program then exits with status 70, the REPL carries on ([Exceptions](./exceptions.md#uncaught-exceptions)) |

## Known limitations

* Errors don't give a source location, only a backtrace of procedure names.
* Indexing a string that contains non-ASCII characters (`string-ref`, `string-set!`, `substring`) scans from the start.
* `call/cc` copies the VM's stack, so capturing a continuation costs time in proportion to the depth of the stack.
