;;; The core AST as tagged vectors (plan/08, C2), as src/compile/data.rs
;;; makes them:
;;;
;;;   #(const v)              #(local-ref id)          #(local-set id e)
;;;   #(global-ref cell)      #(global-set cell e)     #(global-define cell e)
;;;   #(if test then else)    #(seq (e ...))           #(call f (arg ...))
;;;   #(lambda name (var ...) rest body)   name a string, "" if none; rest a var or #f
;;;   #(let ((var . init) ...) body)       #(letrec ((var . init) ...) body)
;;;   #(var id name)          a binding occurrence: its number, and its name
;;;
;;; Only this file indexes the vectors.

(define (node-tag n) (vector-ref n 0))

(define (var-id v) (vector-ref v 1))
(define (var-name v) (vector-ref v 2))

(define (const-value n) (vector-ref n 1))
(define (local-ref-id n) (vector-ref n 1))
(define (local-set-id n) (vector-ref n 1))
(define (local-set-value n) (vector-ref n 2))
;; global-ref, global-set and global-define
(define (global-cell n) (vector-ref n 1))
(define (global-value n) (vector-ref n 2))
(define (if-test n) (vector-ref n 1))
(define (if-then n) (vector-ref n 2))
(define (if-else n) (vector-ref n 3))
(define (seq-body n) (vector-ref n 1))
(define (call-fn n) (vector-ref n 1))
(define (call-args n) (vector-ref n 2))
(define (lambda-name n) (vector-ref n 1))
(define (lambda-params n) (vector-ref n 2))
(define (lambda-rest n) (vector-ref n 3))
(define (lambda-body n) (vector-ref n 4))
;; let and letrec
(define (let-bindings n) (vector-ref n 1))
(define (let-body n) (vector-ref n 2))
(define (binding-var b) (car b))
(define (binding-init b) (cdr b))

;; The parameters, then the rest parameter if there is one.
(define (lambda-vars l)
  (if (lambda-rest l)
      (append (lambda-params l) (list (lambda-rest l)))
      (lambda-params l)))

;; An identifier's name as a string, as the Rust compiler prints it.
(define (identifier-name id)
  (let ((port (open-output-string)))
    (write id port)
    (get-output-string port)))

;;; List helpers that R7RS-small lacks.

(define (every? ok? items)
  (or (null? items) (and (ok? (car items)) (every? ok? (cdr items)))))

(define (last-pair items)
  (if (pair? (cdr items)) (last-pair (cdr items)) items))

;; The position of the first element of `items` that is eqv? to `x`, or #f.
(define (index-of x items)
  (let loop ((items items) (i 0))
    (cond ((null? items) #f)
          ((eqv? (car items) x) i)
          (else (loop (cdr items) (+ i 1))))))

;;; A buffer: a vector that grows as values are added to its end.

(define-record-type buffer
  (make-buffer-from items count)
  buffer?
  (items buffer-items set-buffer-items!)
  (count buffer-count set-buffer-count!))

(define (make-buffer) (make-buffer-from (make-vector 8 #f) 0))

;; Add `x` at the end; its index.
(define (buffer-push! b x)
  (let ((n (buffer-count b)) (items (buffer-items b)))
    (if (= n (vector-length items))
        (let ((bigger (make-vector (* 2 n) #f)))
          (vector-copy! bigger 0 items)
          (set-buffer-items! b bigger)))
    (vector-set! (buffer-items b) n x)
    (set-buffer-count! b (+ n 1))
    n))

(define (buffer-ref b i) (vector-ref (buffer-items b) i))

;; Set element `i`, first growing the buffer with `fill` up to it.
(define (buffer-set! b i x fill)
  (let loop ()
    (if (<= (buffer-count b) i)
        (begin (buffer-push! b fill) (loop))))
  (vector-set! (buffer-items b) i x))

;; The index of the first element `same?` to `x`, or #f.
(define (buffer-index b x same?)
  (let loop ((i 0))
    (cond ((= i (buffer-count b)) #f)
          ((same? (buffer-ref b i) x) i)
          (else (loop (+ i 1))))))

(define (buffer->vector b) (vector-copy (buffer-items b) 0 (buffer-count b)))
