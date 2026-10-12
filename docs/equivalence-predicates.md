# Equivalence Predicates

R7RS: [section 6.1, Equivalence predicates](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.1).

Scheme has three ways to ask whether two objects are the same, from the strictest to the most lenient. The searching procedures in [Pairs and Lists](./pairs-and-lists.md#searching) come in matching sets: `memq`/`memv`/`member` and `assq`/`assv`/`assoc`.

## `eq?`

`(eq? obj1 obj2)`

`#t` if `obj1` and `obj2` are the same object. Symbols with the same name, the empty list, booleans and characters are always `eq?`. In s2, small exact integers (fixnums) and flonums that are `eqv?` are also `eq?`, but bignums and ratios are not: `(eq? (expt 10 20) (expt 10 20))` is `#f`. R7RS doesn't promise either, so use `eqv?` for numbers and characters.

## `eqv?`

`(eqv? obj1 obj2)`

Like `eq?`, but also `#t` for numbers with the same exactness and value, and for equal characters. So `(eqv? 2/3 2/3)` is `#t`, while `(eqv? 2 2.0)` and `(eqv? 0.0 -0.0)` are `#f`. Two strings, pairs, vectors, bytevectors, records or procedures are `eqv?` only if they are the same object: `(eqv? "" "")` is `#f`.

## `equal?`

`(equal? obj1 obj2)`

Compares structure: `#t` if the objects are `eqv?`, or are pairs, vectors, strings or bytevectors whose contents are `equal?`. It terminates on circular structures. Records are compared with `eqv?`, so two records with the same fields are `equal?` only if they are the same record.

```scheme
(equal? '(1 #(2 "three")) (list 1 (vector 2 "three")))   ; => #t
(equal? 2 2.0)                                           ; => #f
```

For numeric equality across exactness, use `=`; for case-insensitive comparison of strings and characters, `string-ci=?` and `char-ci=?`.
