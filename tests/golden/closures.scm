;; Flat closures: the free variables' values are pushed, then MakeClosure
;; copies them into the closure. Inside, they are FreeRef.
(define (compose f g) (lambda (x) (f (g x))))
;; n is free two levels down: the middle lambda must capture it too, so the
;; innermost one can copy it from there.
(define (adder3 n) (lambda (a) (lambda (b) (+ n a b))))
