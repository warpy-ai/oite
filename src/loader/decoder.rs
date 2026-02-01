//! Bytecode decoder for loading binary files produced by bootstrap/emitter.ot
//!
//! The binary format uses:
//! - u8 for opcodes and small values
//! - LEB128 (varint) for variable-length integers
//! - Little-endian u32 for addresses
//! - Little-endian f64 for floating point numbers
//! - Varint-prefixed UTF-8 for strings

use crate::vm::opcodes::OpCode;
use crate::vm::value::JsValue;
use std::collections::HashMap;

/// Magic bytes for TSCL bytecode files
pub const MAGIC: &[u8; 4] = b"TSCL";
/// Current bytecode format version
pub const VERSION: u8 = 1;

/// Errors that can occur during bytecode loading
#[derive(Debug)]
pub enum LoaderError {
    /// Unexpected end of file
    UnexpectedEof,
    /// Invalid opcode byte
    InvalidOpcode(u8),
    /// Invalid type tag for PUSH instruction
    InvalidTypeTag(u8),
    /// Invalid UTF-8 in string
    InvalidUtf8(std::string::FromUtf8Error),
    /// Invalid magic bytes (not a TSCL file)
    InvalidMagic,
    /// Unsupported bytecode version
    UnsupportedVersion(u8),
    /// Varint overflow (too many continuation bytes)
    VarintOverflow,
    /// Address not found in mapping (internal error)
    AddressNotFound(u32),
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderError::UnexpectedEof => write!(f, "Unexpected end of file"),
            LoaderError::InvalidOpcode(op) => write!(f, "Invalid opcode: {}", op),
            LoaderError::InvalidTypeTag(tag) => write!(f, "Invalid type tag: {}", tag),
            LoaderError::InvalidUtf8(e) => write!(f, "Invalid UTF-8: {}", e),
            LoaderError::InvalidMagic => write!(f, "Invalid magic bytes (not a TSCL file)"),
            LoaderError::UnsupportedVersion(v) => write!(f, "Unsupported bytecode version: {}", v),
            LoaderError::VarintOverflow => write!(f, "Varint overflow"),
            LoaderError::AddressNotFound(addr) => {
                write!(f, "Address {} not found in mapping", addr)
            }
        }
    }
}

impl std::error::Error for LoaderError {}

