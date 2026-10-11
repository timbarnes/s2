//! Identifiers, aliases and `syntax-rules` (R7RS §4.3.2), forked from s1's
//! `syntax_rules.rs` and `eval/identifiers.rs`.
//!
//! An *identifier* is a symbol or an alias (`Obj::Alias`). Expanding a
//! macro use renames every identifier its template introduces to a fresh
//! alias, which records the identifier it renames and the macro's
//! definition scope. The expander (`expand.rs`) resolves aliases; this file
//! only matches and instantiates, which runs no Scheme code and needs no
//! scopes, apart from comparing literals, which the caller supplies.
//!
//! Differences from s1: values are `Value`s, the definition environment is
//! a scope depth instead of an `EnvRef`, and literal data in an expansion
//! isn't stripped of aliases here: the expander does it when it meets
//! `quote`, `quasiquote` or a vector constant.

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::heap::{GcRef, Heap, Obj, equal};
use crate::printer::print_value;
use crate::value::Value;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// `v` as an identifier: a symbol or an alias.
pub fn identifier(v: Value) -> Option<GcRef> {
    matches!(kind(v), Some(Obj::Symbol(_) | Obj::Alias { .. })).then(|| v.as_ref().unwrap())
}

/// The symbol an identifier ultimately renames (itself, for a symbol).
pub fn strip(id: GcRef) -> GcRef {
    let mut id = id;
    while let Obj::Alias { original, .. } = &id.get().kind {
        id = original.as_ref().unwrap();
    }
    id
}

/// The name of the symbol an identifier renames.
pub fn name<'a>(id: GcRef) -> &'a str {
    match &strip(id).get().kind {
        Obj::Symbol(s) => s,
        _ => unreachable!("not an identifier"),
    }
}

fn kind<'a>(v: Value) -> Option<&'a Obj> {
    v.as_obj().map(|o| &o.kind)
}

/// `datum` with every alias in it replaced by its symbol: what a quoted
/// template must yield. Returns `datum` itself, allocating nothing, when it
/// contains no aliases. Cyclic data (from datum labels) is returned
/// unchanged.
pub fn strip_datum(heap: &mut Heap, datum: Value) -> Value {
    let mut seen = FxHashSet::default();
    let mut cyclic = false;
    if !contains_alias(datum, &mut seen, &mut cyclic) || cyclic {
        return datum;
    }
    copy_stripped(heap, datum)
}

fn contains_alias(datum: Value, seen: &mut FxHashSet<GcRef>, cyclic: &mut bool) -> bool {
    match kind(datum) {
        Some(Obj::Alias { .. }) => true,
        Some(Obj::Pair { .. } | Obj::Vector(_)) if !seen.insert(datum.as_ref().unwrap()) => {
            *cyclic = true;
            false
        }
        // Bitwise `|` so both sides are always walked (for `cyclic`).
        Some(Obj::Pair { car, cdr }) => contains_alias(car.get(), seen, cyclic) | contains_alias(cdr.get(), seen, cyclic),
        Some(Obj::Vector(items)) => items.iter().fold(false, |any, c| any | contains_alias(c.get(), seen, cyclic)),
        _ => false,
    }
}

fn copy_stripped(heap: &mut Heap, datum: Value) -> Value {
    match kind(datum) {
        Some(Obj::Alias { .. }) => Value::obj(strip(datum.as_ref().unwrap())),
        Some(Obj::Pair { .. }) => {
            let (items, tail) = list_parts(datum);
            let mut result = copy_stripped(heap, tail);
            for item in items.into_iter().rev() {
                let item = copy_stripped(heap, item);
                result = heap.cons(item, result);
            }
            result
        }
        Some(Obj::Vector(items)) => {
            let items: Vec<Value> = items.iter().map(|c| c.get()).collect();
            let copied = items.into_iter().map(|i| copy_stripped(heap, i)).collect();
            heap.vector(copied)
        }
        _ => datum,
    }
}

/// The elements of a (possibly improper) list, and its final cdr.
pub fn list_parts(list: Value) -> (Vec<Value>, Value) {
    let mut items = Vec::new();
    let mut rest = list;
    while let Some(Obj::Pair { car, cdr }) = kind(rest) {
        items.push(car.get());
        rest = cdr.get();
    }
    (items, rest)
}

