# Primitive Expressions

The forms every other expression is built from. R7RS: [section 4.1, Primitive expression types](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-6.html#TAG:__tex2page_sec_4.1). All of them are core forms of the compiler.

## Variable references

An identifier evaluates to the value of the variable it names. Referring to a variable that has no binding raises an error: `unbound variable: name`. A reference to a global variable is checked when it runs, so a procedure can refer to a variable defined later.

## `quote`

`(quote datum)`, or `'datum`

Returns `datum` without evaluating it. `'(1 2 3)` is a list, `'a` is a symbol. Numbers, strings, characters, booleans, vectors and bytevectors evaluate to themselves, so they don't need quoting.

Quoted data are constants: s2 doesn't stop you modifying one with `set-car!` or `string-set!`, but R7RS makes that an error, and portable code should copy first (`list-copy`, `string-copy`).

## Procedure calls

`(operator operand ...)`

Evaluates the operator and the operands, then calls the operator's value with the operands' values. s2 evaluates them left to right, operator first; R7RS leaves the order unspecified, so portable code shouldn't depend on it. Calling something that isn't a procedure raises an error (`not a procedure: 5`), as does passing the wrong number of arguments.

## `lambda`

`(lambda formals body ...)`

Returns a procedure. When it is called, its parameters are bound to the arguments in a new environment that extends the one the `lambda` was evaluated in, and the body is evaluated there. `formals` is one of:

* `(a b c)`: exactly three arguments.
* `(a b . rest)`: at least two; `rest` gets a list of the others.
* `args`: any number, as a list.

The body may start with [internal definitions](./program-structure.md#internal-definitions). Its last expression is in tail position (see [Control Features](./control-features.md#proper-tail-calls)).

For procedures with optional arguments or several argument counts, see [`case-lambda`](./derived-expressions.md#case-lambda).

## `if`

`(if test consequent [alternate])`

Evaluates `test`. If its value is anything but `#f`, evaluates and returns `consequent`; otherwise `alternate`. Without an alternate, the result when `test` is false is unspecified. In s2 that is a value that prints as nothing (see [Input and Output](./input-and-output.md#how-objects-print)).

## `set!`

`(set! variable expression)`

Stores the value of `expression` in `variable`, which must already be bound. Assigning an unbound variable is an error, as is assigning a variable imported from a library (see [Environments and Evaluation](./environments.md#the-system-and-interaction-environments)). The result is unspecified.

## `include` and `include-ci`

`(include "file" ...)` reads the forms in the files and evaluates them in place of the `include`. See [Libraries](./libraries.md#include-and-include-ci).
