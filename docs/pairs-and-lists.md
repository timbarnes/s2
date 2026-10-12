# Pairs and Lists

R7RS: [section 6.4, Pairs and lists](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.4).

A pair holds two values, its car and its cdr, and is written `(a . b)`. A list is a chain of pairs whose last cdr is the empty list `()`, so `(1 2 3)` is `(1 . (2 . (3 . ())))`. A chain that ends in anything else is an improper list, `(1 2 . 3)`. Pairs are mutable, so a list can be made circular; `write` prints circular lists with [datum labels](./lexical-syntax.md#datum-labels).

Procedures that take a list check that they got one: `length`, for example, raises an error for an improper or circular list rather than looping.

## Pairs

* `(pair? obj)`, `(cons obj1 obj2)`, `(car pair)`, `(cdr pair)`.
* `(set-car! pair obj)`, `(set-cdr! pair obj)`.
* `caar`, `cadr`, `cdar`, `cddr`, and every combination up to four levels deep (`caddr`, `cdddr`, `cadddr`, `cddddr`, ...): `(cadr x)` is `(car (cdr x))`. The three- and four-level forms are in `(scheme cxr)`.

## Lists

* `(null? obj)`: `#t` for the empty list.
* `(list? obj)`: `#t` for a proper list (finite and ending in `()`), `#f` otherwise, including for a circular list.
* `(make-list k [fill])`, `(list obj ...)`.
* `(length list)`, `(append list ...)`, `(reverse list)`. `append` shares its last argument, which needn't be a list: `(append '(1) 2)` is `(1 . 2)`.
* `(list-tail list k)`: the list without its first `k` elements. `(list-ref list k)`: element `k`, counting from 0. `(list-set! list k obj)`: store into element `k`.
* `(list-copy obj)`: a copy of the pairs of a list, proper or not. Anything else is returned as is.

## Searching

* `(memq obj list)`, `(memv obj list)`, `(member obj list [compare])`: the first sublist of `list` whose car is `obj`, or `#f`. They compare with `eq?`, `eqv?` and `equal?` respectively, or with `compare`.
* `(assq obj alist)`, `(assv obj alist)`, `(assoc obj alist [compare])`: the first pair in the association list `alist` whose car is `obj`, or `#f`, compared the same way.

```scheme
(member 2.0 '(1 2 3) =)              ; => (2 3)
(assv 2 '((1 one) (2 two)))          ; => (2 two)
```

## Mapping

`(map proc list1 list2 ...)` and `(for-each proc list1 list2 ...)` are described in [Control Features](./control-features.md#map-and-for-each).
