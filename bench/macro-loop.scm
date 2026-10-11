(define-syntax inc! (syntax-rules () ((_ v) (set! v (+ v 1))) ((_ v n) (set! v (+ v n)))))
(define-syntax unless2 (syntax-rules () ((_ c body ...) (if c #f (begin body ...)))))
(define (count n) (let ((x 0) (i 0)) (let lp () (unless2 (= i n) (inc! x 2) (inc! i) (lp))) x))
(display (count 6000000))
