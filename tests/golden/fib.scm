;; fib: compare with the worked example in plan/03 §6. The inner calls are
;; Call; the outer (+ ...) is in tail position, so it is TailCall.
(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))
