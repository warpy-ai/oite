//! Stack traces for uncaught errors.
//!
//! An exception records where it passes on its way out: the instruction
//! in the function that throws it, then the call instruction in each
//! function it unwinds through, up to the one that catches it (or the
//! top level). A rethrow of the same value continues its trace. This is
//! what the native backends record too (`ot_trace`), so every backend
//! prints the same frames:
//!
//! ```text
//! Uncaught TypeError: Cannot read properties of undefined (reading 'y')
//!     at f (file.ot:2:14)
//!     at file.ot:4:1
//! ```

use super::VM;
use super::value::JsValue;
use crate::compiler::{DebugInfo, Position};
use std::rc::Rc;

/// Positions of one loaded program (a file), at `offset` in the VM's
/// instructions.
pub struct DebugSegment {
    pub offset: usize,
    pub len: usize,
    pub file: String,
    pub source: Rc<str>,
    pub info: DebugInfo,
}

/// A frame as stack traces print it: `name (file:line:col)`, or
/// `file:line:col` for top-level code and anonymous functions.
pub fn format_frame(name: Option<&str>, file: &str, pos: Position) -> String {
    match name {
        Some(name) => format!("{} ({}:{}:{})", name, file, pos.line, pos.col),
        None => format!("{}:{}:{}", file, pos.line, pos.col),
    }
}

impl VM {
    /// Record the positions of `info`, a program of `len` instructions
    /// loaded at `offset` from `file`.
    pub fn add_debug_info(&mut self, offset: usize, file: &str, source: &str, info: DebugInfo) {
        let len = info.positions.len();
        self.debug_segments.push(DebugSegment {
            offset,
            len,
            file: file.to_string(),
            source: Rc::from(source),
            info,
        });
    }

    /// Instruction `ip` as a stack frame, if its position is known.
    pub fn describe_ip(&self, ip: usize) -> Option<String> {
        let seg = self
            .debug_segments
            .iter()
            .rev()
            .find(|s| s.offset <= ip && ip < s.offset + s.len)?;
        let local = ip - seg.offset;
        let pos = Position::of_offset(&seg.source, seg.info.position(local)? as usize);
        let name = seg.info.function_at(local).and_then(|f| f.name.as_deref());
        Some(format_frame(name, &seg.file, pos))
    }

    /// `exception` leaves the frames above `catch_depth` (the call stack
    /// length once it is caught): record where it is in each, and in the
    /// function that catches it.
    pub(super) fn record_trace(&mut self, exception: &JsValue, catch_depth: usize) {
        if self.debug_segments.is_empty() {
            return;
        }
        let rethrow = self
            .traced_exception
            .as_ref()
            .is_some_and(|t| same_value(t, exception));
        if !rethrow {
            self.exception_trace.clear();
            self.traced_exception = Some(exception.clone());
        }
        let mut depth = self.call_stack.len();
        let mut ip = self.ip;
        // A rethrow continues from where the catching function was recorded.
        let mut record = !rethrow;
        loop {
            if record && self.exception_trace.len() < MAX_FRAMES {
                self.exception_trace.push(ip);
            }
            record = true;
            if depth <= catch_depth.max(1) {
                break;
            }
            let ret = self.call_stack[depth - 1].return_address;
            if ret == usize::MAX || ret == 0 {
                break;
            }
            ip = ret - 1;
            depth -= 1;
        }
    }

    /// The recorded frames of `exception`, one `    at ...` line each.
    pub(super) fn trace_lines(&self, exception: &JsValue) -> Vec<String> {
        if !self
            .traced_exception
            .as_ref()
            .is_some_and(|t| same_value(t, exception))
        {
            return Vec::new();
        }
        self.exception_trace
            .iter()
            .filter_map(|&ip| self.describe_ip(ip))
            .map(|frame| format!("    at {}", frame))
            .collect()
    }
}

/// Frames kept, as in V8 (`Error.stackTraceLimit`).
pub const MAX_FRAMES: usize = 10;

/// The same thrown value: the same object, or an equal primitive.
fn same_value(a: &JsValue, b: &JsValue) -> bool {
    match (a, b) {
        (JsValue::Object(x), JsValue::Object(y)) => x == y,
        (JsValue::String(x), JsValue::String(y)) => x == y,
        (JsValue::Number(x), JsValue::Number(y)) => x == y,
        (JsValue::Boolean(x), JsValue::Boolean(y)) => x == y,
        (JsValue::Null, JsValue::Null) | (JsValue::Undefined, JsValue::Undefined) => true,
        _ => false,
    }
}
