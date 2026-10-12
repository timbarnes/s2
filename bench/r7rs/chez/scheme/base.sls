;;; (scheme base) for Chez Scheme: R7RS's names, Chez's where they mean the same
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS. cond-expand
;;; and include-ci are left out: Chez has neither, and no benchmark uses them.
(library (scheme base)
  (export
    * + - ... / < <= = => > >= _ abs and append apply assoc assq assv
    begin binary-port? boolean=? boolean? bytevector bytevector-append
    bytevector-copy bytevector-copy! bytevector-length bytevector-u8-ref
    bytevector-u8-set! bytevector? caar cadr call-with-current-continuation call-with-port call-with-values call/cc car case cdar
    cddr cdr ceiling char->integer char-ready? char<=? char<? char=?
    char>=? char>? char? close-input-port close-output-port close-port
    complex? cond cons current-error-port current-input-port
    current-output-port define define-record-type define-syntax define-values denominator do dynamic-wind else eof-object eof-object? eq?
    equal? eqv? error error-object-irritants error-object-message error-object? even? exact exact-integer-sqrt exact-integer? exact? expt
    features file-error? floor floor-quotient floor-remainder floor/
    flush-output-port for-each gcd get-output-bytevector get-output-string guard if import include inexact inexact? input-port-open? input-port? integer->char integer? lambda lcm length let
    let* let*-values let-syntax let-values letrec letrec* letrec-syntax
    list list->string list->vector list-copy list-ref list-set! list-tail list? make-bytevector make-list make-parameter make-string
    make-vector map max member memq memv min modulo negative? newline
    not null? number->string number? numerator odd? open-input-bytevector open-input-string open-output-bytevector open-output-string or output-port-open? output-port? pair? parameterize peek-char peek-u8 positive? procedure? quasiquote quote quotient raise
    raise-continuable rational? rationalize read-bytevector read-bytevector! read-char read-error? read-line read-string read-u8
    real? remainder reverse round set! set-car! set-cdr! square string
    string->list string->number string->symbol string->utf8
    string->vector string-append string-copy string-copy! string-fill!
    string-for-each string-length string-map string-ref string-set!
    string<=? string<? string=? string>=? string>? string? substring
    symbol->string symbol=? symbol? syntax-error syntax-rules textual-port? truncate truncate-quotient truncate-remainder truncate/
    u8-ready? unless unquote unquote-splicing utf8->string values vector
    vector->list vector->string vector-append vector-copy vector-copy!
    vector-fill! vector-for-each vector-length vector-map vector-ref
    vector-set! vector? when with-exception-handler write-bytevector
    write-char write-string write-u8 zero?)
  (import (except (chezscheme)
      assoc bytevector-copy bytevector-copy! define-record-type error member string->list string->utf8 string-copy string-copy! string-fill! utf8->string vector->list vector-copy vector-copy! vector-fill!)
    (prefix (only (chezscheme)
      assoc bytevector-copy bytevector-copy! define-record-type error member string->list string->utf8 string-copy string-copy! string-fill! utf8->string vector->list vector-copy vector-copy! vector-fill!)
      chez:))

  ;; R7RS's define-record-type, as Chez's R6RS one, so Chez optimises it.
  (define-syntax define-record-type
    (lambda (x)
      (syntax-case x ()
        ((_ type (ctor cfield ...) pred fieldspec ...)
         (let ((cfields #'(cfield ...)))
           (with-syntax (((make) (generate-temporaries #'(ctor)))
                         ((r6spec ...)
                          (map (lambda (s)
                                 (syntax-case s ()
                                   ((f a) #'(immutable f a))
                                   ((f a m) #'(mutable f a m))))
                               #'(fieldspec ...)))
                         ((arg ...)
                          (map (lambda (s)
                                 (syntax-case s ()
                                   ((f . _)
                                    (let ((c (memp (lambda (c) (eq? (syntax->datum c) (syntax->datum #'f))) cfields)))
                                      (if c (car c) #'#f)))))
                               #'(fieldspec ...))))
             #'(begin
                 (chez:define-record-type (type make pred) (fields r6spec ...))
                 (define (ctor cfield ...) (make arg ...)))))))))

  ;; (error message irritant ...), where Chez's takes who first.
  (define (error message . irritants)
    (if (string? message)
        (apply chez:error #f message irritants)
        (apply chez:error #f "~s" message irritants)))
  (define (error-object? x) (condition? x))
  (define (error-object-message e) (if (message-condition? e) (condition-message e) ""))
  (define (error-object-irritants e) (if (irritants-condition? e) (condition-irritants e) '()))
  (define (file-error? e) (i/o-error? e))
  (define (read-error? e) (lexical-violation? e))

  (define (exact-integer? x) (and (integer? x) (exact? x)))
  (define (square x) (* x x))
  (define (floor-quotient n d)
    (let ((q (quotient n d)))
      (if (and (not (zero? (remainder n d))) (not (eq? (negative? n) (negative? d))))
          (- q 1)
          q)))
  (define (floor-remainder n d) (modulo n d))
  (define (floor/ n d) (let ((q (floor-quotient n d))) (values q (- n (* q d)))))
  (define (truncate-quotient n d) (quotient n d))
  (define (truncate-remainder n d) (remainder n d))
  (define (truncate/ n d) (values (quotient n d) (remainder n d)))
  (define (features) '(r7rs chez))

  (define (list-set! l k v) (set-car! (list-tail l k) v))
  (define member
    (case-lambda
      ((x l) (chez:member x l))
      ((x l same?) (let loop ((l l)) (cond ((null? l) #f) ((same? x (car l)) l) (else (loop (cdr l))))))))
  (define assoc
    (case-lambda
      ((x l) (chez:assoc x l))
      ((x l same?) (let loop ((l l)) (cond ((null? l) #f) ((same? x (caar l)) (car l)) (else (loop (cdr l))))))))

  ;; R7RS's optional start and end arguments.
  (define string->list
    (case-lambda
      ((s) (chez:string->list s))
      ((s start) (chez:string->list (substring s start (string-length s))))
      ((s start end) (chez:string->list (substring s start end)))))
  (define string-copy
    (case-lambda
      ((s) (chez:string-copy s))
      ((s start) (substring s start (string-length s)))
      ((s start end) (substring s start end))))
  (define string-copy!
    (case-lambda
      ((to at from) (chez:string-copy! from 0 to at (string-length from)))
      ((to at from start) (chez:string-copy! from start to at (- (string-length from) start)))
      ((to at from start end) (chez:string-copy! from start to at (- end start)))))
  (define string-fill!
    (case-lambda
      ((s c) (chez:string-fill! s c))
      ((s c start) (string-fill! s c start (string-length s)))
      ((s c start end) (do ((i start (+ i 1))) ((= i end)) (string-set! s i c)))))
  (define vector->list
    (case-lambda
      ((v) (chez:vector->list v))
      ((v start) (vector->list v start (vector-length v)))
      ((v start end)
       (let loop ((i (- end 1)) (l '()))
         (if (< i start) l (loop (- i 1) (cons (vector-ref v i) l)))))))
  (define vector-copy
    (case-lambda
      ((v) (chez:vector-copy v))
      ((v start) (vector-copy v start (vector-length v)))
      ((v start end)
       (let ((r (make-vector (- end start))))
         (do ((i start (+ i 1))) ((= i end) r) (vector-set! r (- i start) (vector-ref v i)))))))
  (define vector-copy!
    (case-lambda
      ((to at from) (vector-copy! to at from 0 (vector-length from)))
      ((to at from start) (vector-copy! to at from start (vector-length from)))
      ((to at from start end)
       (if (<= at start)
           (do ((i start (+ i 1)) (j at (+ j 1))) ((= i end)) (vector-set! to j (vector-ref from i)))
           (do ((i (- end 1) (- i 1)) (j (+ at (- end start 1)) (- j 1))) ((< i start)) (vector-set! to j (vector-ref from i)))))))
  (define vector-fill!
    (case-lambda
      ((v x) (chez:vector-fill! v x))
      ((v x start) (vector-fill! v x start (vector-length v)))
      ((v x start end) (do ((i start (+ i 1))) ((= i end)) (vector-set! v i x)))))
  (define (vector->string v . range) (list->string (apply vector->list v range)))
  (define (string->vector s . range) (list->vector (apply string->list s range)))
  (define (string-map f s . ss) (list->string (apply map f (chez:string->list s) (map chez:string->list ss))))
  (define bytevector-copy
    (case-lambda
      ((b) (chez:bytevector-copy b))
      ((b start) (bytevector-copy b start (bytevector-length b)))
      ((b start end) (let ((r (make-bytevector (- end start)))) (chez:bytevector-copy! b start r 0 (- end start)) r))))
  (define bytevector-copy!
    (case-lambda
      ((to at from) (chez:bytevector-copy! from 0 to at (bytevector-length from)))
      ((to at from start) (chez:bytevector-copy! from start to at (- (bytevector-length from) start)))
      ((to at from start end) (chez:bytevector-copy! from start to at (- end start)))))
  (define (bytevector-append . bs)
    (let ((r (make-bytevector (apply + (map bytevector-length bs)))))
      (let loop ((bs bs) (at 0))
        (if (null? bs)
            r
            (let ((n (bytevector-length (car bs))))
              (chez:bytevector-copy! (car bs) 0 r at n)
              (loop (cdr bs) (+ at n)))))))
  (define utf8->string
    (case-lambda
      ((b) (chez:utf8->string b))
      ((b start) (chez:utf8->string (bytevector-copy b start)))
      ((b start end) (chez:utf8->string (bytevector-copy b start end)))))
  (define string->utf8
    (case-lambda
      ((s) (chez:string->utf8 s))
      ((s start) (chez:string->utf8 (substring s start (string-length s))))
      ((s start end) (chez:string->utf8 (substring s start end)))))

  ;; Ports.
  (define (in p) (if (null? p) (current-input-port) (car p)))
  (define (out p) (if (null? p) (current-output-port) (car p)))
  (define (read-line . p) (get-line (in p)))
  (define (read-string k . p) (get-string-n (in p) k))
  (define write-string
    (case-lambda
      ((s) (put-string (current-output-port) s))
      ((s p) (put-string p s))
      ((s p start) (put-string p s start (- (string-length s) start)))
      ((s p start end) (put-string p s start (- end start)))))
  (define (read-u8 . p) (get-u8 (in p)))
  (define (peek-u8 . p) (lookahead-u8 (in p)))
  (define (u8-ready? . p) #t)
  (define (write-u8 b . p) (put-u8 (out p) b))
  (define (read-bytevector k . p) (get-bytevector-n (in p) k))
  (define read-bytevector!
    (case-lambda
      ((b) (read-bytevector! b (current-input-port)))
      ((b p) (read-bytevector! b p 0 (bytevector-length b)))
      ((b p start) (read-bytevector! b p start (bytevector-length b)))
      ((b p start end) (get-bytevector-n! p b start (- end start)))))
  (define write-bytevector
    (case-lambda
      ((b) (put-bytevector (current-output-port) b))
      ((b p) (put-bytevector p b))
      ((b p start) (put-bytevector p b start (- (bytevector-length b) start)))
      ((b p start end) (put-bytevector p b start (- end start)))))
  (define (open-input-bytevector b) (open-bytevector-input-port b))
  (define extractors (make-weak-eq-hashtable))
  (define (open-output-bytevector)
    (let-values (((p get) (open-bytevector-output-port)))
      (hashtable-set! extractors p get)
      p))
  (define (get-output-bytevector p) ((hashtable-ref extractors p #f)))
  (define (input-port-open? p) (not (port-closed? p)))
  (define (output-port-open? p) (not (port-closed? p)))
)
