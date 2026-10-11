;; Internal definitions are letrec*. odd? is referenced (by even?'s
;; closure) before its init has run, so it is boxed; even? is only
;; referenced after its init, so it is not.
(define (parity n)
  (define (even? n) (if (= n 0) #t (odd? (- n 1))))
  (define (odd? n) (if (= n 0) #f (even? (- n 1))))
  (even? n))
;; A named let: the loop variable refers to itself, so it is boxed.
(define (count-to n) (let loop ((i 0)) (if (= i n) i (loop (+ i 1)))))
