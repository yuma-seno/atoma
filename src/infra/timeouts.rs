//! Reading a timeout out of the environment, once.
//!
//! There were four of these, and only the LLM one trimmed its input or refused zero.
//! The other three took `ATOMA_MCP_TIMEOUT=0` at face value and produced
//! `Duration::from_secs(0)`: every `tools/list` and `tools/call` times out immediately,
//! and the run reports "Timed out calling tool 'x' on MCP server 'y'" — an error naming
//! the server rather than the setting that caused it. `ATOMA_MCP_INIT_TIMEOUT=0` means no
//! server ever starts, and a trailing space from a CI-provided value did the same thing
//! through the missing trim.
//!
//! An operator who reads that the LLM timeout treats zero as "use the default" has every
//! reason to assume the rule is general. It is now.

use std::time::Duration;

/// Seconds from a raw environment value, or `default` when it says nothing usable.
///
/// Absent, blank, unparseable and zero all mean the default. Zero would otherwise mean
/// "no timeout", which is never what an operator wants from a stall detector — and these
/// are stall detectors: something that has not answered in this long has usually stopped
/// answering rather than fallen behind.
pub fn seconds_from(raw: Option<&str>, default: u64) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(default)
}

/// The same, read from a named variable and returned as a `Duration`.
pub fn from_env(var: &str, default: u64) -> Duration {
    Duration::from_secs(seconds_from(std::env::var(var).ok().as_deref(), default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usable_value_is_taken() {
        assert_eq!(seconds_from(Some("900"), 300), 900);
        assert_eq!(
            seconds_from(Some("  120 "), 300),
            120,
            "a CI value with a space"
        );
    }

    /// The four ways a value says nothing. Zero is the one three of the four call sites
    /// used to accept, turning a stall detector into an immediate failure.
    #[test]
    fn nothing_usable_means_the_default() {
        for raw in [
            None,
            Some(""),
            Some("   "),
            Some("abc"),
            Some("-5"),
            Some("12.5"),
            Some("0"),
        ] {
            assert_eq!(seconds_from(raw, 300), 300, "{raw:?}");
        }
    }
}
