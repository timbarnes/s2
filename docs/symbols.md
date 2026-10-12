# Symbols

R7RS: [section 6.5, Symbols](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.5).

Symbols are case sensitive. A symbol whose name wouldn't read back as a symbol is written with bars: `|hello world|`.

* `(symbol? obj)`.
* `(symbol=? s1 s2 s3 ...)`: `#t` if all are the same symbol.
* `(symbol->string sym)`: a new string holding the name.
* `(string->symbol s)`: the symbol with that name.
