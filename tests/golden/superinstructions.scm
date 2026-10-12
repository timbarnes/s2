;; superinstructions: car and cdr of a local, a constant added or
;; subtracted, a comparison with a constant, and (if (not x) a b) compiled
;; as (if x b a). A boxed local, a large constant and eq? are left alone.
(define (walk l n)
  (if (not (null? l))
      (walk (cdr l) (+ 1 n))
      (if (>= n 100000) (car l) (- n 3))))
(define (boxed l)
  (set! l (cdr l))
  (car l))
(define (big n) (if (< n 100000) (+ n 100000000000) (eq? n 1)))
