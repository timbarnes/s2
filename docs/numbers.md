# Numbers

R7RS: [section 6.2, Numbers](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.2).

s2 implements the R7RS numeric tower without complex numbers. A number is one of:

* an **exact integer** of any size (`42`, `-12345678901234567890`)
* an **exact rational** that isn't an integer, always in lowest terms (`1/2`, `-7/3`)
* an **inexact real**, a 64-bit flonum (`2.0`, `1.0e+21`, `+inf.0`, `-inf.0`, `+nan.0`)

Exact arithmetic stays exact: `(/ 1 2)` is `1/2`, and a whole result comes back as an integer (`(/ 6 3)` is `2`). Any inexact operand makes the result inexact: `(+ 1/2 0.5)` is `1.0`. Exact division by exact zero is an error; inexact division follows IEEE 754 (`(/ 1.0 0)` is `+inf.0`).

Complex number syntax such as `1+2i` is rejected by the reader with "complex numbers are not supported", and `string->number` returns `#f` for it. `sqrt` and `expt` raise an error rather than return a complex result, and `log`, `asin` and `acos` return `+nan.0` where the result would be complex.

All of these procedures are built in (written in Rust). Two-argument `+`, `-`, `=`, `<`, `>`, `<=` and `>=` compile to single VM instructions with a fast path for fixnums.

## Number syntax

The reader and `string->number` accept R7RS number syntax: an optional radix prefix (`#x`, `#b`, `#o`, `#d`), an optional exactness prefix (`#e`, `#i`), integers, ratios (`1/3`), decimals with exponents (`1.5e-3`, radix 10 only), and `+inf.0`, `-inf.0`, `+nan.0`. So `#xff` is `255`, `#e1.5` is `3/2`, and `#i1/4` is `0.25`.

## Type predicates

* `(number? obj)`, `(complex? obj)`, `(real? obj)`: `#t` for any number.
* `(rational? obj)`: `#t` for exact numbers and finite flonums.
* `(integer? obj)`: `#t` for exact integers and integral flonums (`(integer? 2.0)` is `#t`).
* `(exact? z)`, `(inexact? z)`: the number's exactness. `z` must be a number.
* `(exact-integer? obj)`: `#t` only for exact integers.
* `(nan? z)`, `(infinite? z)`, `(finite? z)`.

## Comparison

`(= z1 z2 ...)`, `(< x1 x2 ...)`, `(> x1 x2 ...)`, `(<= x1 x2 ...)` and `(>= x1 x2 ...)` take two or more arguments and test that every adjacent pair is related. Exact and inexact numbers compare by exact value, so the comparisons are transitive: `(= 9007199254740993 9007199254740992.0)` is `#f`. Any comparison involving a NaN is `#f`.

`(zero? z)`, `(positive? x)`, `(negative? x)`, `(odd? n)` and `(even? n)` behave as their names say. Zero is neither positive nor negative. `odd?` and `even?` accept integral flonums.

`(max x1 x2 ...)` and `(min x1 x2 ...)` return the largest and smallest argument. If any argument is inexact the result is inexact: `(max 3 2.5)` is `3.0`.

## Arithmetic

* `(+ z ...)`, `(* z ...)`: sum and product (`0` and `1` with no arguments).
* `(- z1 z2 ...)`, `(/ z1 z2 ...)`: difference and quotient. With one argument, the negation and the reciprocal.
* `(abs x)`, `(square z)`.
* `(numerator q)`, `(denominator q)`: in lowest terms. For a flonum they describe its exact value and are returned inexact: `(denominator 0.5)` is `2.0`.

## Integer division

Each comes in a floor version (quotient rounded toward negative infinity, remainder with the sign of the divisor) and a truncate version (quotient rounded toward zero, remainder with the sign of the dividend). Arguments must be integers, exact or integral flonums. The result is inexact if either argument is.

* `(floor/ n1 n2)`, `(truncate/ n1 n2)`: two values, the quotient and the remainder.
* `(floor-quotient n1 n2)`, `(floor-remainder n1 n2)`, `(truncate-quotient n1 n2)`, `(truncate-remainder n1 n2)`.
* `quotient`, `remainder` and `modulo` are the older names for `truncate-quotient`, `truncate-remainder` and `floor-remainder`: `(modulo -7 2)` is `1`, `(remainder -7 2)` is `-1`.
* `(gcd n ...)`, `(lcm n ...)`: non-negative. With no arguments they return `0` and `1`.

## Rounding

`(floor x)`, `(ceiling x)`, `(truncate x)` and `(round x)` return an integer of the same exactness as `x`: `(floor 2.5)` is `2.0`, `(floor 5/2)` is `2`. `round` rounds halves to even: `(round 2.5)` is `2.0`, `(round 7/2)` is `4`.

`(rationalize x y)` returns the simplest rational that differs from `x` by no more than `y`: `(rationalize 3/10 1/10)` is `1/3`, and `(rationalize .3 1/10)` is the inexact `0.3333333333333333`.

## Exactness

`(exact z)` returns the exact value of `z`. For a flonum that is the exact binary fraction it holds: `(exact 0.5)` is `1/2`, and `(exact 0.1)` is `3602879701896397/36028797018963968`. Infinities and NaNs have no exact value. `(inexact z)` returns the nearest flonum. The older names `inexact->exact` and `exact->inexact` are also defined.

## Powers, roots and transcendental functions

* `(sqrt z)`: exact when `z` is an exact perfect square, of an integer or a rational (`(sqrt 4/9)` is `2/3`), otherwise inexact. Negative arguments are an error.
* `(exact-integer-sqrt k)`: two values `s` and `r` with `k = s*s + r`.
* `(expt z1 z2)`: exact when `z1` is exact and `z2` is an exact integer (`(expt 2 -2)` is `1/4`, `(expt 2 100)` is exact). Otherwise inexact.
* `(exp z)`, `(log z)`, `(log z base)`, `(sin z)`, `(cos z)`, `(tan z)`, `(asin z)`, `(acos z)`, `(atan z)`, `(atan y x)`: always inexact.

## Conversion to and from strings

* `(number->string z [radix])`: the external representation of `z` in radix 2, 8, 10 (the default) or 16. Rationals print as `n/d`. Flonums always show a decimal point, and an exponent when very large or small (`2.0`, `1.0e+21`, `5.0e-324`); they support only radix 10.
* `(string->number string [radix])`: the number `string` represents in R7RS number syntax, or `#f` if it isn't one. Surrounding whitespace is not allowed. A radix prefix in the string overrides `radix`.
