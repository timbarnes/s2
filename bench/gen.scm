;; A generator: call/cc re-entry at shallow depth (D8's copying).
(define (make-gen lst)
  (define return #f)
  (define resume #f)
  (lambda ()
    (call/cc
     (lambda (r)
       (set! return r)
       (if resume
           (resume #f)
           (begin
             (let loop ((l lst))
               (if (pair? l)
                   (begin
                     (call/cc (lambda (c) (set! resume c) (return (car l))))
                     (loop (cdr l)))))
             (return 'done)))))))
(define (iota n) (let loop ((i (- n 1)) (acc '())) (if (< i 0) acc (loop (- i 1) (cons i acc)))))
(define g (make-gen (iota 300000)))
(define (sum acc) (let ((x (g))) (if (eq? x 'done) acc (sum (+ acc x)))))
(display (sum 0))
