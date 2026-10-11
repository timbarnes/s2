;;; core.scm: R7RS derived syntax (§7.3) as syntax-rules macros, and the
;;; standard procedures that are written in Scheme: the ones that call
;;; procedures they are given (map, for-each, member with a predicate…),
;;; which a builtin can't do (src/builtins/mod.rs).
;;;
;;; Compiled into s2 and loaded into every session. The core forms the
;;; expander implements itself are quote, quasiquote, lambda, define, set!,
;;; if, begin, let (and named let), letrec, letrec*, define-syntax,
;;; let-syntax, letrec-syntax and syntax-rules (src/compile/expand.rs).
;;;
;;; Identifiers these templates introduce, such as `if`, `let` and `memv`,
;;; mean what they mean here, at top level, whatever the code that uses
;;; the macro binds: that is hygiene.
;;;
;;; Not yet: let-values, let*-values and define-values need
;;; call-with-values (P7), as do guard, parameterize and the promises, and
;;; with-input-from-file and with-output-to-file need parameterize.

(define-syntax and
  (syntax-rules ()
    ((_) #t)
    ((_ test) test)
    ((_ test1 test2 ...) (if test1 (and test2 ...) #f))))

(define-syntax or
  (syntax-rules ()
    ((_) #f)
    ((_ test) test)
    ((_ test1 test2 ...) (let ((x test1)) (if x x (or test2 ...))))))

(define-syntax when
  (syntax-rules ()
    ((_ test result1 result2 ...) (if test (begin result1 result2 ...)))))

(define-syntax unless
  (syntax-rules ()
    ((_ test result1 result2 ...) (if test (if #f #f) (begin result1 result2 ...)))))

(define-syntax let*
  (syntax-rules ()
    ((_ () body1 body2 ...) (let () body1 body2 ...))
    ((_ ((name1 val1) (name2 val2) ...) body1 body2 ...)
     (let ((name1 val1)) (let* ((name2 val2) ...) body1 body2 ...)))))

(define-syntax cond
  (syntax-rules (else =>)
    ((_ (else result1 result2 ...)) (begin result1 result2 ...))
    ((_ (test => result)) (let ((temp test)) (if temp (result temp))))
    ((_ (test => result) clause1 clause2 ...)
     (let ((temp test)) (if temp (result temp) (cond clause1 clause2 ...))))
    ((_ (test)) test)
    ((_ (test) clause1 clause2 ...) (let ((temp test)) (if temp temp (cond clause1 clause2 ...))))
    ((_ (test result1 result2 ...)) (if test (begin result1 result2 ...)))
    ((_ (test result1 result2 ...) clause1 clause2 ...)
     (if test (begin result1 result2 ...) (cond clause1 clause2 ...)))))

(define-syntax case
  (syntax-rules (else =>)
    ((_ (key ...) clauses ...)
     (let ((atom-key (key ...))) (case atom-key clauses ...)))
    ((_ key (else => result)) (result key))
    ((_ key (else result1 result2 ...)) (begin result1 result2 ...))
    ((_ key ((atoms ...) => result)) (if (memv key '(atoms ...)) (result key)))
    ((_ key ((atoms ...) result1 result2 ...)) (if (memv key '(atoms ...)) (begin result1 result2 ...)))
    ((_ key ((atoms ...) => result) clause clauses ...)
     (if (memv key '(atoms ...)) (result key) (case key clause clauses ...)))
    ((_ key ((atoms ...) result1 result2 ...) clause clauses ...)
     (if (memv key '(atoms ...)) (begin result1 result2 ...) (case key clause clauses ...)))))

(define-syntax do
  (syntax-rules ()
    ((_ ((var init step ...) ...) (test expr ...) command ...)
     (letrec ((loop (lambda (var ...)
                      (if test
                          (begin (if #f #f) expr ...)
                          (begin command ... (loop (do "step" var step ...) ...))))))
       (loop init ...)))
    ((_ "step" x) x)
    ((_ "step" x y) y)))
;;; ---------------------------------------------------------------------
;;; Records (R7RS §5.5), over the primitives in src/builtins/record.rs.
;;; Forked from s1-core.scm.

(define-syntax define-record-type
  (syntax-rules ()
    ((_ type (constructor field ...) predicate fieldspec ...)
     (begin
       (define type (%make-record-type 'type '(fieldspec ...)))
       (define (constructor field ...) (%record-make type '(field ...) (list field ...)))
       (define (predicate obj) (%record? obj type))
       (%define-record-field type fieldspec) ...))))

;; One field spec: (field), (field accessor) or (field accessor modifier).
(define-syntax %define-record-field
  (syntax-rules ()
    ((_ type (field)) 'field)
    ((_ type (field accessor))
     (define (accessor obj) (%record-get obj type 'field)))
    ((_ type (field accessor modifier))
     (begin
       (define (accessor obj) (%record-get obj type 'field))
       (define (modifier obj value) (%record-set! obj type 'field value))))))

;;; ---------------------------------------------------------------------
;;; Lists (R7RS §6.4), forked from s1-core.scm. map and for-each stop at
;;; the shortest list.

(define (%any-null? lists)
  (cond ((null? lists) #f)
        ((null? (car lists)) #t)
        (else (%any-null? (cdr lists)))))

(define (map f list1 . lists)
  (if (null? lists)
      ;; One list, the common case: walk it directly.
      (let loop ((l list1))
        (if (pair? l) (cons (f (car l)) (loop (cdr l))) '()))
      (let loop ((ls (cons list1 lists)))
        (if (%any-null? ls)
            '()
            (cons (apply f (map car ls)) (loop (map cdr ls)))))))

(define (for-each f list1 . lists)
  (if (null? lists)
      (let loop ((l list1))
        (when (pair? l) (f (car l)) (loop (cdr l))))
      (let loop ((ls (cons list1 lists)))
        (unless (%any-null? ls)
          (apply f (map car ls))
          (loop (map cdr ls))))))

(define (member x lst . compare)
  (let ((same? (if (null? compare) equal? (car compare))))
    (let loop ((l lst))
      (cond ((null? l) #f)
            ((same? x (car l)) l)
            (else (loop (cdr l)))))))

(define (assoc x alist . compare)
  (let ((same? (if (null? compare) equal? (car compare))))
    (let loop ((l alist))
      (cond ((null? l) #f)
            ((same? x (caar l)) (car l))
            (else (loop (cdr l)))))))

;;; Strings and vectors (R7RS §6.10), via lists, as in s1.

(define (string-map f s . strings)
  (list->string (apply map f (map string->list (cons s strings)))))
(define (string-for-each f s . strings)
  (apply for-each f (map string->list (cons s strings))))
(define (vector-map f v . vectors)
  (list->vector (apply map f (map vector->list (cons v vectors)))))
(define (vector-for-each f v . vectors)
  (apply for-each f (map vector->list (cons v vectors))))

;;; Ports (R7RS §6.13.1).

(define (call-with-port port proc)
  (let ((result (proc port)))
    (close-port port)
    result))
(define (call-with-input-file filename proc)
  (call-with-port (open-input-file filename) proc))
(define (call-with-output-file filename proc)
  (call-with-port (open-output-file filename) proc))
