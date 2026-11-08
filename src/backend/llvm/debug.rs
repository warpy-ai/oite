//! DWARF debug info for native binaries (`oitec build --debug`).
//!
//! A compile unit, a subprogram for each compiled function, and a debug
//! location for the code of each `IrOp::Loc`, so debuggers and
//! symbolizers map machine code back to Oite source lines.

use std::collections::HashMap;
use std::path::Path;

use llvm_sys::core::*;
use llvm_sys::debuginfo::*;
use llvm_sys::prelude::*;

use crate::ir::{IrFunction, IrOp, Site};

pub struct DebugBuilder {
    builder: LLVMDIBuilderRef,
    context: LLVMContextRef,
    files: HashMap<String, LLVMMetadataRef>,
    /// `void ()`: compiled functions get no parameter types.
    subroutine_ty: LLVMMetadataRef,
    /// Lexical block files: code of one file inside a function of another
    /// (top-level code runs the core library's, then the program's).
    block_files: HashMap<(usize, String), LLVMMetadataRef>,
}

/// Where a function's code is, for its debug locations.
pub struct FunctionScope {
    pub subprogram: LLVMMetadataRef,
    file: String,
    line: u32,
}

impl DebugBuilder {
    /// Debug info for `module`, whose main program is `main_file`.
    pub unsafe fn new(module: LLVMModuleRef, context: LLVMContextRef, main_file: &str) -> Self {
        unsafe {
            let builder = LLVMCreateDIBuilder(module);
            let mut this = Self {
                builder,
                context,
                files: HashMap::new(),
                subroutine_ty: std::ptr::null_mut(),
                block_files: HashMap::new(),
            };
            let file = this.file(main_file);
            let producer = "oitec";
            LLVMDIBuilderCreateCompileUnit(
                builder,
                LLVMDWARFSourceLanguage::LLVMDWARFSourceLanguageC,
                file,
                producer.as_ptr() as _,
                producer.len(),
                0,
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                0,
                LLVMDWARFEmissionKind::LLVMDWARFEmissionKindFull,
                0,
                0,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            this.subroutine_ty = LLVMDIBuilderCreateSubroutineType(
                builder,
                file,
                std::ptr::null_mut(),
                0,
                LLVMDIFlagZero,
            );

            let i32_ty = LLVMInt32TypeInContext(context);
            for (key, value) in [
                ("Debug Info Version", LLVMDebugMetadataVersion()),
                ("Dwarf Version", 4),
            ] {
                let v = LLVMValueAsMetadata(LLVMConstInt(i32_ty, value as u64, 0));
                LLVMAddModuleFlag(
                    module,
                    llvm_sys::LLVMModuleFlagBehavior::LLVMModuleFlagBehaviorWarning,
                    key.as_ptr() as _,
                    key.len(),
                    v,
                );
            }
            this
        }
    }

    unsafe fn file(&mut self, path: &str) -> LLVMMetadataRef {
        if let Some(&f) = self.files.get(path) {
            return f;
        }
        let p = Path::new(path);
        let name = p
            .file_name()
            .map_or(path.to_string(), |n| n.to_string_lossy().into_owned());
        let dir = p
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let f = unsafe {
            LLVMDIBuilderCreateFile(
                self.builder,
                name.as_ptr() as _,
                name.len(),
                dir.as_ptr() as _,
                dir.len(),
            )
        };
        self.files.insert(path.to_string(), f);
        f
    }

    /// A subprogram for `func` (the LLVM function `f`, named `linkage`),
    /// at its first known position.
    pub unsafe fn function(
        &mut self,
        func: &IrFunction,
        f: LLVMValueRef,
        linkage: &str,
        sites: &[Site],
    ) -> Option<FunctionScope> {
        let first = func
            .blocks
            .iter()
            .flat_map(|b| &b.ops)
            .find_map(|op| match op {
                IrOp::Loc(n) => sites.get(*n as usize),
                _ => None,
            })?;
        let name = match &first.name {
            Some(n) if func.name != "main" => n.clone(),
            _ => linkage.to_string(),
        };
        unsafe {
            let file = self.file(&first.file);
            let sp = LLVMDIBuilderCreateFunction(
                self.builder,
                file,
                name.as_ptr() as _,
                name.len(),
                // No linkage name: debuggers show the Oite name.
                std::ptr::null(),
                0,
                file,
                first.line,
                self.subroutine_ty,
                1,
                1,
                first.line,
                LLVMDIFlagZero,
                0,
            );
            LLVMSetSubprogram(f, sp);
            Some(FunctionScope {
                subprogram: sp,
                file: first.file.clone(),
                line: first.line,
            })
        }
    }

    /// The location of `site` in `scope`'s function (`None`: where the
    /// function starts).
    pub unsafe fn location(
        &mut self,
        scope: &FunctionScope,
        site: Option<&Site>,
    ) -> LLVMMetadataRef {
        unsafe {
            let Some(site) = site else {
                return LLVMDIBuilderCreateDebugLocation(
                    self.context,
                    scope.line,
                    0,
                    scope.subprogram,
                    std::ptr::null_mut(),
                );
            };
            let lexical = if site.file == scope.file {
                scope.subprogram
            } else {
                let key = (scope.subprogram as usize, site.file.clone());
                match self.block_files.get(&key) {
                    Some(&b) => b,
                    None => {
                        let file = self.file(&site.file);
                        let b = LLVMDIBuilderCreateLexicalBlockFile(
                            self.builder,
                            scope.subprogram,
                            file,
                            0,
                        );
                        self.block_files.insert(key, b);
                        b
                    }
                }
            };
            LLVMDIBuilderCreateDebugLocation(
                self.context,
                site.line,
                site.col,
                lexical,
                std::ptr::null_mut(),
            )
        }
    }

    pub unsafe fn finalize(self) {
        unsafe {
            LLVMDIBuilderFinalize(self.builder);
            LLVMDisposeDIBuilder(self.builder);
        }
    }
}
