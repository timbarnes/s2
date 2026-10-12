;;; (scheme inexact) for Chez Scheme: Chez's own
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme inexact)
  (export
    acos asin atan cos exp finite? infinite? log nan? sin sqrt tan)
  (import (chezscheme))
)
