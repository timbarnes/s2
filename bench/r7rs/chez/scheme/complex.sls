;;; (scheme complex) for Chez Scheme: Chez's own
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme complex)
  (export
    angle imag-part magnitude make-polar make-rectangular real-part)
  (import (chezscheme))
)