/// A vector's elements, or a list's elements and final cdr.
fn sequence_parts(v: Value) -> (Vec<Value>, Value) {
    match kind(v) {
        Some(Obj::Vector(items)) => (items.iter().map(|c| c.get()).collect(), Value::NIL),
        _ => list_parts(v),
    }
}

// ---------------------------------------------------------------------------
// Transformers
// ---------------------------------------------------------------------------

/// A transformer: a parsed `syntax-rules` form.
pub struct SyntaxRules {
    /// The custom ellipsis identifier, if one was given.
    pub ellipsis: Option<GcRef>,
    /// The literal identifiers, matched by identity.
    pub literals: Vec<GcRef>,
    /// (pattern, template) pairs, in order.
    pub rules: Vec<(Value, Value)>,
    /// The definition scope: how many of the expander's frames were
    /// visible where the macro was defined (0 at top level). The aliases
    /// an expansion makes resolve there.
    pub depth: u32,
}

/// What a pattern variable matched: one form, or a sequence per ellipsis.
/// The sequence is shared, because instantiating `sub ...` copies the
/// bindings once per repetition: with a `Vec`, expanding a recursive macro
/// such as `or` with n arguments took O(n³) time (as it does in s1).
#[derive(Clone)]
enum Match {
    One(Value),
    Many(Rc<[Match]>),
}

/// The pattern variables of a successful match.
pub struct Bindings(FxHashMap<GcRef, Match>);

impl SyntaxRules {
    fn is_literal(&self, id: GcRef) -> bool {
        self.literals.contains(&id)
    }

    /// Whether `v` is the ellipsis. Literals take priority (R7RS §4.3.2). A
    /// custom ellipsis is matched by identity; the default matches any
    /// identifier named `...`, so a `...` made by an outer template's
    /// `(... ...)` escape (an alias) works in an inner macro.
    fn is_ellipsis(&self, v: Value) -> bool {
        let Some(id) = identifier(v) else { return false };
        if self.is_literal(id) {
            return false;
        }
        match self.ellipsis {
            Some(e) => id == e,
            None => name(id) == "...",
        }
    }

    /// Whether `id` is the wildcard `_` (and not a literal).
    fn is_underscore(&self, id: GcRef) -> bool {
        !self.is_literal(id) && name(id) == "_"
    }

    /// Find the first rule whose pattern matches the use `form`. `same`
    /// says whether a literal (as the macro's definition scope sees it)
    /// and an input identifier (as the use's scope sees it) mean the same
    /// binding: R7RS's `free-identifier=?`.
    pub fn match_use(&self, form: Value, same: &dyn Fn(GcRef, GcRef) -> bool) -> Result<(Value, Bindings), String> {
        let input = match kind(form) {
            Some(Obj::Pair { cdr, .. }) => cdr.get(),
            _ => unreachable!("a macro use is a pair"),
        };
        for &(pattern, template) in &self.rules {
            let Some(Obj::Pair { cdr, .. }) = kind(pattern) else { continue };
            let mut binds = FxHashMap::default();
            if self.match_pattern(cdr.get(), input, same, &mut binds) {
                return Ok((template, Bindings(binds)));
            }
        }
        Err(format!("no syntax rule matches {}", print_value(form)))
    }

    fn match_pattern(&self, pattern: Value, input: Value, same: &dyn Fn(GcRef, GcRef) -> bool, binds: &mut FxHashMap<GcRef, Match>) -> bool {
        if let Some(p) = identifier(pattern) {
            return if self.is_literal(p) {
                identifier(input).is_some_and(|i| same(p, i))
            } else {
                if !self.is_underscore(p) {
                    binds.insert(p, Match::One(input));
                }
                true
            };
        }
        match kind(pattern) {
            Some(Obj::Pair { .. }) => {
                let (items, tail) = list_parts(pattern);
                if !items.iter().any(|&p| self.is_ellipsis(p)) {
                    return self.match_pairs(pattern, input, same, binds);
                }
                let (inputs, input_tail) = list_parts(input);
                self.match_sequence(&items, tail, &inputs, input_tail, same, binds)
            }
            Some(Obj::Vector(items)) => match kind(input) {
                Some(Obj::Vector(_)) => {
                    let items: Vec<Value> = items.iter().map(|c| c.get()).collect();
                    let (inputs, _) = sequence_parts(input);
                    self.match_sequence(&items, Value::NIL, &inputs, Value::NIL, same, binds)
                }
                _ => false,
            },
            _ if pattern.is_nil() => input.is_nil(),
            _ => equal(pattern, input),
        }
    }

