;;; Identifiers, aliases and syntax-rules (R7RS 4.3.2), as
;;; src/compile/syntax_rules.rs does them, with the same messages.
;;;
;;; An identifier is a symbol or an alias: the renamed identifier a macro
;;; expansion introduces, which remembers the identifier it renames and the
;;; macro's definition scope. The expander resolves aliases; this file only
;;; parses, matches and instantiates.
;;;
;;; A macro is a Rust object (a parsed SyntaxRules), read and made through
;;; %macro-parts and %make-macro, so the two front ends share macros.

(define (identifier? v) (or (symbol? v) (%alias? v)))

(define (alias-original a) (vector-ref (%alias-parts a) 0))
(define (alias-depth a) (vector-ref (%alias-parts a) 1))
(define (alias-env a) (vector-ref (%alias-parts a) 2))

;; The symbol an identifier ultimately renames.
(define (strip id) (if (%alias? id) (strip (alias-original id)) id))

;; The name of that symbol.
(define (id-name id) (symbol->string (strip id)))

;; The elements of a possibly improper list, and its final cdr:
;; (items . tail).
(define (list-parts list)
  (let loop ((rest list) (items '()))
    (if (pair? rest)
        (loop (cdr rest) (cons (car rest) items))
        (cons (reverse items) rest))))

;; A vector's elements, or a list's elements and final cdr.
(define (sequence-parts v)
  (if (vector? v) (cons (vector->list v) '()) (list-parts v)))

(define (fail text) (error text))

;;; A transformer, as the matcher sees it.

(define-record-type syntax-transformer
  (make-transformer ellipsis literals rules depth env)
  transformer?
  (ellipsis transformer-ellipsis)
  (literals transformer-literals)
  ;; ((pattern . template) ...)
  (rules transformer-rules)
  (depth transformer-depth)
  (env transformer-env))

(define (macro->transformer m)
  (let ((parts (%macro-parts m)))
    (make-transformer (vector-ref parts 0) (vector-ref parts 1) (vector-ref parts 2)
                      (vector-ref parts 3) (vector-ref parts 4))))

(define (literal? sr id) (memq id (transformer-literals sr)))

;; Whether `v` is the ellipsis. Literals take priority. A custom ellipsis
;; is matched by identity; the default matches any identifier named ...
(define (ellipsis? sr v)
  (and (identifier? v)
       (not (literal? sr v))
       (if (transformer-ellipsis sr)
           (eq? v (transformer-ellipsis sr))
           (string=? (id-name v) "..."))))

(define (underscore? sr id) (and (not (literal? sr id)) (string=? (id-name id) "_")))

(define (any? ok? items)
  (and (pair? items) (or (ok? (car items)) (any? ok? (cdr items)))))

;;; Matching. Bindings are an association list from pattern variable to
;;; (one . form) or (many . (match ...)).

;; The first rule whose pattern matches the use `form`: (template .
;; bindings). `same?` says whether a literal (as the macro's definition
;; scope sees it) and an input identifier (as the use's scope sees it)
;; mean the same binding.
(define (match-use sr form same?)
  (let loop ((rules (transformer-rules sr)))
    (if (null? rules)
        (fail (string-append "no syntax rule matches " (show form)))
        (let ((pattern (car (car rules))))
          (let ((binds (and (pair? pattern) (match-pattern sr (cdr pattern) (cdr form) same? '()))))
            (if binds
                (cons (cdr (car rules)) binds)
                (loop (cdr rules))))))))

;; The bindings extended by matching, or #f.
(define (match-pattern sr pattern input same? binds)
  (cond ((identifier? pattern)
         (cond ((literal? sr pattern) (and (identifier? input) (same? pattern input) binds))
               ((underscore? sr pattern) binds)
               (else (cons (cons pattern (cons 'one input)) binds))))
        ((pair? pattern)
         (let ((parts (list-parts pattern)))
           (if (any? (lambda (p) (ellipsis? sr p)) (car parts))
               (let ((inputs (list-parts input)))
                 (match-sequence sr (car parts) (cdr parts) (car inputs) (cdr inputs) same? binds))
               (match-pairs sr pattern input same? binds))))
        ((vector? pattern)
         (and (vector? input)
              (match-sequence sr (vector->list pattern) '() (vector->list input) '() same? binds)))
        ((null? pattern) (and (null? input) binds))
        (else (and (equal? pattern input) binds))))

;; A list pattern with no ellipsis, pair by pair; a dotted tail matches
;; whatever input remains.
(define (match-pairs sr pattern input same? binds)
  (cond ((and (pair? pattern) (pair? input))
         (let ((binds (match-pattern sr (car pattern) (car input) same? binds)))
           (and binds (match-pairs sr (cdr pattern) (cdr input) same? binds))))
        ((pair? pattern) #f)
        ((null? pattern) (and (null? input) binds))
        (else (match-pattern sr pattern input same? binds))))

(define (take items n) (if (= n 0) '() (cons (car items) (take (cdr items) (- n 1)))))

;; Match `pattern` against each input in turn, pairwise, from the start
;; of both lists.
(define (match-each sr patterns inputs same? binds)
  (cond ((not binds) #f)
        ((or (null? patterns) (null? inputs)) binds)
        (else (match-each sr (cdr patterns) (cdr inputs) same?
                          (match-pattern sr (car patterns) (car inputs) same? binds)))))

;; A list pattern containing an ellipsis, or any vector pattern: the
;; element before the ellipsis takes as many inputs as leave enough for
;; the patterns after it; a non-nil `tail` matches the input's final cdr.
(define (match-sequence sr items tail inputs input-tail same? binds)
  (let ((at (let loop ((items items) (i 0))
              (cond ((null? items) #f)
                    ((ellipsis? sr (car items)) i)
                    (else (loop (cdr items) (+ i 1)))))))
    (if (eqv? at 0)
        #f
        (let* ((before (if at (take items (- at 1)) items))
               (repeated (and at (list-ref items (- at 1))))
               (after (if at (list-tail items (+ at 1)) '()))
               (n-before (length before))
               (reps (if repeated
                         (let ((n (- (length inputs) n-before (length after))))
                           (and (>= n 0) n))
                         (and (= (length inputs) n-before) 0))))
          (and reps
               (let ((binds (match-each sr before inputs same? binds)))
                 (and binds
                      (let ((binds
                             (if repeated
                                 (let collect ((ins (take (list-tail inputs n-before) reps)) (matches '()))
                                   (if (pair? ins)
                                       (let ((b (match-pattern sr repeated (car ins) same? '())))
                                         (and b (collect (cdr ins) (cons b matches))))
                                       (let ((matches (reverse matches)))
                                         (let add ((vars (pattern-vars sr repeated)) (binds binds))
                                           (if (null? vars)
                                               (match-each sr after (list-tail inputs (+ n-before reps)) same? binds)
                                               (add (cdr vars)
                                                    (cons (cons (car vars)
                                                                (cons 'many (map (lambda (b) (cdr (assq (car vars) b))) matches)))
                                                          binds)))))))
                                 binds)))
                        (and binds
                             (if (null? tail)
                                 (and (null? input-tail) binds)
                                 (match-pattern sr tail input-tail same? binds)))))))))))

;; The pattern variables in `pattern`, in order.
(define (pattern-vars sr pattern)
  (let ((vars '()))
    (collect-pattern-vars sr pattern 0 (lambda (v depth) (set! vars (cons v vars))))
    (reverse vars)))

;; Call `out` with each pattern variable and its ellipsis depth.
(define (collect-pattern-vars sr pattern depth out)
  (cond ((identifier? pattern)
         (if (and (not (literal? sr pattern)) (not (underscore? sr pattern)) (not (ellipsis? sr pattern)))
             (out pattern depth)))
        ((or (pair? pattern) (vector? pattern))
         (let ((parts (sequence-parts pattern)))
           (let loop ((items (car parts)))
             (if (pair? items)
                 (begin
                   (collect-pattern-vars sr (car items)
                                         (if (and (pair? (cdr items)) (ellipsis? sr (cadr items))) (+ depth 1) depth)
                                         out)
                   (loop (cdr items)))))
           (collect-pattern-vars sr (cdr parts) depth out)))))

;;; Instantiation: one fresh alias per distinct identifier the template
;;; introduces, per expansion.

(define (instantiate sr template binds)
  (let ((renames '()))
    (define (rename id)
      (cond ((assq id renames) => cdr)
            (else (let ((a (%make-alias id (transformer-depth sr) (transformer-env sr))))
                    (set! renames (cons (cons id a) renames))
                    a))))
    ;; `escaped` is true inside (... template), where the ellipsis is
    ;; an ordinary identifier.
    (define (inst template binds escaped?)
      (cond ((identifier? template)
             (let ((b (assq template binds)))
               (cond ((not b) (rename template))
                     ((eq? (car (cdr b)) 'one) (cdr (cdr b)))
                     (else (fail (string-append "syntax-rules: pattern variable " (show template)
                                                " used without an ellipsis"))))))
            ((pair? template)
             (if (and (not escaped?) (ellipsis? sr (car template)) (pair? (cdr template)))
                 (inst (cadr template) binds #t)
                 (let* ((parts (list-parts template))
                        (out (inst-items (car parts) binds escaped?))
                        (tail (inst (cdr parts) binds escaped?)))
                   (append out tail))))
            ((vector? template) (list->vector (inst-items (vector->list template) binds escaped?)))
            (else template)))
    ;; The elements of a list or vector template, expanding `sub ...`.
    (define (inst-items items binds escaped?)
      (let loop ((items items) (out '()))
        (if (null? items)
            (reverse out)
            (let ((depth (if escaped?
                             0
                             (let count ((rest (cdr items)) (n 0))
                               (if (and (pair? rest) (ellipsis? sr (car rest))) (count (cdr rest) (+ n 1)) n)))))
              (if (= depth 0)
                  (loop (cdr items) (cons (inst (car items) binds escaped?) out))
                  (loop (list-tail (cdr items) depth)
                        (append (reverse (inst-repeated (car items) binds depth)) out)))))))
    ;; `sub ...` with `depth` ellipses: the list of results.
    (define (inst-repeated sub binds depth)
      (let* ((vars (template-vars sub binds))
             (seqs (let loop ((vars vars) (seqs '()))
                     (if (null? vars)
                         (reverse seqs)
                         (let ((b (cdr (assq (car vars) binds))))
                           (loop (cdr vars) (if (eq? (car b) 'many) (cons (cons (car vars) (cdr b)) seqs) seqs)))))))
        (if (null? seqs)
            (fail (string-append "syntax-rules: no pattern variable to repeat in " (show sub))))
        (let ((n (length (cdr (car seqs)))))
          (if (any? (lambda (s) (not (= (length (cdr s)) n))) seqs)
              (fail "syntax-rules: pattern variables repeat different numbers of times"))
          (let loop ((i 0) (out '()))
            (if (= i n)
                (reverse out)
                (let ((inner (let add ((seqs seqs) (inner binds))
                               (if (null? seqs)
                                   inner
                                   (add (cdr seqs) (cons (cons (car (car seqs)) (list-ref (cdr (car seqs)) i)) inner))))))
                  (loop (+ i 1)
                        (if (= depth 1)
                            (cons (inst sub inner #f) out)
                            (append (reverse (inst-repeated sub inner (- depth 1))) out)))))))))
    (inst template binds #f)))

;; The pattern variables (bound in `binds`) occurring in `template`, in
;; order, once each.
(define (template-vars template binds)
  (let ((out '()))
    (let walk ((t template))
      (cond ((identifier? t)
             (if (and (assq t binds) (not (memq t out))) (set! out (cons t out))))
            ((pair? t) (walk (car t)) (walk (cdr t)))
            ((vector? t) (for-each walk (vector->list t)))))
    (reverse out)))

;;; Parsing: (syntax-rules [ellipsis] (literal ...) (pattern template) ...),
;;; defined in a scope of `depth` frames over `env`, checked, as a macro.

(define (parse-syntax-rules form depth env)
  (let ((parts (list-parts form)))
    (if (or (not (null? (cdr parts))) (< (length (car parts)) 2))
        (fail "syntax-rules: expected (syntax-rules (literal ...) rule ...)"))
    (let* ((rest (cdr (car parts)))
           (ellipsis (and (identifier? (car rest)) (car rest)))
           (rest (if ellipsis (cdr rest) rest)))
      (if (null? rest) (fail "syntax-rules: missing literal list"))
      (let ((literals (list-parts (car rest))))
        (if (not (and (null? (cdr literals)) (every? identifier? (car literals))))
            (fail "syntax-rules: literals must be a list of identifiers"))
        (let ((sr (make-transformer ellipsis (car literals) '() depth env)))
          (let ((rules
                 (map-in-order
                  (lambda (rule)
                    (let ((pt (list-parts rule)))
                      (if (not (and (= (length (car pt)) 2) (null? (cdr pt))))
                          (fail (string-append "syntax-rules: a rule must be (pattern template): " (show rule))))
                      (let ((pattern (car (car pt))) (template (cadr (car pt))))
                        (if (not (pair? pattern))
                            (fail (string-append "syntax-rules: a pattern must be a list: " (show pattern))))
                        ;; The keyword position is ignored entirely.
                        (check-pattern sr (cdr pattern))
                        (let ((depths '()))
                          (collect-pattern-vars sr (cdr pattern) 0
                                                (lambda (v d)
                                                  (if (assq v depths)
                                                      (fail (string-append "syntax-rules: pattern variable " (show v) " appears twice")))
                                                  (set! depths (cons (cons v d) depths))))
                          (check-template sr template depths 0 #f))
                        (cons pattern template))))
                  (cdr rest))))
            (%make-macro ellipsis (car literals) rules depth env)))))))

(define (map-in-order f items)
  (let loop ((items items) (out '()))
    (if (null? items) (reverse out) (loop (cdr items) (cons (f (car items)) out)))))

;; At most one ellipsis per list or vector level, and it must follow a
;; pattern.
(define (check-pattern sr p)
  (if (or (pair? p) (vector? p))
      (let* ((parts (sequence-parts p))
             (n (let count ((items (car parts)) (n 0))
                  (if (null? items) n (count (cdr items) (if (ellipsis? sr (car items)) (+ n 1) n))))))
        (if (> n 1) (fail (string-append "syntax-rules: more than one ellipsis in " (show p))))
        (if (and (pair? (car parts)) (ellipsis? sr (car (car parts))))
            (fail (string-append "syntax-rules: ellipsis must follow a pattern in " (show p))))
        (for-each (lambda (i) (if (not (ellipsis? sr i)) (check-pattern sr i))) (car parts))
        (check-pattern sr (cdr parts)))))

;; Every pattern variable must be used under at least as many ellipses as
;; it was matched under.
(define (check-template sr template depths depth escaped?)
  (cond ((identifier? template)
         (let ((d (assq template depths)))
           (if (and d (> (cdr d) depth))
               (fail (string-append "syntax-rules: pattern variable " (show template) " needs "
                                    (number->string (cdr d)) " ellipsis(es) in the template")))))
        ((and (pair? template) (not escaped?) (ellipsis? sr (car template)))
         (let ((parts (list-parts (cdr template))))
           (if (and (= (length (car parts)) 1) (null? (cdr parts)))
               (check-template sr (car (car parts)) depths depth #t)
               (fail "syntax-rules: (... template) takes exactly one template"))))
        ((or (pair? template) (vector? template))
         (let ((parts (sequence-parts template)))
           (let loop ((items (car parts)))
             (if (pair? items)
                 (let ((n (if escaped?
                              0
                              (let count ((rest (cdr items)) (n 0))
                                (if (and (pair? rest) (ellipsis? sr (car rest))) (count (cdr rest) (+ n 1)) n)))))
                   (check-template sr (car items) depths (+ depth n) escaped?)
                   (loop (list-tail (cdr items) n)))))
           (check-template sr (cdr parts) depths depth escaped?)))))
