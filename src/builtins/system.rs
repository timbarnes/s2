//! The system interface (R7RS §6.14): `(scheme process-context)` and
//! `(scheme time)`, forked from s1's `builtin/system.rs`. `exit` and
//! `emergency-exit` are in `scheme/core.scm`, on `%exit`.

use std::io::Write;
use std::sync::{LazyLock, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::{Error, Primitive, err, expected, prim, string_arg};
use crate::value::Value;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The command line `command-line` returns: the program and its
/// arguments, set by `main`.
static COMMAND_LINE: OnceLock<Vec<String>> = OnceLock::new();

/// Set the command line (once, from `main`).
pub fn set_command_line(words: Vec<String>) {
    COMMAND_LINE.set(words).ok();
}

pub static PRIMITIVES: &[Primitive] = &[
    prim("command-line", 0, Some(0), |h, _| {
        let words = COMMAND_LINE.get().cloned().unwrap_or_else(|| vec!["s2".to_string()]);
        let strings = words.iter().map(|w| h.string(w)).collect();
        Ok(h.list(strings))
    }),
    prim("get-environment-variable", 1, Some(1), |h, a| {
        let name = string_arg(a[0], "get-environment-variable")?.borrow().to_string();
        // A name std::env can't look up (empty, or containing '=' or NUL)
        // is simply not set.
        if name.is_empty() || name.contains(['=', '\0']) {
            return Ok(Value::FALSE);
        }
        Ok(match std::env::var_os(&name) {
            Some(value) => h.string(&value.to_string_lossy()),
            None => Value::FALSE,
        })
    }),
    prim("get-environment-variables", 0, Some(0), |h, _| {
        let mut pairs = Vec::new();
        for (name, value) in std::env::vars_os() {
            let (name, value) = (h.string(&name.to_string_lossy()), h.string(&value.to_string_lossy()));
            pairs.push(h.cons(name, value));
        }
        Ok(h.list(pairs))
    }),
    prim("current-second", 0, Some(0), |_, _| {
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "current-second: the system clock is before 1970")?;
        Ok(Value::flonum(since_epoch.as_secs_f64()))
    }),
    // Jiffies are nanoseconds since s2 started.
    prim("current-jiffy", 0, Some(0), |h, _| Ok(h.integer(START.elapsed().as_nanos().into()))),
    prim("jiffies-per-second", 0, Some(0), |_, _| Ok(Value::fixnum(1_000_000_000).unwrap())),
    // (%exit [obj]): end the process now, with standard output flushed.
    // #t or no argument is success (0), #f failure (1), an integer itself.
    prim("%exit", 0, Some(1), |_, a| {
        let code = exit_status(a.first().copied())?;
        std::io::stdout().flush().ok();
        std::process::exit(code)
    }),
];

fn exit_status(obj: Option<Value>) -> Result<i32, Error> {
    match obj {
        None => Ok(0),
        Some(v) if v.as_bool() == Some(true) => Ok(0),
        Some(v) if v.as_bool() == Some(false) => Ok(1),
        Some(v) => match v.as_fixnum() {
            Some(n) => i32::try_from(n).map_err(|_| err!("exit: exit status {} is out of range", n)),
            None => Err(expected("exit", "a boolean or an exact integer", v)),
        },
    }
}
