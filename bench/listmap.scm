;; Allocation and map.
(define build (lambda (n acc) (if (= n 0) acc (build (- n 1) (cons n acc)))))
(define data (build 200 '()))
(define dbl (lambda (x) (* x 2)))
(define lp (lambda (i) (if (= i 0) 'done (begin (map dbl data) (lp (- i 1))))))
(display (lp 30000))
