;; Deep non-tail recursion: a deep stack, and the GC with it.
(define count (lambda (n) (if (= n 0) 0 (+ 1 (count (- n 1))))))
(define (repeat k) (if (= k 0) 'done (begin (count 1000000) (repeat (- k 1)))))
(display (repeat 5))
