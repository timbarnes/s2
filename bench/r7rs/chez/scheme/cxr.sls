;;; (scheme cxr) for Chez Scheme: Chez's own
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme cxr)
  (export
    caaar caadr cadar caddr cdaar cdadr cddar cdddr caaaar caaadr caadar
    caaddr cadaar cadadr caddar cadddr cdaaar cdaadr cdadar cdaddr
    cddaar cddadr cdddar cddddr)
  (import (chezscheme))
)
