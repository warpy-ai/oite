//! Source sites: which source position each IR operation came from.

use super::*;

/// The stack frame (`f (file:2:14)`) of each instruction of a program,
/// numbered as `IrModule::sites`.
pub struct Sites {
    table: crate::compiler::SourceTable,
    ids: std::cell::RefCell<SiteIds>,
    /// Mark every op, not only those that can throw (debug info).
    pub(super) every_op: bool,
}

#[derive(Default)]
pub(super) struct SiteIds {
    by_ip: HashMap<usize, u32>,
    by_frame: HashMap<String, u32>,
    sites: Vec<crate::ir::Site>,
}

/// `IrOp::Loc` of code whose position is unknown.
pub const NO_SITE: u32 = u32::MAX;

impl Sites {
    pub fn new(table: crate::compiler::SourceTable) -> Self {
        Self {
            table,
            ids: Default::default(),
            every_op: false,
        }
    }

    /// Mark every op with its position, for debug info.
    pub fn every_op(mut self) -> Self {
        self.every_op = true;
        self
    }

    /// The site of instruction `ip`.
    pub(super) fn site(&self, ip: usize) -> u32 {
        let mut ids = self.ids.borrow_mut();
        if let Some(&id) = ids.by_ip.get(&ip) {
            return id;
        }
        let id = match self.table.locate(ip) {
            Some((name, file, pos)) => {
                let frame = crate::compiler::format_frame(name, file, pos);
                match ids.by_frame.get(&frame) {
                    Some(&id) => id,
                    None => {
                        let id = ids.sites.len() as u32;
                        ids.sites.push(crate::ir::Site {
                            frame: frame.clone(),
                            name: name.map(str::to_string),
                            file: file.to_string(),
                            line: pos.line as u32,
                            col: pos.col as u32,
                        });
                        ids.by_frame.insert(frame, id);
                        id
                    }
                }
            }
            None => NO_SITE,
        };
        ids.by_ip.insert(ip, id);
        id
    }

    pub(super) fn take(&self) -> Vec<crate::ir::Site> {
        std::mem::take(&mut self.ids.borrow_mut().sites)
    }
}
