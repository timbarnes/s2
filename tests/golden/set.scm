;; Boxing only where needed. `n` is captured and assigned: boxed.
;; `x` is assigned but not captured: a plain slot (LocalSet).
;; `y` is captured but never assigned: copied by value, no box.
(define (make-counter) (let ((n 0)) (lambda () (set! n (+ n 1)) n)))
(define (f x y) (set! x (+ x 1)) (cons x (lambda () y)))
(define g 0)
(set! g 1)