    /// Match a list pattern with no ellipsis, pair by pair; a dotted tail in
    /// the pattern matches whatever input remains (`(a . rest)`).
    fn match_pairs(&self, mut pattern: Value, mut input: Value, same: &dyn Fn(GcRef, GcRef) -> bool, binds: &mut FxHashMap<GcRef, Match>) -> bool {
        loop {
            match (kind(pattern), kind(input)) {
                (Some(Obj::Pair { car: p, cdr: prest }), Some(Obj::Pair { car: i, cdr: irest })) => {
                    if !self.match_pattern(p.get(), i.get(), same, binds) {
                        return false;
                    }
                    pattern = prest.get();
                    input = irest.get();
                }
                (Some(Obj::Pair { .. }), _) => return false,
                _ if pattern.is_nil() => return input.is_nil(),
                _ => return self.match_pattern(pattern, input, same, binds),
            }
        }
    }

    /// Match pattern elements `items` (with final cdr `tail`) against input
    /// elements `inputs` (with final cdr `input_tail`): a list pattern
    /// containing an ellipsis, or any vector pattern. The element before the
    /// ellipsis takes as many inputs as leave enough for the patterns after
    /// it; a non-nil `tail` then matches the input's final cdr.
    fn match_sequence(
        &self,
        items: &[Value],
        tail: Value,
        inputs: &[Value],
        input_tail: Value,
        same: &dyn Fn(GcRef, GcRef) -> bool,
        binds: &mut FxHashMap<GcRef, Match>,
    ) -> bool {
        let ellipsis_at = items.iter().position(|&p| self.is_ellipsis(p));
        let (before, repeated, after) = match ellipsis_at {
            // `parse` rejects a leading ellipsis; refuse rather than panic.
            Some(0) => return false,
            Some(i) => (&items[..i - 1], Some(items[i - 1]), &items[i + 1..]),
            None => (items, None, &items[items.len()..]),
        };
        let reps = match repeated {
            Some(_) => match inputs.len().checked_sub(before.len() + after.len()) {
                Some(n) => n,
                None => return false,
            },
            // Only vectors get here without an ellipsis.
            None if inputs.len() != before.len() => return false,
            None => 0,
        };
        for (&p, &i) in before.iter().zip(inputs) {
            if !self.match_pattern(p, i, same, binds) {
                return false;
            }
        }
        if let Some(sub) = repeated {
            let mut matches = Vec::with_capacity(reps);
            for &i in &inputs[before.len()..before.len() + reps] {
                let mut b = FxHashMap::default();
                if !self.match_pattern(sub, i, same, &mut b) {
                    return false;
                }
                matches.push(b);
            }
            for var in self.pattern_vars(sub) {
                let seq = matches.iter_mut().map(|b| b.remove(&var).expect("pattern variable bound by match")).collect();
                binds.insert(var, Match::Many(seq));
            }
            for (&p, &i) in after.iter().zip(&inputs[before.len() + reps..]) {
                if !self.match_pattern(p, i, same, binds) {
                    return false;
                }
            }
        }
        if tail.is_nil() { input_tail.is_nil() } else { self.match_pattern(tail, input_tail, same, binds) }
    }

    /// The pattern variables in `pattern`, in order.
    fn pattern_vars(&self, pattern: Value) -> Vec<GcRef> {
        let mut vars = Vec::new();
        self.collect_pattern_vars(pattern, 0, &mut |v, _| vars.push(v));
        vars
    }

