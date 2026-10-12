//! The low-level side of ports: [`PortKind`] and character input with
//! push-back. The Scheme port procedures are in `builtins/port.rs`.
//!
//! Forked from s1/src/io.rs at bc6f118. One change: an output file port
//! owns its `File`, instead of an id in s1's table of open files, so
//! closing the port (or collecting it) closes the file.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};

use std::cell::Cell;

/// What a port reads from or writes to (R7RS 6.13).
///
/// A port is a heap object (`SchemeValue::Port`) and port procedures change
/// it in place, so every reference to the same port sees the same position,
/// accumulated output and open/closed state. Textual input files are read
/// whole into a `StringPortInput`, binary ones into a `BytevectorInput`.
#[derive(Debug)]
pub enum PortKind {
    /// Standard input
    Stdin,
    /// Standard output
    Stdout,
    /// Standard error
    Stderr,
    /// Textual input from a string (also used for files being read or loaded)
    StringPortInput {
        /// The whole text.
        content: String,
        /// The byte offset of the next character.
        pos: Cell<usize>,
        /// Set by the `#!fold-case` directive (cleared by `#!no-fold-case`):
        /// the reader then folds identifiers and character names to lower
        /// case for the rest of this port.
        fold_case: Cell<bool>,
    },
    /// Textual output accumulated in a string (`open-output-string`)
    StringPortOutput {
        /// The text written so far.
        content: String,
    },
    /// Binary input from bytes (`open-input-bytevector`, binary files)
    BytevectorInput {
        /// The whole input.
        bytes: Vec<u8>,
        /// The offset of the next byte.
        pos: usize,
    },
    /// Binary output accumulated in bytes (`open-output-bytevector`)
    BytevectorOutput {
        /// The bytes written so far.
        bytes: Vec<u8>,
    },
    /// Output to a file, textual or binary, buffered. The buffer is
    /// written when the port is flushed, closed or collected, and before
    /// s2 exits (`Heap::flush_ports`), so nothing is lost if a program
    /// exits without closing it.
    FileOutput {
        /// The file name, for printing the port.
        name: String,
        file: BufWriter<File>,
        /// Whether it is a binary port.
        binary: bool,
    },
    /// A port after `close-port`. It keeps its direction and kind, so the
    /// port predicates still answer as before; reading or writing it is an
    /// error.
    Closed {
        /// Whether it was an input port.
        input: bool,
        /// Whether it was an output port.
        output: bool,
        /// Whether it was a textual port.
        textual: bool,
    },
}

impl PortKind {
    /// `input-port?`
    pub fn is_input(&self) -> bool {
        match self {
            PortKind::Stdin | PortKind::StringPortInput { .. } | PortKind::BytevectorInput { .. } => true,
            PortKind::Closed { input, .. } => *input,
            _ => false,
        }
    }

    /// `output-port?`
    pub fn is_output(&self) -> bool {
        match self {
            PortKind::Stdout
            | PortKind::Stderr
            | PortKind::StringPortOutput { .. }
            | PortKind::BytevectorOutput { .. }
            | PortKind::FileOutput { .. } => true,
            PortKind::Closed { output, .. } => *output,
            _ => false,
        }
    }

    /// `textual-port?`; a port that is not textual is binary.
    pub fn is_textual(&self) -> bool {
        match self {
            PortKind::BytevectorInput { .. } | PortKind::BytevectorOutput { .. } => false,
            PortKind::FileOutput { binary, .. } => !binary,
            PortKind::Closed { textual, .. } => *textual,
            _ => true,
        }
    }

    /// Whether the port has not been closed.
    pub fn is_open(&self) -> bool {
        !matches!(self, PortKind::Closed { .. })
    }

    /// The closed form of this port.
    pub fn closed(&self) -> PortKind {
        PortKind::Closed {
            input: self.is_input(),
            output: self.is_output(),
            textual: self.is_textual(),
        }
    }
}

impl PortKind {
    /// Read the next character from a textual input port; `None` at end of
    /// input or for any other kind of port.
    pub fn next_char_utf8(&mut self) -> Option<char> {
        match self {
            PortKind::StringPortInput { content, pos, .. } => {
                let mut p = pos.get();
                if p >= content.len() {
                    return None;
                }
                let rest = &content[p..];
                let mut iter = rest.chars();
                let ch = iter.next()?;
                p += ch.len_utf8();
                pos.set(p);
                Some(ch)
            }
            PortKind::Stdin => stdin_next_char(),
            _ => None,
        }
    }

    /// Push back `c`, the character most recently read from this port, so
    /// the next read returns it again. Several characters can be pushed back
    /// as long as it is done in reverse order of reading. The reader relies
    /// on this to look ahead without losing characters between datums.
    pub fn unread_char(&mut self, c: char) {
        match self {
            PortKind::StringPortInput { pos, .. } => pos.set(pos.get() - c.len_utf8()),
            PortKind::Stdin => STDIN_STATE.with(|st| st.borrow_mut().pushback.push(c)),
            _ => {}
        }
    }

    /// Whether `#!fold-case` is in effect for this port.
    pub fn fold_case(&self) -> bool {
        match self {
            PortKind::StringPortInput { fold_case, .. } => fold_case.get(),
            PortKind::Stdin => STDIN_STATE.with(|st| st.borrow().fold_case),
            _ => false,
        }
    }

    /// Turn `#!fold-case` on or off for this port.
    pub fn set_fold_case(&mut self, on: bool) {
        match self {
            PortKind::StringPortInput { fold_case, .. } => fold_case.set(on),
            PortKind::Stdin => STDIN_STATE.with(|st| st.borrow_mut().fold_case = on),
            _ => {}
        }
    }
}

/// Reader state for standard input, which (unlike a string port) has no
/// position to rewind or field to hold it. There is only one stdin.
#[derive(Default)]
struct StdinState {
    /// Characters pushed back by `unread_char`, the next one last.
    pushback: Vec<char>,
    /// Whether `#!fold-case` is in effect.
    fold_case: bool,
}

thread_local! {
    /// The state of standard input.
    static STDIN_STATE: std::cell::RefCell<StdinState> = Default::default();
}

/// Read one UTF-8 character from stdin, after any pushed-back characters.
fn stdin_next_char() -> Option<char> {
    if let Some(c) = STDIN_STATE.with(|st| st.borrow_mut().pushback.pop()) {
        return Some(c);
    }
    io::stdout().flush().ok();
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut buf = [0u8; 4]; // max size of UTF-8 char
    let mut first = [0u8; 1];
    if handle.read_exact(&mut first).is_err() {
        return None;
    }

    let needed = utf8_char_width(first[0]);
    buf[0] = first[0];
    if needed > 1 {
        if handle.read_exact(&mut buf[1..needed]).is_err() {
            return None;
        }
    }

    std::str::from_utf8(&buf[..needed])
        .ok()
        .and_then(|s| s.chars().next())
}

/// UTF-8 width helper
fn utf8_char_width(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// Create a new string port for in-memory string I/O.
///
/// String ports allow reading from a string as if it were a file, with
/// an internal position pointer that advances as characters are read.
///
pub fn new_string_port_input(content: &str) -> PortKind {
    PortKind::StringPortInput {
        content: content.to_string(),
        pos: Cell::new(0),
        fold_case: Cell::new(false),
    }
}

