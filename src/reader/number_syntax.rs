//! R7RS numeric literal syntax (R7RS section 7.1.1), shared by the reader and
//! by `string->number`.
//!
//! s1's numbers are exact integers (`BigInt`), exact rationals and flonums.
//! Complex number syntax such as `1+2i` is recognised as a number but
//! reported as unsupported rather than being read as a symbol.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

/// A number read from text.
#[derive(Debug, PartialEq)]
pub enum Number {
    /// An exact integer.
    Int(BigInt),
    /// A non-integer, in lowest terms
    Rational(BigRational),
    /// An inexact real.
    Float(f64),
}

/// The exact number `n/d` (`d` non-zero): an `Int` when it is whole.
pub fn exact_ratio(n: BigInt, d: BigInt) -> Number {
    let r = BigRational::new(n, d);
    if r.is_integer() {
        Number::Int(r.to_integer())
    } else {
        Number::Rational(r)
    }
}

/// The result of reading text as a number.
#[derive(Debug, PartialEq)]
pub enum NumberSyntax {
    /// Not number syntax: the reader treats the text as an identifier.
    NotANumber,
    /// The number.
    Value(Number),
    /// Number syntax that s1 cannot turn into a value; the message says why.
    Error(String),
}

/// The real-number forms the grammar distinguishes, before exactness is
/// applied.
enum Real {
    /// Digits only.
    Integer(BigInt),
    /// `n/d`.
    Ratio(BigInt, BigInt),
    /// A radix-10 decimal: its digits as an integer, and the power of ten
    /// to scale them by (`12.5e3` is 125 × 10^2).
    Decimal {
        /// The digits, without the point.
        digits: BigInt,
        /// The power of ten to scale `digits` by.
        exp10: i64,
        /// The text, for parsing as an inexact `f64`.
        text: String,
    },
    /// `+inf.0` or `-inf.0`; true if positive.
    Inf(bool),
    /// `+nan.0` or `-nan.0`.
    NaN,
}

/// Exponents beyond this are refused for exact (`#e`) decimals, which would
/// otherwise build an enormous integer.
const MAX_EXACT_EXPONENT: i64 = 10_000;

/// Read `text` as a number, using `default_radix` unless a `#x`/`#b`/`#o`/`#d`
/// prefix overrides it.
pub fn parse_number(text: &str, default_radix: u32) -> NumberSyntax {
    let lower = text.to_ascii_lowercase();
    let mut body = lower.as_str();
    let mut radix = None;
    let mut exactness = None;
    while let Some(rest) = body.strip_prefix('#') {
        let mut chars = rest.chars();
        match chars.next() {
            Some(r @ ('x' | 'b' | 'o' | 'd')) if radix.is_none() => {
                radix = Some(match r {
                    'x' => 16,
                    'b' => 2,
                    'o' => 8,
                    _ => 10,
                });
            }
            Some(e @ ('e' | 'i')) if exactness.is_none() => exactness = Some(e == 'e'),
            _ => return NumberSyntax::NotANumber,
        }
        body = chars.as_str();
    }
    let radix = radix.unwrap_or(default_radix);

    if is_complex(body, radix) {
        return NumberSyntax::Error(format!("complex numbers are not supported: {}", text));
    }
    match parse_real(body, radix) {
        Some(real) => apply_exactness(real, exactness, text),
        None => NumberSyntax::NotANumber,
    }
}

/// The value of `real` with the `#e`/`#i` prefix `exact` applied (none:
/// exact unless written with a point or exponent).
fn apply_exactness(real: Real, exact: Option<bool>, text: &str) -> NumberSyntax {
    let value = match (real, exact) {
        (Real::Integer(i), Some(false)) => Number::Float(i.to_f64().unwrap_or(f64::NAN)),
        (Real::Integer(i), _) => Number::Int(i),
        (Real::Ratio(n, d), Some(false)) => {
            Number::Float(n.to_f64().unwrap_or(f64::NAN) / d.to_f64().unwrap_or(f64::NAN))
        }
        (Real::Ratio(n, d), _) => {
            if d.is_zero() {
                return NumberSyntax::Error(format!("division by zero in {}", text));
            }
            exact_ratio(n, d)
        }
        (Real::Decimal { digits, exp10, .. }, Some(true)) => {
            if exp10 >= 0 {
                if exp10 > MAX_EXACT_EXPONENT {
                    return NumberSyntax::Error(format!("exponent too large: {}", text));
                }
                Number::Int(digits * BigInt::from(10).pow(exp10 as u32))
            } else {
                if -exp10 > MAX_EXACT_EXPONENT {
                    return NumberSyntax::Error(format!("exponent too large: {}", text));
                }
                exact_ratio(digits, BigInt::from(10).pow((-exp10) as u32))
            }
        }
        (Real::Decimal { text: t, .. }, _) => match t.parse::<f64>() {
            Ok(f) => Number::Float(f),
            Err(_) => return NumberSyntax::NotANumber,
        },
        (Real::Inf(_) | Real::NaN, Some(true)) => {
            return NumberSyntax::Error(format!("{} has no exact representation", text));
        }
        (Real::Inf(positive), _) => Number::Float(if positive {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }),
        (Real::NaN, _) => Number::Float(f64::NAN),
    };
    NumberSyntax::Value(value)
}