    /// Call `out` with each pattern variable in `pattern` and its ellipsis
    /// depth (`depth` at the top).
    fn collect_pattern_vars(&self, pattern: Value, depth: usize, out: &mut dyn FnMut(GcRef, usize)) {
        if let Some(id) = identifier(pattern) {
            if !self.is_literal(id) && !self.is_underscore(id) && !self.is_ellipsis(pattern) {
                out(id, depth);
            }
            return;
        }
        if let Some(Obj::Pair { .. } | Obj::Vector(_)) = kind(pattern) {
            let (items, tail) = sequence_parts(pattern);
            for (i, &item) in items.iter().enumerate() {
                let repeated = items.get(i + 1).is_some_and(|&n| self.is_ellipsis(n));
                self.collect_pattern_vars(item, depth + repeated as usize, out);
            }
            self.collect_pattern_vars(tail, depth, out);
        }
    }

    /// Build the expansion of `template` under `binds`, renaming every
    /// identifier the template introduces: one fresh alias per distinct
    /// identifier, so all occurrences of `temp` in one expansion are the
    /// same alias.
    pub fn instantiate(&self, heap: &mut Heap, template: Value, binds: &Bindings) -> Result<Value, String> {
        self.instantiate_in(heap, template, &binds.0, &mut FxHashMap::default(), false)
    }

    /// `escaped` is true inside `(... template)`, where the ellipsis is an
    /// ordinary identifier.
    fn instantiate_in(
        &self,
        heap: &mut Heap,
        template: Value,
        binds: &FxHashMap<GcRef, Match>,
        renames: &mut FxHashMap<GcRef, Value>,
        escaped: bool,
    ) -> Result<Value, String> {
        if let Some(id) = identifier(template) {
            return match binds.get(&id) {
                Some(Match::One(v)) => Ok(*v),
                Some(Match::Many(_)) => Err(format!("syntax-rules: pattern variable {} used without an ellipsis", print_value(template))),
                None => Ok(*renames.entry(id).or_insert_with(|| heap.alias(template, self.depth))),
            };
        }
        match kind(template) {
            Some(Obj::Pair { car, cdr }) => {
                // (... template): the template with the ellipsis escaped.
                if !escaped
                    && self.is_ellipsis(car.get())
                    && let Some(Obj::Pair { car: inner, .. }) = kind(cdr.get())
                {
                    return self.instantiate_in(heap, inner.get(), binds, renames, true);
                }
                let (items, tail) = list_parts(template);
                let out = self.instantiate_items(heap, &items, binds, renames, escaped)?;
                let mut result = self.instantiate_in(heap, tail, binds, renames, escaped)?;
                for item in out.into_iter().rev() {
                    result = heap.cons(item, result);
                }
                Ok(result)
            }
            Some(Obj::Vector(items)) => {
                let items: Vec<Value> = items.iter().map(|c| c.get()).collect();
                let out = self.instantiate_items(heap, &items, binds, renames, escaped)?;
                Ok(heap.vector(out))
            }
            _ => Ok(template),
        }
    }

    /// Instantiate the elements of a list or vector template, expanding
    /// `sub ...` (with any number of ellipses) in place.
    fn instantiate_items(
        &self,
        heap: &mut Heap,
        items: &[Value],
        binds: &FxHashMap<GcRef, Match>,
        renames: &mut FxHashMap<GcRef, Value>,
        escaped: bool,
    ) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < items.len() {
            let mut depth = 0;
            while !escaped && items.get(i + 1 + depth).is_some_and(|&n| self.is_ellipsis(n)) {
                depth += 1;
            }
            if depth == 0 {
                out.push(self.instantiate_in(heap, items[i], binds, renames, escaped)?);
            } else {
                self.instantiate_repeated(heap, items[i], binds, renames, depth, &mut out)?;
            }
            i += 1 + depth;
        }
        Ok(out)
    }

    /// Instantiate `sub ...` (with `depth` ellipses), appending each result.
    fn instantiate_repeated(
        &self,
        heap: &mut Heap,
        sub: Value,
        binds: &FxHashMap<GcRef, Match>,
        renames: &mut FxHashMap<GcRef, Value>,
        depth: usize,
        out: &mut Vec<Value>,
    ) -> Result<(), String> {
        let mut vars = Vec::new();
        template_vars(sub, binds, &mut vars, &mut FxHashSet::default());
        let seqs: Vec<(GcRef, Rc<[Match]>)> = vars
            .iter()
            .filter_map(|v| match binds.get(v) {
                Some(Match::Many(seq)) => Some((*v, seq.clone())),
                _ => None,
            })
            .collect();
        let Some((_, first)) = seqs.first() else {
            return Err(format!("syntax-rules: no pattern variable to repeat in {}", print_value(sub)));
        };
        let len = first.len();
        if seqs.iter().any(|(_, seq)| seq.len() != len) {
            return Err("syntax-rules: pattern variables repeat different numbers of times".into());
        }
        for i in 0..len {
            let mut inner = binds.clone();
            for (var, seq) in &seqs {
                inner.insert(*var, seq[i].clone());
            }
            if depth == 1 {
                out.push(self.instantiate_in(heap, sub, &inner, renames, false)?);
            } else {
                self.instantiate_repeated(heap, sub, &inner, renames, depth - 1, out)?;
            }
        }
        Ok(())
    }
}

