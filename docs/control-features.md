# Control Features

R7RS: [section 6.10, Control features](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.10), and [section 3.5, Proper tail recursion](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-5.html#TAG:__tex2page_sec_3.5).

## Proper tail calls

s2 is properly tail-recursive: a procedure call in tail position reuses the caller's continuation, so a loop written as a tail call runs in constant space however many times it repeats. The tail positions are the last expression of a `lambda` or `case-lambda` body; the branches of `if`, `cond` (including `=>`), `case`, `when` and `unless`; the last expression of `and`, `or` and `begin`; the bodies of every `let` form, named `let`, `let-values`, `let-syntax` and `letrec-syntax`; a `do` loop's result expressions; and macro uses in any of these. `apply` and `call/cc` call their procedure, `call-with-values` its consumer, and `eval` its compiled expression, in tail position.

Calls inside `guard`, `parameterize`, `dynamic-wind` and `with-exception-handler` bodies aren't tail calls, since those forms must do something after the body returns.

Recursion that isn't in tail position can go about 20 million calls deep for a simple procedure: the VM's value stack holds 64 million values, and the system only gives it memory as it is used. A program that goes deeper ends with `stack overflow`, which can't be caught, as handling it would need more stack.

`(%frame-depth)`, an internal procedure, returns the number of frames waiting for a return; `tests/scheme/tail_tests.scm` uses it to check every tail context.

## Procedures

* `(procedure? obj)`: `#t` for built-in procedures, closures, `case-lambda` procedures, parameter objects and continuations.
* `(apply proc arg1 ... args)`: calls `proc` with the arguments `arg1 ...` followed by the elements of the list `args`. `(apply + 1 2 '(3 4))` is `10`.

## `map` and `for-each`

`(map proc list1 list2 ...)` returns a list of the results of applying `proc` to corresponding elements of the lists. `(for-each proc list1 list2 ...)` does the same for effect, in order from first to last. Both stop at the end of the shortest list, so a circular list can be paired with a finite one. s2's `map` also applies `proc` in order, though R7RS doesn't require that.

`string-map`, `string-for-each`, `vector-map` and `vector-for-each` are the same for strings and vectors; see [Strings](./strings.md#iteration) and [Vectors](./vectors.md).

## Continuations

`(call-with-current-continuation proc)`, or `(call/cc proc)`

Calls `proc` with the current continuation, packaged as an escape procedure. Calling the escape procedure with a value, at any later time, returns that value from the `call/cc` again. Continuations are fully re-entrant: one can be called after the `call/cc` has returned, any number of times, as generators and backtracking need.

```scheme
(call/cc (lambda (k) (+ 1 (k 42))))     ; => 42
```

Calling a continuation runs the `before` and `after` thunks of any `dynamic-wind` extents it enters or leaves. A continuation captured during a top-level form covers the rest of that form only: invoking it later finishes that form, and evaluation carries on from the form that invoked it.

For exceptions, which are the usual way to escape on error, see [Exceptions](./exceptions.md).

## Multiple values

* `(values obj ...)`: returns its arguments as multiple values to its continuation.
* `(call-with-values producer consumer)`: calls `producer` with no arguments and passes the values it returns as arguments to `consumer`. `(call-with-values (lambda () (values 1 2)) +)` is `3`.

`let-values`, `let*-values` and `define-values` bind multiple values to variables; see [Derived Expressions](./derived-expressions.md#let-values-let-values).

## `dynamic-wind`

`(dynamic-wind before thunk after)`

Calls `before`, then `thunk`, then `after`, all with no arguments, and returns `thunk`'s values. If control leaves `thunk` by any means (a continuation, an exception, `exit`), `after` runs on the way out; if control re-enters it through a continuation, `before` runs again. `parameterize`, `guard` and the exception handlers are built on the same mechanism.

## Promises

`force`, `delay`, `delay-force` and `make-promise` are in [Derived Expressions](./derived-expressions.md#delayed-evaluation).
