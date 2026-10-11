;; Rest parameters: arity 1+ means one required argument plus a list.
(define (f a . more) (cons a more))
(define (g . all) all)
