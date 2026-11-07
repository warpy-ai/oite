//! Compile errors with source positions, rendered as
//!
//! ```text
//! file.ot:5:13: borrow error: Cannot borrow moved variable 'a'
//!   |
//! 5 | console.log(a.v);
//!   |             ^
//! file.ot:4:9: note: value moved here
//!   |
//! 4 | let b = a;
//!   |         ^
//! ```

use std::fmt;

/// A position in a source file: 1-based line and column (in characters).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    /// The position of byte `offset` in `source`.
    pub fn of_offset(source: &str, offset: usize) -> Self {
        let offset = offset.min(source.len());
        let before = &source[..floor_char_boundary(source, offset)];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let col = before[line_start..].chars().count() + 1;
        Self { line, col }
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// One error, with an optional related place.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// "syntax error", "borrow error", "error", ...
    pub kind: &'static str,
    pub message: String,
    pub position: Option<Position>,
    pub note: Option<(String, Position)>,
}

impl Diagnostic {
    pub fn new(kind: &'static str, message: impl Into<String>, position: Option<Position>) -> Self {
        Self {
            kind,
            message: message.into(),
            position,
            note: None,
        }
    }
}

/// Why compilation failed: one or more diagnostics.
#[derive(Clone, Debug)]
pub struct CompileError {
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileError {
    pub fn single(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    /// Each diagnostic as `file:line:col: kind: message`, with the source
    /// line and a caret under the column.
    pub fn render(&self, file: &str, source: &str) -> String {
        let mut out = String::new();
        for d in &self.diagnostics {
            render_one(&mut out, file, source, d.kind, &d.message, d.position);
            if let Some((note, pos)) = &d.note {
                render_one(&mut out, file, source, "note", note, Some(*pos));
            }
        }
        out.trim_end().to_string()
    }
}

fn render_one(
    out: &mut String,
    file: &str,
    source: &str,
    kind: &str,
    message: &str,
    position: Option<Position>,
) {
    let Some(pos) = position else {
        out.push_str(&format!("{}: {}: {}\n", file, kind, message));
        return;
    };
    out.push_str(&format!(
        "{}:{}:{}: {}: {}\n",
        file, pos.line, pos.col, kind, message
    ));
    if let Some(text) = source.lines().nth(pos.line - 1) {
        let number = pos.line.to_string();
        let gutter = " ".repeat(number.len());
        // Tabs before the column keep the caret aligned.
        let pad: String = text
            .chars()
            .take(pos.col - 1)
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();
        out.push_str(&format!("{} |\n", gutter));
        out.push_str(&format!("{} | {}\n", number, text));
        out.push_str(&format!("{} | {}^\n", gutter, pad));
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            if let Some(p) = d.position {
                write!(f, "{}:{}: ", p.line, p.col)?;
            }
            write!(f, "{}: {}", d.kind, d.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

impl From<CompileError> for String {
    fn from(e: CompileError) -> Self {
        e.to_string()
    }
}

/// A borrow checker message (`BORROW ERROR: ...` / `LIFETIME ERROR: ...`)
/// as a kind and the message.
pub fn split_borrow_message(message: &str) -> (&'static str, String) {
    if let Some(rest) = message.strip_prefix("BORROW ERROR: ") {
        ("borrow error", rest.to_string())
    } else if let Some(rest) = message.strip_prefix("LIFETIME ERROR: ") {
        ("lifetime error", rest.to_string())
    } else {
        ("error", message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_lines_and_characters() {
        let src = "ab\ncé x\n";
        assert_eq!(Position::of_offset(src, 0), Position { line: 1, col: 1 });
        assert_eq!(Position::of_offset(src, 3), Position { line: 2, col: 1 });
        // `é` is two bytes, one column.
        assert_eq!(Position::of_offset(src, 7), Position { line: 2, col: 4 });
    }

    #[test]
    fn render_shows_the_line_and_a_caret() {
        let e = CompileError::single(Diagnostic::new(
            "error",
            "bad",
            Some(Position { line: 2, col: 5 }),
        ));
        assert_eq!(
            e.render("f.ot", "let a;\nlet b = ;\n"),
            "f.ot:2:5: error: bad\n  |\n2 | let b = ;\n  |     ^"
        );
    }
}