/// Bytecode decoder that reads binary files and produces OpCode vectors
pub struct BytecodeDecoder<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BytecodeDecoder<'a> {
    /// Create a new decoder for the given bytes
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Reset position to start (useful for legacy files without header)
    pub fn reset(&mut self) {
        self.pos = 0;
    }

    /// Current position in the byte stream
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Check if we've reached the end
    pub fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Validate and skip the header, returning the version number
    /// Returns Err(InvalidMagic) if magic bytes don't match
    pub fn validate_header(&mut self) -> Result<u8, LoaderError> {
        if self.bytes.len() < 8 {
            return Err(LoaderError::InvalidMagic);
        }

        // Check magic bytes
        if &self.bytes[0..4] != MAGIC {
            return Err(LoaderError::InvalidMagic);
        }

        let version = self.bytes[4];
        if version != VERSION {
            return Err(LoaderError::UnsupportedVersion(version));
        }

        // Skip header (8 bytes: magic + version + reserved)
        self.pos = 8;
        Ok(version)
    }

    /// Read a single byte
    fn read_u8(&mut self) -> Result<u8, LoaderError> {
        if self.pos >= self.bytes.len() {
            return Err(LoaderError::UnexpectedEof);
        }
        let byte = self.bytes[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    /// Read a 32-bit little-endian unsigned integer
    fn read_u32_le(&mut self) -> Result<u32, LoaderError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(LoaderError::UnexpectedEof);
        }
        let value = u32::from_le_bytes([
            self.bytes[self.pos],
            self.bytes[self.pos + 1],
            self.bytes[self.pos + 2],
            self.bytes[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(value)
    }

    /// Read a 64-bit little-endian floating point number
    fn read_f64_le(&mut self) -> Result<f64, LoaderError> {
        if self.pos + 8 > self.bytes.len() {
            return Err(LoaderError::UnexpectedEof);
        }
        let value = f64::from_le_bytes([
            self.bytes[self.pos],
            self.bytes[self.pos + 1],
            self.bytes[self.pos + 2],
            self.bytes[self.pos + 3],
            self.bytes[self.pos + 4],
            self.bytes[self.pos + 5],
            self.bytes[self.pos + 6],
            self.bytes[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(value)
    }

    /// Read a LEB128-encoded variable-length integer
    fn read_varint(&mut self) -> Result<u64, LoaderError> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return Err(LoaderError::VarintOverflow);
            }
        }
        Ok(result)
    }

    /// Read a varint-prefixed UTF-8 string
    fn read_string(&mut self) -> Result<String, LoaderError> {
        let len = self.read_varint()? as usize;
        if self.pos + len > self.bytes.len() {
            return Err(LoaderError::UnexpectedEof);
        }
        let bytes = self.bytes[self.pos..self.pos + len].to_vec();
        self.pos += len;
        String::from_utf8(bytes).map_err(LoaderError::InvalidUtf8)
    }

    /// Decode all instructions from the bytecode
    /// This is a two-pass decoder:
    /// 1. First pass: decode instructions, record byte offset -> instruction index
    /// 2. Second pass: fix up addresses in Jump, JumpIfFalse, MakeClosure
    pub fn decode_all(&mut self) -> Result<Vec<OpCode>, LoaderError> {
        // Try to validate header, but if invalid magic, assume legacy format
        if self.pos == 0 {
            match self.validate_header() {
                Ok(_) => {} // Header validated, continue from position 8
                Err(LoaderError::InvalidMagic) => {
                    self.reset(); // Legacy format, start from beginning
                }
                Err(e) => return Err(e),
            }
        }

        // First pass: decode all instructions, record byte offsets
        // Note: We use absolute file positions (including header) because that's
        // what the emitter writes when computing jump addresses with currentOffset()
        let mut instructions = Vec::new();
        let mut byte_to_instr: HashMap<usize, usize> = HashMap::new();

        while !self.is_eof() {
            let byte_offset = self.pos; // Absolute file position
            let instr_index = instructions.len();
            byte_to_instr.insert(byte_offset, instr_index);

            let op = self.decode_instruction()?;
            instructions.push(op);
        }

        // Second pass: fix up addresses
        for op in &mut instructions {
            match op {
                OpCode::Jump(addr) => {
                    let byte_addr = *addr;
                    *addr = *byte_to_instr
                        .get(&byte_addr)
                        .ok_or(LoaderError::AddressNotFound(byte_addr as u32))?;
                }
                OpCode::JumpIfFalse(addr) => {
                    let byte_addr = *addr;
                    *addr = *byte_to_instr
                        .get(&byte_addr)
                        .ok_or(LoaderError::AddressNotFound(byte_addr as u32))?;
                }
                OpCode::MakeClosure(addr) => {
                    let byte_addr = *addr;
                    *addr = *byte_to_instr
                        .get(&byte_addr)
                        .ok_or(LoaderError::AddressNotFound(byte_addr as u32))?;
                }
                OpCode::Push(JsValue::Function { address, .. }) => {
                    let byte_addr = *address;
                    *address = *byte_to_instr
                        .get(&byte_addr)
                        .ok_or(LoaderError::AddressNotFound(byte_addr as u32))?;
                }
                _ => {}
            }
        }

        Ok(instructions)
    }

    /// Decode a single instruction
    fn decode_instruction(&mut self) -> Result<OpCode, LoaderError> {
        let opcode = self.read_u8()?;

        match opcode {
            // LOAD_THIS
            0 => Ok(OpCode::LoadThis),

            // PUSH with type tag
            1 => {
                let type_tag = self.read_u8()?;
                let value = match type_tag {
                    0 => JsValue::Number(self.read_f64_le()?),
                    1 => JsValue::String(self.read_string()?),
                    2 => JsValue::Boolean(true),
                    3 => JsValue::Boolean(false),
                    4 => JsValue::Null,
                    5 => JsValue::Undefined,
                    _ => return Err(LoaderError::InvalidTypeTag(type_tag)),
                };
                Ok(OpCode::Push(value))
            }

            // Simple arithmetic ops
            2 => Ok(OpCode::Add),
            3 => Ok(OpCode::Sub),
            4 => Ok(OpCode::Mul),
            5 => Ok(OpCode::Div),

            // Print
            6 => Ok(OpCode::Print),

            // Pop
            7 => Ok(OpCode::Pop),

            // Store (variable assignment)
            8 => Ok(OpCode::Store(self.read_string()?)),

            // Load (variable read)
            9 => Ok(OpCode::Load(self.read_string()?)),

            // Drop (remove variable from scope)
            10 => Ok(OpCode::Drop(self.read_string()?)),

            // Call with arg count
            11 => Ok(OpCode::Call(self.read_u8()? as usize)),

            // Return
            12 => Ok(OpCode::Return),

            // Jump (absolute byte address, will be converted to instruction index)
            13 => Ok(OpCode::Jump(self.read_u32_le()? as usize)),

            // NewObject
            14 => Ok(OpCode::NewObject),

            // SetProp
            15 => Ok(OpCode::SetProp(self.read_string()?)),

            // GetProp
            16 => Ok(OpCode::GetProp(self.read_string()?)),

            // Dup
            17 => Ok(OpCode::Dup),

            // Comparison and equality ops
            18 => Ok(OpCode::Eq),   // ===
            19 => Ok(OpCode::EqEq), // ==
            20 => Ok(OpCode::Ne),   // !==
            21 => Ok(OpCode::NeEq), // !=
            22 => Ok(OpCode::Lt),   // <
            23 => Ok(OpCode::LtEq), // <=
            24 => Ok(OpCode::Gt),   // >
            25 => Ok(OpCode::GtEq), // >=

            // Mod
            26 => Ok(OpCode::Mod),

            // Logical ops
            27 => Ok(OpCode::And),
            28 => Ok(OpCode::Or),
            29 => Ok(OpCode::Not),

            // Neg (unary minus)
            30 => Ok(OpCode::Neg),

            // NewArray (emitter doesn't emit size, use 0)
            31 => Ok(OpCode::NewArray(0)),

            // StoreElement
            32 => Ok(OpCode::StoreElement),

            // LoadElement
            33 => Ok(OpCode::LoadElement),

            // JumpIfFalse (absolute byte address)
            34 => Ok(OpCode::JumpIfFalse(self.read_u32_le()? as usize)),

            // 35 is not used

            // Binary operators (36-53)
            36 => Ok(OpCode::Mod),                // %
            37 => Ok(OpCode::Pow),                // ** (need to add to VM)
            38 => Ok(OpCode::Eq),                 // ===
            39 => Ok(OpCode::EqEq),               // ==
            40 => Ok(OpCode::Ne),                 // !==
            41 => Ok(OpCode::NeEq),               // !=
            42 => Ok(OpCode::Lt),                 // <
            43 => Ok(OpCode::LtEq),               // <=
            44 => Ok(OpCode::Gt),                 // >
            45 => Ok(OpCode::GtEq),               // >=
            46 => Ok(OpCode::ShiftLeft),          // <<
            47 => Ok(OpCode::ShiftRight),         // >>
            48 => Ok(OpCode::ShiftRightUnsigned), // >>>
            49 => Ok(OpCode::BitAnd),             // &
            50 => Ok(OpCode::Xor),                // ^
            51 => Ok(OpCode::BitOr),              // |
            52 => Ok(OpCode::And),                // &&
            53 => Ok(OpCode::Or),                 // ||

            // CallMethod with name and arg count
            54 => {
                let name = self.read_string()?;
                let arg_count = self.read_u8()? as usize;
                Ok(OpCode::CallMethod(name, arg_count))
            }

            // Require
            55 => Ok(OpCode::Require),

            // MakeClosure with address and param names (param names are discarded)
            56 => {
                let addr = self.read_u32_le()? as usize;
                let param_count = self.read_u8()?;
                // Read and discard parameter names
                for _ in 0..param_count {
                    let _ = self.read_string()?;
                }
                Ok(OpCode::MakeClosure(addr))
            }

            // Construct with arg count
            57 => Ok(OpCode::Construct(self.read_u8()? as usize)),

            // StoreLocal (indexed local variable store)
            58 => Ok(OpCode::StoreLocal(self.read_u32_le()?)),

            // LoadLocal (indexed local variable load)
            59 => Ok(OpCode::LoadLocal(self.read_u32_le()?)),

            // Halt
            255 => Ok(OpCode::Halt),

            _ => Err(LoaderError::InvalidOpcode(opcode)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_varint_single_byte() {
        let bytes = vec![42];
        let mut decoder = BytecodeDecoder::new(&bytes);
        assert_eq!(decoder.read_varint().unwrap(), 42);
    }

    #[test]
    fn test_read_varint_multi_byte() {
        // 300 = 0b100101100 = [0xAC, 0x02] in LEB128
        let bytes = vec![0xAC, 0x02];
        let mut decoder = BytecodeDecoder::new(&bytes);
        assert_eq!(decoder.read_varint().unwrap(), 300);
    }

    #[test]
    fn test_read_string() {
        // String "hi" = [2, 'h', 'i']
        let bytes = vec![2, b'h', b'i'];
        let mut decoder = BytecodeDecoder::new(&bytes);
        assert_eq!(decoder.read_string().unwrap(), "hi");
    }

    #[test]
    fn test_decode_push_number() {
        // PUSH NUMBER 42.0
        let mut bytes = vec![1, 0]; // opcode, type tag
        bytes.extend_from_slice(&42.0_f64.to_le_bytes());
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Push(JsValue::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected Push(Number)"),
        }
    }

    #[test]
    fn test_decode_push_string() {
        // PUSH STRING "hi"
        let bytes = vec![1, 1, 2, b'h', b'i'];
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Push(JsValue::String(s)) => assert_eq!(s, "hi"),
            _ => panic!("Expected Push(String)"),
        }
    }

    #[test]
    fn test_decode_push_boolean() {
        // PUSH TRUE
        let bytes = vec![1, 2];
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Push(JsValue::Boolean(true)) => {}
            _ => panic!("Expected Push(Boolean(true))"),
        }

        // PUSH FALSE
        let bytes = vec![1, 3];
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Push(JsValue::Boolean(false)) => {}
            _ => panic!("Expected Push(Boolean(false))"),
        }
    }

    #[test]
    fn test_decode_push_null() {
        let bytes = vec![1, 4];
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Push(JsValue::Null) => {}
            _ => panic!("Expected Push(Null)"),
        }
    }

    #[test]
    fn test_decode_simple_ops() {
        let ops = vec![
            (2, "Add"),
            (3, "Sub"),
            (4, "Mul"),
            (5, "Div"),
            (7, "Pop"),
            (12, "Return"),
            (14, "NewObject"),
            (17, "Dup"),
            (255, "Halt"),
        ];

        for (byte, name) in ops {
            let bytes = vec![byte];
            let mut decoder = BytecodeDecoder::new(&bytes);
            let op = decoder.decode_instruction();
            assert!(op.is_ok(), "Failed to decode {}", name);
        }
    }

    #[test]
    fn test_decode_store() {
        // STORE "x"
        let bytes = vec![8, 1, b'x'];
        let mut decoder = BytecodeDecoder::new(&bytes);
        let op = decoder.decode_instruction().unwrap();
        match op {
            OpCode::Store(name) => assert_eq!(name, "x"),
            _ => panic!("Expected Store"),
        }
    }

    #[test]
    fn test_header_validation() {
        let mut bytes = b"TSCL".to_vec();
        bytes.push(VERSION); // version
        bytes.extend_from_slice(&[0, 0, 0]); // reserved
        bytes.push(255); // HALT

        let mut decoder = BytecodeDecoder::new(&bytes);
        let version = decoder.validate_header().unwrap();
        assert_eq!(version, VERSION);
        assert_eq!(decoder.position(), 8);
    }

    #[test]
    fn test_invalid_magic() {
        let bytes = b"NOTV1234";
        let mut decoder = BytecodeDecoder::new(bytes);
        assert!(matches!(
            decoder.validate_header(),
            Err(LoaderError::InvalidMagic)
        ));
    }
}
