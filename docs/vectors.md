# Vectors

R7RS: [section 6.8, Vectors](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.8).

Vectors are written `#(a b c)`. Procedures with optional `start` and `end` work on the elements from index `start` (default 0) up to, not including, `end` (default the length).

* `(vector? obj)`, `(make-vector k [fill])`, `(vector obj ...)`, `(vector-length v)`.
* `(vector-ref v k)`, `(vector-set! v k obj)`.
* `(vector-copy v [start [end]])`, `(vector-append v ...)`.
* `(vector-copy! to at from [start [end]])`: copies elements into `to` starting at index `at`; correct even when `to` and `from` are the same vector.
* `(vector-fill! v fill [start [end]])`.
* `(vector->list v [start [end]])`, `(list->vector list)`, `(vector->string v [start [end]])`, `(string->vector s [start [end]])`.
* `(vector-map proc v1 v2 ...)` returns a vector of `proc`'s results on corresponding elements; `(vector-for-each proc v1 v2 ...)` calls `proc` for effect. Both stop at the shortest vector.
