# Booleans

R7RS: [section 6.3, Booleans](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.3).

`#t` / `#true` and `#f` / `#false`. Only `#f` counts as false in conditionals.

* `(boolean? obj)`.
* `(not obj)`: `#t` if `obj` is `#f`, otherwise `#f`.
* `(boolean=? b1 b2 b3 ...)`: `#t` if all are the same boolean.
