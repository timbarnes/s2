;;; (scheme write) for Chez Scheme: Chez's write for all three
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme write)
  (export
    display write write-shared write-simple)
  (import (chezscheme))
  (define write-shared write)
  (define write-simple write)
)