/// The pattern variables (keys of `binds`) occurring in `template`.
fn template_vars(template: Value, binds: &FxHashMap<GcRef, Match>, out: &mut Vec<GcRef>, seen: &mut FxHashSet<GcRef>) {
    if let Some(id) = identifier(template) {
        if binds.contains_key(&id) && seen.insert(id) {
            out.push(id);
        }
        return;
    }
    match kind(template) {
        Some(Obj::Pair { car, cdr }) => {
            template_vars(car.get(), binds, out, seen);
            template_vars(cdr.get(), binds, out, seen);
        }
        Some(Obj::Vector(items)) => items.iter().for_each(|c| template_vars(c.get(), binds, out, seen)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Parsing and validation
// ---------------------------------------------------------------------------

/// Parse `(syntax-rules [ellipsis] (literal ...) (pattern template) ...)`,
/// defined in a scope of `depth` frames, checking the rules are well formed.
pub fn parse(form: Value, depth: u32) -> Result<SyntaxRules, String> {
    let (parts, tail) = list_parts(form);
    if !tail.is_nil() || parts.len() < 2 {
        return Err("syntax-rules: expected (syntax-rules (literal ...) rule ...)".into());
    }
    let mut rest = &parts[1..];
    let ellipsis = identifier(rest[0]);
    if ellipsis.is_some() {
        rest = &rest[1..];
    }
    let (literal_list, rules) = rest.split_first().ok_or("syntax-rules: missing literal list")?;
    let (literals, lit_tail) = list_parts(*literal_list);
    let literals: Option<Vec<GcRef>> = literals.into_iter().map(identifier).collect();
    let Some(literals) = literals.filter(|_| lit_tail.is_nil()) else {
        return Err("syntax-rules: literals must be a list of identifiers".into());
    };
    let mut sr = SyntaxRules { ellipsis, literals, rules: Vec::new(), depth };
    for &rule in rules {
        let (pt, rule_tail) = list_parts(rule);
        if pt.len() != 2 || !rule_tail.is_nil() {
            return Err(format!("syntax-rules: a rule must be (pattern template): {}", print_value(rule)));
        }
        let (pattern, template) = (pt[0], pt[1]);
        let Some(Obj::Pair { cdr, .. }) = kind(pattern) else {
            return Err(format!("syntax-rules: a pattern must be a list: {}", print_value(pattern)));
        };
        // The keyword position is ignored entirely.
        sr.check_pattern(cdr.get())?;
        let mut depths: FxHashMap<GcRef, Vec<usize>> = FxHashMap::default();
        sr.collect_pattern_vars(cdr.get(), 0, &mut |v, d| depths.entry(v).or_default().push(d));
        if let Some((v, _)) = depths.iter().find(|(_, ds)| ds.len() > 1) {
            return Err(format!("syntax-rules: pattern variable {} appears twice", print_value(Value::obj(*v))));
        }
        let depths: FxHashMap<GcRef, usize> = depths.into_iter().map(|(v, ds)| (v, ds[0])).collect();
        sr.check_template(template, &depths, 0, false)?;
        sr.rules.push((pattern, template));
    }
    Ok(sr)
}

impl SyntaxRules {
    /// At most one ellipsis per list or vector level, and it must follow a
    /// pattern.
    fn check_pattern(&self, p: Value) -> Result<(), String> {
        if !matches!(kind(p), Some(Obj::Pair { .. } | Obj::Vector(_))) {
            return Ok(());
        }
        let (items, tail) = sequence_parts(p);
        let ellipses: Vec<usize> = items.iter().enumerate().filter(|&(_, &i)| self.is_ellipsis(i)).map(|(i, _)| i).collect();
        if ellipses.len() > 1 {
            return Err(format!("syntax-rules: more than one ellipsis in {}", print_value(p)));
        }
        if ellipses.first() == Some(&0) {
            return Err(format!("syntax-rules: ellipsis must follow a pattern in {}", print_value(p)));
        }
        for &i in items.iter().filter(|&&i| !self.is_ellipsis(i)) {
            self.check_pattern(i)?;
        }
        self.check_pattern(tail)
    }

    /// Every pattern variable must be used under at least as many ellipses
    /// as it was matched under.
    fn check_template(&self, template: Value, depths: &FxHashMap<GcRef, usize>, depth: usize, escaped: bool) -> Result<(), String> {
        if let Some(id) = identifier(template) {
            return match depths.get(&id) {
                Some(&d) if d > depth => Err(format!(
                    "syntax-rules: pattern variable {} needs {} ellipsis(es) in the template",
                    print_value(template),
                    d
                )),
                _ => Ok(()),
            };
        }
        if let Some(Obj::Pair { car, cdr }) = kind(template)
            && !escaped
            && self.is_ellipsis(car.get())
        {
            return match list_parts(cdr.get()) {
                (inner, tail) if inner.len() == 1 && tail.is_nil() => self.check_template(inner[0], depths, depth, true),
                _ => Err("syntax-rules: (... template) takes exactly one template".into()),
            };
        }
        if !matches!(kind(template), Some(Obj::Pair { .. } | Obj::Vector(_))) {
            return Ok(());
        }
        let (items, tail) = sequence_parts(template);
        let mut i = 0;
        while i < items.len() {
            let mut n = 0;
            while !escaped && items.get(i + 1 + n).is_some_and(|&x| self.is_ellipsis(x)) {
                n += 1;
            }
            self.check_template(items[i], depths, depth + n, escaped)?;
            i += 1 + n;
        }
        self.check_template(tail, depths, depth, escaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::new_string_port_input;
    use crate::reader::parse as read_datum;

    fn read(heap: &mut Heap, src: &str) -> Value {
        read_datum(heap, &mut new_string_port_input(src)).unwrap()
    }

    /// Expand `use_src` with the rules in `src`. Literals match by name.
    fn expand(src: &str, use_src: &str) -> Result<String, String> {
        let mut heap = Heap::new();
        let form = read(&mut heap, src);
        let sr = parse(form, 0)?;
        let use_form = read(&mut heap, use_src);
        let (template, binds) = sr.match_use(use_form, &|a, b| strip(a) == strip(b))?;
        let out = sr.instantiate(&mut heap, template, &binds)?;
        Ok(print_value(out))
    }

    #[test]
    fn substitutes_and_renames() {
        let mut heap = Heap::new();
        let sr = parse(read(&mut heap, "(syntax-rules () ((_ e) (t e t)))"), 0).unwrap();
        let use_form = read(&mut heap, "(m t)");
        let (template, binds) = sr.match_use(use_form, &|_, _| false).unwrap();
        let (items, _) = list_parts(sr.instantiate(&mut heap, template, &binds).unwrap());
        assert!(matches!(kind(items[0]), Some(Obj::Alias { .. })), "introduced identifiers are aliases");
        assert!(items[0].eq(items[2]), "both template t's are the same alias");
        assert!(items[1].eq(heap.intern("t")), "the user's t is untouched");
        let (again, _) = list_parts(sr.instantiate(&mut heap, template, &binds).unwrap());
        assert!(!again[0].eq(items[0]), "a new expansion gets new aliases");
    }

    #[test]
    fn patterns() {
        let part2 = "(syntax-rules () ((_ a b (m n) ... x y) (v (l a b) (l m ...) (l n ...) (l x y))))";
        assert_eq!(expand(part2, "(p 10 20 (31 32) (41 42) 63 77)").unwrap(), "(v (l 10 20) (l 31 41) (l 32 42) (l 63 77))");
        assert_eq!(expand(part2, "(p 10 20 63 77)").unwrap(), "(v (l 10 20) (l) (l) (l 63 77))");
        assert!(expand(part2, "(p 10 20 63)").is_err(), "too few elements");
        let tail = "(syntax-rules () ((_ (a x ... . r)) (q (a (x ...) r))))";
        assert_eq!(expand(tail, "(p (1 2 3 . 4))").unwrap(), "(q (1 (2 3) 4))");
        assert_eq!(expand(tail, "(p (1 2 3))").unwrap(), "(q (1 (2 3) ()))");
        assert_eq!(expand("(syntax-rules () ((_ #(a b ...)) (q a b ...)))", "(p #(1 2 3))").unwrap(), "(q 1 2 3)");
        assert_eq!(expand("(syntax-rules () ((_ (a b ...) ...) (q b ... ...)))", "(p (1 2 3) (4 5))").unwrap(), "(q 2 3 5)");
        assert_eq!(expand("(syntax-rules () ((_ x) #(x x)))", "(p 1)").unwrap(), "#(1 1)");
        assert_eq!(expand("(syntax-rules () ((_ 1 \"s\") yes))", "(p 1 \"s\")").unwrap(), "yes");
        let under = "(syntax-rules () ((_) 0) ((_ _) 1) ((_ _ _) 2) ((_ . _) many))";
        assert_eq!(expand(under, "(p a b)").unwrap(), "2");
        assert_eq!(expand(under, "(p a b c)").unwrap(), "many");
    }

    #[test]
    fn literals_escapes_and_custom_ellipses() {
        assert_eq!(expand("(syntax-rules (=>) ((_ a => b) arrow) ((_ a b c) plain))", "(p 1 => 2)").unwrap(), "arrow");
        assert_eq!(expand("(syntax-rules (_) ((_ _) lit) ((_ x) other))", "(p _)").unwrap(), "lit");
        assert_eq!(expand("(syntax-rules () ((_ x) (... (x ...))))", "(p 100)").unwrap(), "(100 ...)");
        assert_eq!(expand("(syntax-rules dots () ((_ x dots) (l x dots)))", "(p 1 2)").unwrap(), "(l 1 2)");
        assert_eq!(expand("(syntax-rules ... (...) ((_ x) (x ...)))", "(p 100)").unwrap(), "(100 ...)");
        assert_eq!(expand("(syntax-rules () ((_ a) a))", "(p 1 2)").unwrap_err(), "no syntax rule matches (p 1 2)");
    }

    #[test]
    fn malformed_rules_are_rejected() {
        let err = |src: &str| expand(src, "(p)").unwrap_err();
        assert!(err("(syntax-rules () ((_ ... x) 1))").contains("must follow"));
        assert!(err("(syntax-rules () ((_ x ... y ...) 1))").contains("more than one"));
        assert!(err("(syntax-rules () ((_ x ...) x))").contains("needs"));
        assert!(err("(syntax-rules () ((_ x x) 1))").contains("twice"));
        assert!(err("(syntax-rules () (_ 1))").contains("must be a list"));
        assert!(err("(syntax-rules (1) ((_) 1))").contains("identifiers"));
    }

    #[test]
    fn strip_datum_copies_only_when_needed() {
        let mut heap = Heap::new();
        let plain = read(&mut heap, "(a 1)");
        assert!(strip_datum(&mut heap, plain).eq(plain), "no aliases: returned as is");
        let a = heap.intern("a");
        let renamed = heap.alias(a, 0);
        let vec = heap.vector(vec![renamed]);
        let tail = heap.cons(renamed, renamed);
        let datum = heap.list(vec![renamed, vec, tail]);
        let stripped = strip_datum(&mut heap, datum);
        assert!(!stripped.eq(datum));
        assert_eq!(print_value(stripped), "(a #(a) (a . a))");
        let (items, _) = list_parts(stripped);
        assert!(items[0].eq(a), "every symbol in the copy is the interned one");
        let cyclic = read(&mut heap, "#0=(a . #0#)");
        assert!(strip_datum(&mut heap, cyclic).eq(cyclic));
    }
}
