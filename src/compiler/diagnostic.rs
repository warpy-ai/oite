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
    /// The file the diagnostics are in, when it is not the one being
    /// compiled (a module it imports): (path, source).
    pub origin: Option<(String, String)>,
}

impl CompileError {
    pub fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            diagnostics,
            origin: None,
        }
    }

    pub fn single(diagnostic: Diagnostic) -> Self {
        Self::new(vec![diagnostic])
    }

    /// The diagnostics are in `file` (with `source`).
    pub fn in_file(mut self, file: &str, source: &str) -> Self {
        if self.origin.is_none() {
            self.origin = Some((file.to_string(), source.to_string()));
        }
        self
    }

    /// Each diagnostic as `file:line:col: kind: message`, with the source
    /// line and a caret under the column.
    pub fn render(&self, file: &str, source: &str) -> String {
        let (file, source) = match &self.origin {
            Some((f, s)) => (f.as_str(), s.as_str()),
            None => (file, source),
        };
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

/// Instructions with no known source position.
pub const NO_POSITION: u32 = u32::MAX;

/// Where a compiled program's instructions come from, for stack traces.
#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    /// Per instruction: the byte offset of its source, or `NO_POSITION`.
    pub positions: Vec<u32>,
    /// The instructions of each function body, with its name.
    pub functions: Vec<FunctionRange>,
}

#[derive(Clone, Debug)]
pub struct FunctionRange {
    pub start: usize,
    pub end: usize,
    pub name: Option<String>,
}

impl DebugInfo {
    /// The source offset of instruction `ip`.
    pub fn position(&self, ip: usize) -> Option<u32> {
        self.positions
            .get(ip)
            .copied()
            .filter(|p| *p != NO_POSITION)
    }

    /// The innermost function whose body holds instruction `ip`, if any
    /// (none: top-level code).
    pub fn function_at(&self, ip: usize) -> Option<&FunctionRange> {
        self.functions
            .iter()
            .filter(|f| f.start <= ip && ip < f.end)
            .max_by_key(|f| f.start)
    }
}

/// One loaded program (a file): its instructions are at `offset`.
pub struct SourceSegment {
    pub offset: usize,
    pub len: usize,
    pub file: String,
    pub source: std::rc::Rc<str>,
    pub info: DebugInfo,
}

/// The source positions of every program in a combined instruction
/// stream (the VM's, or the one a native build lowers).
#[derive(Default)]
pub struct SourceTable {
    segments: Vec<SourceSegment>,
}

impl SourceTable {
    /// `info` describes the instructions loaded at `offset` from `file`.
    pub fn add(&mut self, offset: usize, file: &str, source: &str, info: DebugInfo) {
        let len = info.positions.len();
        self.segments.push(SourceSegment {
            offset,
            len,
            file: file.to_string(),
            source: std::rc::Rc::from(source),
            info,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Instruction `ip` as a stack frame (`name (file:line:col)`), if its
    /// position is known.
    pub fn describe(&self, ip: usize) -> Option<String> {
        let (name, file, pos) = self.locate(ip)?;
        Some(format_frame(name, file, pos))
    }

    /// Where instruction `ip` is: its function's name, its file and
    /// position.
    pub fn locate(&self, ip: usize) -> Option<(Option<&str>, &str, Position)> {
        let seg = self
            .segments
            .iter()
            .rev()
            .find(|s| s.offset <= ip && ip < s.offset + s.len)?;
        let local = ip - seg.offset;
        let pos = Position::of_offset(&seg.source, seg.info.position(local)? as usize);
        let name = seg.info.function_at(local).and_then(|f| f.name.as_deref());
        Some((name, &seg.file, pos))
    }
}

/// A frame as stack traces print it: `name (file:line:col)`, or
/// `file:line:col` for top-level code and anonymous functions.
pub fn format_frame(name: Option<&str>, file: &str, pos: Position) -> String {
    match name {
        Some(name) => format!("{} ({}:{}:{})", name, file, pos.line, pos.col),
        None => format!("{}:{}:{}", file, pos.line, pos.col),
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
