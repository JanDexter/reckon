//! ANSI styling helpers. Honors `NO_COLOR` and `RECKON_NO_COLOR`; falls
//! back to plain output if stdout is not a TTY (heuristic: `--no-color`
//! flag passed by the CLI, or env override).
//!
//! Palette (per the design plan):
//!   - amber:     ANSI 256 = 172 (closest to #D97706)
//!   - green:     ANSI 256 = 71  (muted)
//!   - red-dim:   ANSI 256 = 95  (dim brown-red for rejected items)
//!   - white:     default terminal foreground
//!   - dim:       SGR 2 over the current color

use std::fmt::{self, Display, Write as _};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub color: bool,
    pub italic: bool,
}

impl Theme {
    pub fn auto() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some()
            || std::env::var_os("RECKON_NO_COLOR").is_some();
        // We do not link to is-terminal: assume color unless explicitly off,
        // which keeps the binary slim. CI pipelines should set NO_COLOR.
        Self {
            color: !no_color,
            italic: !no_color,
        }
    }

    pub fn plain() -> Self {
        Self { color: false, italic: false }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Style {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    /// 256-color foreground code, or None for default.
    pub fg: Option<u8>,
}

impl Style {
    pub const AMBER: u8 = 172;
    pub const GREEN: u8 = 71;
    pub const RED_DIM: u8 = 95;

    pub fn amber() -> Self {
        Self { fg: Some(Self::AMBER), ..Default::default() }
    }
    pub fn amber_bold() -> Self {
        Self { fg: Some(Self::AMBER), bold: true, ..Default::default() }
    }
    pub fn green() -> Self {
        Self { fg: Some(Self::GREEN), ..Default::default() }
    }
    pub fn red_dim() -> Self {
        Self { fg: Some(Self::RED_DIM), dim: true, ..Default::default() }
    }
    pub fn dim() -> Self {
        Self { dim: true, ..Default::default() }
    }
    pub fn dim_italic() -> Self {
        Self { dim: true, italic: true, ..Default::default() }
    }
    pub fn bold() -> Self {
        Self { bold: true, ..Default::default() }
    }

    pub fn paint(&self, theme: Theme, text: impl Into<String>) -> Painted {
        Painted { style: *self, theme, text: text.into() }
    }
}

pub struct Painted {
    style: Style,
    theme: Theme,
    text: String,
}

impl Display for Painted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.theme.color {
            return f.write_str(&self.text);
        }
        let mut codes = String::new();
        if self.style.bold {
            codes.push_str("1;");
        }
        if self.style.dim {
            codes.push_str("2;");
        }
        if self.style.italic && self.theme.italic {
            codes.push_str("3;");
        }
        if let Some(fg) = self.style.fg {
            let _ = write!(codes, "38;5;{fg};");
        }
        if codes.is_empty() {
            return f.write_str(&self.text);
        }
        let codes = codes.trim_end_matches(';');
        write!(f, "\x1b[{codes}m{}\x1b[0m", self.text)
    }
}

/// One-line streaming status. Each call rewrites the same line. Pass an
/// empty string to clear. The caller is responsible for ending the stream
/// with [`stream_done`] before printing the final output.
pub struct StreamLine {
    theme: Theme,
    last_len: usize,
}

impl StreamLine {
    pub fn new(theme: Theme) -> Self {
        Self { theme, last_len: 0 }
    }

    pub fn set(&mut self, msg: &str) {
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = write!(out, "\r{:width$}\r  ", "", width = self.last_len + 4);
        let dim = Style::dim().paint(self.theme, msg.to_string());
        let _ = write!(out, "{dim}");
        let _ = out.flush();
        self.last_len = msg.chars().count();
    }

    pub fn done(&mut self) {
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = write!(out, "\r{:width$}\r", "", width = self.last_len + 4);
        let _ = out.flush();
        self.last_len = 0;
    }
}

/// Render a horizontal rule of `n` columns wide using `─`.
pub fn rule(n: usize) -> String {
    "─".repeat(n)
}

/// Box-draw the warning block (Screen 3). `title` is the top header text
/// embedded into the top border. Body is rendered with two-space padding
/// on each side. The box character is amber.
pub fn warning_box(theme: Theme, title: &str, lines: &[String]) -> String {
    let mut out = String::new();
    let inner_width = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(title.chars().count() + 6)
        .max(56);
    let amber = |s: &str| Style::amber().paint(theme, s).to_string();

    let title_marker = format!("── {title} ──");
    let mut top_inner = title_marker.clone();
    while top_inner.chars().count() < inner_width + 2 {
        top_inner.push('─');
    }
    let top_line = format!("{}{}{}", amber("┌"), amber(&top_inner), amber("┐"));
    out.push_str("  ");
    out.push_str(&top_line);
    out.push('\n');

    let blank_pad = " ".repeat(inner_width);
    let blank = format!("  {} {} {}\n", amber("│"), blank_pad, amber("│"));
    out.push_str(&blank);

    for l in lines {
        let pad = inner_width.saturating_sub(l.chars().count());
        let padded = format!("{l}{}", " ".repeat(pad));
        let _ = write!(&mut out, "  {} {} {}\n", amber("│"), padded, amber("│"));
    }

    out.push_str(&blank);

    let bottom_dashes = "─".repeat(inner_width + 2);
    let bottom_line = format!("{}{}{}", amber("└"), amber(&bottom_dashes), amber("┘"));
    out.push_str("  ");
    out.push_str(&bottom_line);
    out.push('\n');
    out
}

/// Wrap a paragraph to `width` columns. Returns owned lines. Pure-ASCII
/// hyphenation rules are good enough for the demo corpus.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let after = if current.is_empty() {
            word.chars().count()
        } else {
            current.chars().count() + 1 + word.chars().count()
        };
        if after > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
