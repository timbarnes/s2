# Environments and Evaluation

R7RS: [section 6.12, Environments and evaluation](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.12).

An environment is a set of top-level bindings that an expression can be evaluated in. Libraries, `import` and `cond-expand` are described in [Libraries](./libraries.md).

## The system and interaction environments

s2's built-in procedures and special forms, and everything `scheme/core.scm` defines, live in the *system environment*. Programs, the REPL and loaded files run in the *interaction environment*, which imports every system binding. An imported name shares the system's variable, so:

* `(define car ...)` at the REPL makes a new `car` for the interaction environment only. Procedures defined in `core.scm`, and code compiled before the `define`, keep using the system's `car`.
* `(set! car ...)` is an error: an imported variable can only be assigned by its owner. Names the program defines itself can be `set!` as usual.

## `environment`

`(environment import-set ...)`

Returns a new environment containing exactly the bindings of the [import sets](./libraries.md#import). It is immutable: `define`, `set!` and `import` at its top level are errors (local definitions inside expressions are fine).

```scheme
(eval '(+ 1 2) (environment '(scheme base)))              ; => 3
(eval '(char-upcase #\a) (environment '(scheme base)))   ; error: char-upcase is unbound
```

## `scheme-report-environment` and `null-environment`

These are R5RS procedures that R7RS keeps in `(scheme r5rs)` for older code. `(scheme-report-environment 5)` returns an immutable environment containing the bindings of `(scheme r5rs)`, and `(null-environment 5)` one with only its syntactic keywords (`if`, `define`, `let`, ...). 5 is the only version accepted. New code should use `environment`.

## `interaction-environment`

`(interaction-environment)`

Returns the environment the REPL and loaded files run in: everything the system environment holds, imported, and the program's own top-level definitions. Environments print as `#<environment>`, and `environment?` (an s2 extension) tests for them.

## `eval`

`(eval expr environment)`

Evaluates `expr`, a datum, in `environment`, and returns its value. Definitions in `expr` bind in that environment. The caller's local variables are not visible:

```scheme
(eval '(* 7 3) (interaction-environment))       ; => 21
(let ((x 1)) (eval 'x (interaction-environment))) ; error: x is unbound
```

`eval` compiles `expr` and then calls the result in its own place, so a call to `eval` in tail position is a tail call.

## `load`

`(load filename [environment])`

Reads the forms in `filename` and evaluates them in turn, in `environment` or by default the interaction environment, and returns when the file is finished. See [System Interface](./system-interface.md#load).
