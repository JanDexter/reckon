//! `reckon` wordmark for terminal use. Two variants:
//!   - standalone: `reckon`
//!   - bob-paired: `reckon · for bob` (the trailing fragment dimmed)
//!
//! Always lowercase, monospace, no chrome.

use super::style::{Style, Theme};

pub fn standalone(theme: Theme) -> String {
    Style::amber_bold().paint(theme, "reckon").to_string()
}

pub fn for_bob(theme: Theme) -> String {
    let head = Style::amber_bold().paint(theme, "reckon");
    let tail = Style::dim().paint(theme, " · for bob");
    format!("{head}{tail}")
}
