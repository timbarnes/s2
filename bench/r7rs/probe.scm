;;; probe.scm: what a Scheme has of the numeric tower and Unicode, and its
;;; R7RS (features). Run as an R7RS program on each system (plan/07).
(import (scheme base) (scheme write) (scheme inexact) (scheme char))
(define (try label thunk)
  (display label) (display ": ")
  (write (guard (e (#t 'error)) (thunk)))
  (newline))
(try "bignum" (lambda () (expt 2 100)))
(try "ratio" (lambda () (/ 1 3)))
(try "complex" (lambda () (sqrt -4)))
(try "unicode" (lambda () (list (string-length "λμ") (char-upcase #\λ))))
(try "float" (lambda () (list 0.1 (/ 1. 3))))
(try "features" (lambda () (features)))