/// A signed real in `radix`, or `None` if `s` isn't one.
fn parse_real(s: &str, radix: u32) -> Option<Real> {
    match s {
        "+inf.0" => return Some(Real::Inf(true)),
        "-inf.0" => return Some(Real::Inf(false)),
        "+nan.0" | "-nan.0" => return Some(Real::NaN),
        _ => {}
    }
    let (negative, unsigned) = match s.as_bytes().first() {
        Some(b'+') => (false, &s[1..]),
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let real = parse_ureal(unsigned, radix)?;
    Some(if negative {
        match real {
            Real::Integer(i) => Real::Integer(-i),
            Real::Ratio(n, d) => Real::Ratio(-n, d),
            Real::Decimal { digits, exp10, text } => Real::Decimal {
                digits: -digits,
                exp10,
                text: format!("-{}", text),
            },
            other => other,
        }
    } else {
        real
    })
}

/// An unsigned real in `radix`: an integer, a ratio, or (in radix 10) a
/// decimal.
fn parse_ureal(s: &str, radix: u32) -> Option<Real> {
    if let Some((n, d)) = s.split_once('/') {
        return Some(Real::Ratio(parse_uinteger(n, radix)?, parse_uinteger(d, radix)?));
    }
    if let Some(i) = parse_uinteger(s, radix) {
        return Some(Real::Integer(i));
    }
    if radix == 10 {
        parse_decimal(s)
    } else {
        None
    }
}

/// An unsigned integer in `radix`.
fn parse_uinteger(s: &str, radix: u32) -> Option<BigInt> {
    if s.is_empty() || !s.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    BigInt::parse_bytes(s.as_bytes(), radix)
}

/// `digits [. digits] [e [sign] digits]` with at least one mantissa digit.
/// The exponent marker may also be R5RS's `s`, `f`, `d` or `l`, which all
/// mean the same here (the text is already lower-cased).
fn parse_decimal(s: &str) -> Option<Real> {
    let (mantissa, exponent) = match s.split_once(['e', 's', 'f', 'd', 'l']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    let all_digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
    if int_part.len() + frac_part.len() == 0 || !all_digits(int_part) || !all_digits(frac_part) {
        return None;
    }
    let exp: i64 = match exponent {
        None => 0,
        Some(e) => {
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            if digits.is_empty() || !all_digits(digits) {
                return None;
            }
            // Saturate absurd exponents; the float parse handles them as
            // infinity or zero, and exact reads refuse them.
            e.parse::<i64>().unwrap_or(if e.starts_with('-') {
                i64::MIN / 2
            } else {
                i64::MAX / 2
            })
        }
    };
    let digits = BigInt::parse_bytes(format!("{}{}", int_part, frac_part).as_bytes(), 10)?;
    Some(Real::Decimal {
        digits,
        exp10: exp.saturating_sub(frac_part.len() as i64),
        // Rust's float parser only knows `e` as the exponent marker.
        text: s.chars().map(|c| if "sfdl".contains(c) { 'e' } else { c }).collect(),
    })
}

/// Rectangular (`1+2i`, `-i`) or polar (`1@2`) complex syntax.
fn is_complex(s: &str, radix: u32) -> bool {
    if let Some((magnitude, angle)) = s.split_once('@') {
        return parse_real(magnitude, radix).is_some() && parse_real(angle, radix).is_some();
    }
    let Some(without_i) = s.strip_suffix('i') else {
        return false;
    };
    // The imaginary part starts at the last sign that isn't the first
    // character or an exponent's sign.
    let bytes = without_i.as_bytes();
    let split = (1..bytes.len())
        .rev()
        .find(|&k| (bytes[k] == b'+' || bytes[k] == b'-') && !(radix == 10 && bytes[k - 1] == b'e'));
    let (real, imag) = match split {
        Some(k) => (&without_i[..k], &without_i[k..]),
        None => ("", without_i),
    };
    let imag_ok = match imag {
        "+" | "-" => true,
        _ => imag.starts_with(['+', '-']) && parse_real(imag, radix).is_some(),
    };
    imag_ok && (real.is_empty() || parse_real(real, radix).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> NumberSyntax {
        NumberSyntax::Value(Number::Int(BigInt::from(n)))
    }
    fn float(f: f64) -> NumberSyntax {
        NumberSyntax::Value(Number::Float(f))
    }
    fn ratio(n: i64, d: i64) -> NumberSyntax {
        NumberSyntax::Value(Number::Rational(BigRational::new(n.into(), d.into())))
    }
    fn is_error(s: &str) -> bool {
        matches!(parse_number(s, 10), NumberSyntax::Error(_))
    }

    #[test]
    fn integers_and_prefixes() {
        assert_eq!(parse_number("42", 10), int(42));
        assert_eq!(parse_number("-42", 10), int(-42));
        assert_eq!(parse_number("+7", 10), int(7));
        assert_eq!(parse_number("#xff", 10), int(255));
        assert_eq!(parse_number("#XFF", 10), int(255));
        assert_eq!(parse_number("#b-101", 10), int(-5));
        assert_eq!(parse_number("#o17", 10), int(15));
        assert_eq!(parse_number("#d10", 10), int(10));
        assert_eq!(parse_number("#e#x10", 10), int(16));
        assert_eq!(parse_number("#x#e10", 10), int(16));
        assert_eq!(parse_number("ff", 16), int(255));
        assert_eq!(parse_number("#x#x1", 10), NumberSyntax::NotANumber);
        assert_eq!(parse_number("#b2", 10), NumberSyntax::NotANumber);
    }

    #[test]
    fn decimals_and_exactness() {
        assert_eq!(parse_number("1.5", 10), float(1.5));
        assert_eq!(parse_number(".5", 10), float(0.5));
        assert_eq!(parse_number("1.", 10), float(1.0));
        assert_eq!(parse_number("-1e3", 10), float(-1000.0));
        assert_eq!(parse_number("1E3", 10), float(1000.0));
        assert_eq!(parse_number("1s2", 10), float(100.0));
        assert_eq!(parse_number("1D2", 10), float(100.0));
        assert_eq!(parse_number("1d", 10), NumberSyntax::NotANumber);
        assert_eq!(parse_number("#i5", 10), float(5.0));
        assert_eq!(parse_number("#e1.5e1", 10), int(15));
        assert_eq!(parse_number("#e1e3", 10), int(1000));
        assert_eq!(parse_number("#i1/2", 10), float(0.5));
        assert_eq!(parse_number("4/2", 10), int(2));
        assert_eq!(parse_number("1/2", 10), ratio(1, 2));
        assert_eq!(parse_number("-6/4", 10), ratio(-3, 2));
        assert_eq!(parse_number("#e1.5", 10), ratio(3, 2));
        assert_eq!(parse_number("#e-0.125", 10), ratio(-1, 8));
        assert_eq!(parse_number("#x1/A", 10), ratio(1, 10));
        assert!(is_error("1/0"));
    }

    #[test]
    fn infinities_and_nan() {
        assert_eq!(parse_number("+inf.0", 10), float(f64::INFINITY));
        assert_eq!(parse_number("-inf.0", 10), float(f64::NEG_INFINITY));
        assert!(matches!(
            parse_number("+nan.0", 10),
            NumberSyntax::Value(Number::Float(f)) if f.is_nan()
        ));
        assert!(is_error("#e+inf.0"));
        // Unsigned spellings and Rust's own float words are identifiers.
        assert_eq!(parse_number("inf.0", 10), NumberSyntax::NotANumber);
        assert_eq!(parse_number("inf", 10), NumberSyntax::NotANumber);
        assert_eq!(parse_number("nan", 10), NumberSyntax::NotANumber);
    }

    #[test]
    fn complex_is_refused() {
        assert!(is_error("1+2i"));
        assert!(is_error("+i"));
        assert!(is_error("-2.5i"));
        assert!(is_error("1@2"));
        assert!(is_error("1e2+3i"));
    }

    #[test]
    fn identifiers_are_not_numbers() {
        for s in ["+", "-", "...", "->x", "1+", "a", "e", "1e", "1.2.3", "-", "+.", "i"] {
            assert_eq!(parse_number(s, 10), NumberSyntax::NotANumber, "{}", s);
        }
    }
}
