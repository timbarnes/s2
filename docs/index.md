# s2 Scheme

s2 is a Scheme compiler and virtual machine written in Rust. It implements [R7RS-small](https://standards.scheme.org/corrected-r7rs/r7rs.html), the current Scheme standard, with one deliberate omission: there are no complex numbers. Everything else in the standard is there: hygienic `syntax-rules` macros, libraries and `import`, exact rationals and arbitrary-size integers, full Unicode characters and strings, first-class continuations, `dynamic-wind`, exceptions, parameters, records, bytevectors, binary and textual ports, and proper tail calls.

s2 compiles each top-level form to bytecode for a stack-based virtual machine, and runs it. Programs and libraries can also be compiled to files ahead of time. It is a learning project, the successor to the interpreter [s1](https://github.com/timbarnes/s1), and it is about five times faster than s1. The [comparison with other Schemes](design/07-comparison.md) shows where it stands.

These pages are a quick guide. Each one summarizes what a part of the language does in s2 and notes where s2 makes a choice the standard leaves open. For the precise definition of any form or procedure, follow the links to the R7RS report:

* [R7RS-small, corrected HTML edition](https://standards.scheme.org/corrected-r7rs/r7rs.html) (the version these pages link to; it includes the published errata)
* [R7RS-small, official PDF](https://small.r7rs.org/attachment/r7rs.pdf)

## Getting started

```bash
cargo build --release
target/release/s2                    # start the REPL
target/release/s2 hello.scm a b      # run a program with arguments, then exit
target/release/s2 --compile hello.scm && target/release/s2 hello.s2o a b
```

At the REPL, each result is written on a line of its own:

```scheme
s2> (define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))
fact
s2> (fact 20)
2432902008176640000
s2> (exact->inexact 1/3)
0.3333333333333333
```

A program can begin with the usual `(import (scheme base) ...)`, but doesn't need to: every standard binding is already available. See [Program Structure](./program-structure.md) for how programs and the REPL work, and [System Interface](./system-interface.md) for the command line.

## Conformance

s2 passes every test in chibi-scheme's R7RS test suite except those involving complex numbers. [Standards Conformance](./conformance.md) lists where s2 differs from the report and how it settles the questions the report leaves to the implementation.

## Contents

The pages follow the order of the R7RS report.

| Page | R7RS |
| --- | --- |
| [Lexical Syntax](./lexical-syntax.md): identifiers, comments, literals, datum labels | ch. 2 |
| [Primitive Expressions](./primitive-expressions.md): variables, `quote`, calls, `lambda`, `if`, `set!` | 4.1 |
| [Derived Expressions](./derived-expressions.md): `cond`, `case`, `let` forms, `do`, `delay`, `parameterize`, quasiquote, `case-lambda` | 4.2 |
| [Macros](./macros.md): `define-syntax`, `syntax-rules`, hygiene | 4.3 |
| [Program Structure](./program-structure.md): programs, definitions, the REPL | 5.1–5.4, 5.7 |
| [Records](./records.md): `define-record-type` | 5.5 |
| [Libraries](./libraries.md): `import`, `define-library`, `cond-expand` | 5.2, 5.6 |
| [Equivalence Predicates](./equivalence-predicates.md) | 6.1 |
| [Numbers](./numbers.md) | 6.2 |
| [Booleans](./booleans.md) | 6.3 |
| [Pairs and Lists](./pairs-and-lists.md) | 6.4 |
| [Symbols](./symbols.md) | 6.5 |
| [Characters](./characters.md) | 6.6 |
| [Strings](./strings.md) | 6.7 |
| [Vectors](./vectors.md) | 6.8 |
| [Bytevectors](./bytevectors.md) | 6.9 |
| [Control Features](./control-features.md): procedures, `apply`, `map`, continuations, `values`, `dynamic-wind`, tail calls | 6.10, 3.5 |
| [Exceptions](./exceptions.md) | 6.11 |
| [Environments and Evaluation](./environments.md) | 6.12 |
| [Input and Output](./input-and-output.md) | 6.13 |
| [System Interface](./system-interface.md): running s2, compiled files, `exit`, environment variables, time | 6.14 |
| [Standards Conformance](./conformance.md) | |
| [s2 Extensions](./extensions.md): what s2 adds to the standard | |

How s2 is built is documented in the [design notes](design/README.md), which record each decision and each phase of the work, and in the [internal docs](https://timbarnes.github.io/s2/api/s2/) generated from the source.
