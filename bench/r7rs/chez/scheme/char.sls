;;; (scheme char) for Chez Scheme: Chez's, and digit-value
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme char)
  (export
    char-alphabetic? char-ci<=? char-ci<? char-ci=? char-ci>=? char-ci>?
    char-downcase char-foldcase char-lower-case? char-numeric? char-upcase char-upper-case? char-whitespace? digit-value string-ci<=?
    string-ci<? string-ci=? string-ci>=? string-ci>? string-downcase
    string-foldcase string-upcase)
  (import (chezscheme))
  ;; ASCII digits only.
  (define (digit-value c) (and (char<=? #\0 c #\9) (- (char->integer c) 48)))
)
