# Program Structure

R7RS: [chapter 5, Program structure](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_chap_5). Record types (5.5) and libraries (5.2, 5.6) have their own pages: [Records](./records.md) and [Libraries](./libraries.md).

## Programs

A program is a file of definitions and expressions, evaluated in order. R7RS requires a program to start with an `import` declaration naming the libraries it uses; in s2 that is optional. Programs run in the [interaction environment](./environments.md#the-system-and-interaction-environments), which already contains every standard binding, so

```scheme
(import (scheme base) (scheme write))
(display "hello")
(newline)
```

and the same file without its first line both work. `import` is still useful for loading your own libraries, or to rename (see [Libraries](./libraries.md#import)).

Run a program with `s2 program.scm [arg ...]`. It exits when the last form has been evaluated, or when it calls `exit`. See [System Interface](./system-interface.md#running-s2).

Each top-level form is expanded and compiled as a whole, then run, before the next form is read. So a macro defined by one form can be used by the next, and a definition is visible to every later form.

## Definitions

R7RS: [section 5.3](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.3).

`(define variable expression)` binds `variable` to the value of `expression`.

`(define (name formals ...) body ...)` defines a procedure; it is short for `(define name (lambda (formals ...) body ...))`. The formals can include a rest parameter, as with [`lambda`](./primitive-expressions.md#lambda): `(define (f a . rest) ...)`, or `(define (f . args) ...)`.

At top level, `define` creates the variable if it doesn't exist and otherwise works like `set!`, so a definition can be re-evaluated at the REPL to replace an earlier one. Defining a name that the program got from the system environment, such as `car`, gives the program its own variable and leaves the system's untouched (see [Environments and Evaluation](./environments.md#the-system-and-interaction-environments)).

`define` returns the name being defined, so `(define x 5)` at the REPL prints `x`. R7RS leaves the value unspecified.

### Internal definitions

Definitions at the start of a body (of a `lambda`, `define`, `let` and the other binding forms) are local to that body. They behave like `letrec*`: they can refer to each other, and are evaluated in order.

```scheme
(define (f)
  (define x 1)
  (define (g) (* x 10))
  (g))
(f)                        ; => 10
```

R7RS requires all of a body's definitions to come before its expressions, and so does s2: a definition after an expression is an error.

### Multiple-value definitions

`(define-values formals expression)` binds several variables from the values of one expression. See [Derived Expressions](./derived-expressions.md#define-values).

## Syntax definitions

`(define-syntax keyword transformer)` binds a macro, at top level or in a body. See [Macros](./macros.md).

## The REPL

R7RS: [section 5.7](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.7).

Run `s2` with no program to start the REPL. It reads a form, evaluates it in the interaction environment, and writes each of its values on a line of its own. An unspecified value, or no values, prints nothing:

```scheme
s2> (values 1 2)
1
2
s2> (car 1)
error: car: expected a pair, got 1
```

The `s2> ` prompt appears only when standard input is a terminal, so `s2 < file.scm` prints just the results.

An error that nothing handles is reported, and the REPL reads the next form. At the REPL, a definition replaces any earlier binding of the same name, including an imported one, so you can redefine things as you work. `(exit)` or end of input (Ctrl-D) leaves the REPL.

The REPL has no line editing or history of its own; running it under a wrapper such as `rlwrap s2` adds them. `(load "file.scm")` loads a file into it. See [s2 Extensions](./extensions.md) for the few tools that help at the REPL.
