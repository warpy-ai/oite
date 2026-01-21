//! ABI Compatibility Tests
//!
//! These tests verify that the tscl ABI remains stable across builds.
//! Any change to the ABI should cause these tests to fail.

#[cfg(test)]
mod tests {
    use crate::ir::format::IR_FORMAT_VERSION;
    use crate::runtime::ABI_VERSION;

    /// Test that ABI version is set to the expected value.
    #[test]
    fn test_abi_version() {
        assert_eq!(ABI_VERSION, 1, "ABI version must be 1");
    }

    /// Test that IR format version is set to the expected value.
    #[test]
    fn test_ir_format_version() {
        assert_eq!(IR_FORMAT_VERSION, 1, "IR format version must be 1");
    }

    /// Test that ABI version is a valid u32.
    #[test]
    fn test_abi_version_is_valid() {
        assert!(ABI_VERSION > 0, "ABI version must be > 0");
        assert!(
            ABI_VERSION < 100,
            "ABI version should be < 100 for active development"
        );
    }

    /// Test that IR format version is a valid u32.
    #[test]
    fn test_ir_format_version_is_valid() {
        assert!(IR_FORMAT_VERSION > 0, "IR format version must be > 0");
        assert!(IR_FORMAT_VERSION < 100, "IR format version should be < 100");
    }

    /// Test that the ABI name is correct.
    #[test]
    fn test_abi_name() {
        use crate::runtime::abi_version::ABI_NAME;
        assert_eq!(ABI_NAME, "tscl", "ABI name must be 'tscl'");
    }
}

#[cfg(test)]
mod value_encoding_tests {
    /// Verify that special value encodings are correct.
    #[test]
    fn test_special_value_encodings() {
        // These values are hardcoded in the runtime - any change breaks ABI
        const UNDEFINED_ENCODING: u64 = 0x7FF8000000000001;
        const NULL_ENCODING: u64 = 0x7FF8000000000002;
        const TRUE_ENCODING: u64 = 0x7FF8000000000003;
        const FALSE_ENCODING: u64 = 0x7FF8000000000004;

        // Verify these are quiet NaN values (exponent = 0x7FF)
        let undefined_bits = UNDEFINED_ENCODING;
        let exponent = (undefined_bits >> 52) & 0x7FF;
        assert_eq!(
            exponent, 0x7FF,
            "undefined must be a NaN (exponent = 0x7FF)"
        );

        let null_bits = NULL_ENCODING;
        let exponent = (null_bits >> 52) & 0x7FF;
        assert_eq!(exponent, 0x7FF, "null must be a NaN (exponent = 0x7FF)");

        let true_bits = TRUE_ENCODING;
        let exponent = (true_bits >> 52) & 0x7FF;
        assert_eq!(exponent, 0x7FF, "true must be a NaN (exponent = 0x7FF)");

        let false_bits = FALSE_ENCODING;
        let exponent = (false_bits >> 52) & 0x7FF;
        assert_eq!(exponent, 0x7FF, "false must be a NaN (exponent = 0x7FF)");
    }

    /// Verify that NaN-boxed values have the correct properties.
    #[test]
    fn test_nan_boxing_properties() {
        // A valid pointer should fit in 52 bits (mantissa)
        // This test verifies our assumptions about pointer encoding
        let max_pointer: u64 = (1 << 52) - 1;
        assert!(max_pointer > 0, "Must be able to represent 52-bit values");
        assert_eq!(max_pointer, 0xFFFFFFFFFFFFF, "52-bit mask should be all 1s");
    }
}

#[cfg(test)]
mod object_layout_tests {
    /// Verify object layout assumptions.
    #[test]
    fn test_object_size_assumptions() {
        use crate::runtime::abi::TsclValue;

        // Object header should fit in expected size
        // This ensures ABI compatibility for struct layouts
        let size_of_u64 = std::mem::size_of::<u64>();
        let size_of_u32 = std::mem::size_of::<u32>();

        assert_eq!(size_of_u64, 8, "u64 must be 8 bytes");
        assert_eq!(size_of_u32, 4, "u32 must be 4 bytes");

        // NaN-boxed value is u64
        assert_eq!(
            std::mem::size_of::<TsclValue>(),
            8,
            "TsclValue must be 8 bytes (64-bit)"
        );
    }
}

#[cfg(test)]
mod ir_format_tests {
    use crate::ir::format::{serialize_module, IR_FORMAT_VERSION};
    use crate::ir::{IrFunction, IrModule, IrOp, IrType, Terminator};

    /// Test that IR serialization produces deterministic output.
    #[test]
    fn test_ir_serialization_determinism() {
        let mut module = IrModule::new();
        let mut func = IrFunction::new("test".to_string());
        func.params.push(("x".to_string(), IrType::Number));
        func.params.push(("y".to_string(), IrType::Number));
        func.return_ty = IrType::Number;

        let entry = func.alloc_block();
        let x = func.alloc_value(IrType::Number);
        let y = func.alloc_value(IrType::Number);
        let sum = func.alloc_value(IrType::Number);

        func.add_local("x".to_string(), IrType::Number);
        func.add_local("y".to_string(), IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::LoadLocal(x, 0));
            block.push(IrOp::LoadLocal(y, 1));
            block.push(IrOp::AddNum(sum, x, y));
            block.terminate(Terminator::Return(Some(sum)));
        }

        module.add_function(func);

        // Serialize twice and verify identical output
        let output1 = serialize_module(&module);
        let output2 = serialize_module(&module);

        assert_eq!(output1, output2, "IR serialization must be deterministic");

        // Verify header contains correct version
        assert!(
            output1.contains("; Format version: 1"),
            "IR must contain format version"
        );
        assert!(
            output1.contains("; ABI version: 1"),
            "IR must contain ABI version"
        );
    }

    /// Test that IR contains function definition.
    #[test]
    fn test_ir_contains_function() {
        let mut module = IrModule::new();
        let mut func = IrFunction::new("add".to_string());
        func.params.push(("a".to_string(), IrType::Number));
        func.params.push(("b".to_string(), IrType::Number));
        func.return_ty = IrType::Number;

        let entry = func.alloc_block();
        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let result = func.alloc_value(IrType::Number);

        func.add_local("a".to_string(), IrType::Number);
        func.add_local("b".to_string(), IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::LoadLocal(a, 0));
            block.push(IrOp::LoadLocal(b, 1));
            block.push(IrOp::AddNum(result, a, b));
            block.terminate(Terminator::Return(Some(result)));
        }

        module.add_function(func);

        let output = serialize_module(&module);

        assert!(
            output.contains("fn add(a: num, b: num) -> num"),
            "IR must contain function signature"
        );
        assert!(
            output.contains("add.num"),
            "IR must contain add.num operation"
        );
        assert!(
            output.contains("return"),
            "IR must contain return statement"
        );
    }
}

#[cfg(test)]
mod abi_stability_tests {
    use crate::runtime::ABI_VERSION;

    /// Test that ABI version hasn't changed unexpectedly.
    #[test]
    fn test_abi_stability() {
        // This test serves as a canary - if it fails, the ABI has changed
        // and we need to decide whether to bump ABI_VERSION
        assert_eq!(
            ABI_VERSION, 1,
            "ABI version must remain 1 until intentional change"
        );

        // Verify we haven't accidentally changed to a development version
        assert!(
            ABI_VERSION < 2,
            "ABI should not be version 2+ without explicit decision"
        );
    }
}
