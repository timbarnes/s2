//! `SString`, the payload of a Scheme string.
//!
//! Strings are UTF-8, so finding the k-th character means scanning from the
//! start. `SString` also records whether the string is all ASCII, in which
//! case character k is byte k: `string-ref`, `string-set!` and
//! `string-length` are O(1) for ASCII strings, which most strings are.
//!
//! The flag is exact when it says `Ascii::Yes`. A mutation that might make
//! the string non-ASCII sets `Ascii::No`, without rescanning to see whether
//! the string happens to be all ASCII again; `No` only costs speed. Reads
//! go through `Deref` to `String`; there is no `DerefMut`, so every change
//! goes through a method here that keeps the flag right.
//!
//! Scheme strings never change length, so the text is a `Box<str>` (16
//! bytes) rather than a `String` (24): with the flag it fits beside
//! `BigInt`, whose spare byte holds `SchemeValue`'s tag, and `SchemeValue`
//! stays at 32 bytes (see the size assertion in gc/mod.rs).

use std::fmt;
use std::ops::Deref;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ascii {
    Yes,
    No,
}

#[derive(Clone, Debug)]
pub struct SString {
    s: Box<str>,
    ascii: Ascii,
}

impl SString {
    pub fn new(s: String) -> SString {
        let ascii = if s.is_ascii() { Ascii::Yes } else { Ascii::No };
        SString { s: s.into_boxed_str(), ascii }
    }

    pub fn as_str(&self) -> &str {
        &self.s
    }

    fn ascii(&self) -> bool {
        self.ascii == Ascii::Yes
    }

    /// The number of characters.
    pub fn char_len(&self) -> usize {
        if self.ascii() { self.s.len() } else { self.s.chars().count() }
    }

    /// Character `k`, if there is one.
    pub fn char_at(&self, k: usize) -> Option<char> {
        if self.ascii() {
            self.s.as_bytes().get(k).map(|&b| b as char)
        } else {
            self.s.chars().nth(k)
        }
    }

    /// The byte offset of character `k` (`k` may be the length, for the end).
    fn byte_offset(&self, k: usize) -> Option<usize> {
        if self.ascii() {
            (k <= self.s.len()).then_some(k)
        } else {
            self.s.char_indices().map(|(i, _)| i).chain(std::iter::once(self.s.len())).nth(k)
        }
    }

    /// The characters from `start` up to, not including, `end`, or None if
    /// the range is out of bounds.
    pub fn substring(&self, start: usize, end: usize) -> Option<&str> {
        if start > end {
            return None;
        }
        let from = self.byte_offset(start)?;
        let to = if self.ascii() {
            self.byte_offset(end)?
        } else {
            // Continue from `from` rather than rescanning from the start.
            let rest = &self.s[from..];
            rest.char_indices().map(|(i, _)| from + i).chain(std::iter::once(self.s.len())).nth(end - start)?
        };
        Some(&self.s[from..to])
    }

    /// Replace character `k` with `c`. False if there is no character `k`.
    pub fn set_char(&mut self, k: usize, c: char) -> bool {
        if self.ascii() && c.is_ascii() {
            match self.s.len() > k {
                true => {
                    // Safe: an ASCII byte replaces an ASCII byte.
                    unsafe { self.s.as_bytes_mut()[k] = c as u8 };
                    true
                }
                false => false,
            }
        } else {
            self.replace_chars(k, std::slice::from_ref(&c))
        }
    }

    /// Replace the characters from `at` on with `chars`. False, changing
    /// nothing, unless they all fit.
    pub fn replace_chars(&mut self, at: usize, chars: &[char]) -> bool {
        let all_ascii = chars.iter().all(char::is_ascii);
        if self.ascii() && all_ascii {
            if at + chars.len() > self.s.len() {
                return false;
            }
            // Safe: ASCII bytes replace ASCII bytes.
            let bytes = unsafe { self.s.as_bytes_mut() };
            for (b, c) in bytes[at..at + chars.len()].iter_mut().zip(chars) {
                *b = *c as u8;
            }
            return true;
        }
        let (Some(from), Some(to)) = (self.byte_offset(at), self.byte_offset(at + chars.len())) else {
            return false;
        };
        let replacement: String = chars.iter().collect();
        let mut s = std::mem::take(&mut self.s).into_string();
        s.replace_range(from..to, &replacement);
        self.s = s.into_boxed_str();
        if !all_ascii {
            self.ascii = Ascii::No;
        }
        true
    }
}

impl Deref for SString {
    type Target = str;
    fn deref(&self) -> &str {
        &self.s
    }
}

impl AsRef<str> for SString {
    fn as_ref(&self) -> &str {
        &self.s
    }
}

impl AsRef<std::ffi::OsStr> for SString {
    fn as_ref(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(&*self.s)
    }
}

impl AsRef<std::path::Path> for SString {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&*self.s)
    }
}

impl From<String> for SString {
    fn from(s: String) -> SString {
        SString::new(s)
    }
}

impl From<&str> for SString {
    fn from(s: &str) -> SString {
        SString::new(s.to_string())
    }
}

impl PartialEq for SString {
    fn eq(&self, other: &SString) -> bool {
        self.s == other.s
    }
}

impl PartialEq<str> for SString {
    fn eq(&self, other: &str) -> bool {
        &*self.s == other
    }
}

impl PartialEq<&str> for SString {
    fn eq(&self, other: &&str) -> bool {
        &*self.s == *other
    }
}

impl fmt::Display for SString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.s.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_unicode_agree() {
        for text in ["hello", "héllo", "a𝄞b", ""] {
            let s = SString::from(text);
            let chars: Vec<char> = text.chars().collect();
            assert_eq!(s.char_len(), chars.len());
            for (k, c) in chars.iter().enumerate() {
                assert_eq!(s.char_at(k), Some(*c));
            }
            assert_eq!(s.char_at(chars.len()), None);
            for start in 0..=chars.len() {
                for end in start..=chars.len() {
                    let want: String = chars[start..end].iter().collect();
                    assert_eq!(s.substring(start, end), Some(want.as_str()));
                }
            }
            assert_eq!(s.substring(0, chars.len() + 1), None);
        }
    }

    #[test]
    fn mutation_keeps_the_flag_right() {
        let mut s = SString::from("abc");
        assert!(s.set_char(1, 'x'));
        assert_eq!((s.as_str(), s.ascii()), ("axc", true));
        assert!(s.set_char(1, 'λ'));
        assert_eq!((s.as_str(), s.ascii()), ("aλc", false));
        assert_eq!(s.char_at(2), Some('c'));
        assert!(s.set_char(1, 'b'));
        assert_eq!(s.as_str(), "abc");
        assert!(!s.set_char(3, 'z'));
        assert!(s.replace_chars(1, &['𝄞', 'q']));
        assert_eq!((s.as_str(), s.char_len()), ("a𝄞q", 3));
        assert!(!s.replace_chars(2, &['1', '2']));
        assert_eq!(s.as_str(), "a𝄞q");
    }
}
