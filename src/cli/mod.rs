//! Terminal output for the `reckon` CLI binary.
//!
//! Visual language per `reckon-cli-plan.md`: restrained, monospace, one
//! accent (deep amber ≈ `#D97706` → ANSI 256 color 172), dim for
//! secondary, bold for the one important line per block, dim red only for
//! rejected/excluded items, muted green only for success confirmation.
//!
//! No TUI runtime — every screen is sequential stdout. The "spinner" is
//! a single line rewritten with `\r`, ending on a newline before the
//! final output.

pub mod style;
pub mod why;
pub mod check;
pub mod memoir;
pub mod trace;
pub mod wordmark;

pub use style::{Style, Theme};

/// Cheap trace-id source. Not cryptographically random; only used to make
/// trace ids feel distinct across rapid invocations.
pub fn why_rand_id() -> u32 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    nanos ^ pid
}
