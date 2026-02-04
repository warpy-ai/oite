//! NaN-boxed value representation for native code interop
//!
//! NaN-boxing encodes JavaScript values in 64 bits by exploiting IEEE 754's
//! quiet NaN space. This provides:
//! - Immediate numbers (no indirection)
//! - Fast type checking (bit pattern comparison)
//! - Uniform 64-bit representation for registers and stack
//!
//! Layout:
//! - Numbers: Regular IEEE 754 f64 (if not a quiet NaN)
//! - Tagged values: 0x7FFC_xxxx_xxxx_xxxx (quiet NaN + payload)
//!
//! Tag encoding (bits 47-50 of the NaN payload):
//! - 0x0: Pointer (object/array/function/string) in bits 0-47
//! - 0x1: Boolean (bit 0: 0=false, 1=true)
//! - 0x2: Null
//! - 0x3: Undefined
//! - 0x4: Reserved (future: Symbol)

use super::heap::HeapPtr;

/// Quiet NaN with signal bit clear and all exponent bits set.
/// Any value with these bits set (and mantissa non-zero) is a quiet NaN.
const QNAN: u64 = 0x7FFC_0000_0000_0000;

/// Mask for the tag bits (bits 48-51)
const TAG_MASK: u64 = 0x000F_0000_0000_0000;

/// Mask for the pointer/payload (bits 0-47)
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Tag values (shifted to bits 48-51)
const TAG_POINTER: u64 = 0x0000_0000_0000_0000;
const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
const TAG_NULL: u64 = 0x0002_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;

/// A NaN-boxed value that can represent any tscl runtime value.
///
/// This is the primary value type used by native-compiled code.
/// It's designed to be:
/// - Passed in registers (single u64)
/// - Cheaply copied (Copy trait)
/// - Fast to type-check (bit pattern comparison)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TsclValue {
    bits: u64,
}

impl TsclValue {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create a number value from f64.
    #[inline]
    pub fn number(n: f64) -> Self {
        let bits = n.to_bits();
        // If it's a NaN, canonicalize to a quiet NaN to avoid confusion
        if n.is_nan() {
            Self {
                bits: f64::NAN.to_bits(),
            }
        } else {
            Self { bits }
        }
    }

    /// Create a boolean value.
    #[inline]
    pub fn boolean(b: bool) -> Self {
        Self {
            bits: QNAN | TAG_BOOLEAN | (b as u64),
        }
    }

    /// Create the null value.
    #[inline]
    pub fn null() -> Self {
        Self {
            bits: QNAN | TAG_NULL,
        }
    }

    /// Create the undefined value.
    #[inline]
    pub fn undefined() -> Self {
        Self {
            bits: QNAN | TAG_UNDEFINED,
        }
    }

    /// Create a pointer value (object, array, function, or string).
    ///
    /// # Safety
    /// The pointer must be valid and fit in 48 bits.
    #[inline]
    pub fn pointer(ptr: HeapPtr) -> Self {
        let addr = ptr.as_usize();
        debug_assert!(addr <= PAYLOAD_MASK as usize, "Pointer exceeds 48 bits");
        Self {
            bits: QNAN | TAG_POINTER | (addr as u64),
        }
    }

    /// Create from raw bits (for deserialization or FFI).
    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self { bits }
    }

    /// Get the raw bits (for serialization or FFI).
    #[inline]
    pub const fn to_bits(self) -> u64 {
        self.bits
    }

    // =========================================================================
    // Type Checking
    // =========================================================================

    /// Check if this value is a number (not a NaN-boxed tagged value).
    #[inline]
    pub fn is_number(self) -> bool {
        // It's a number if it's not a quiet NaN with our tag pattern
        (self.bits & QNAN) != QNAN || self.bits == f64::NAN.to_bits()
    }

    /// Check if this value is a pointer (object, array, function, or string).
    #[inline]
    pub fn is_pointer(self) -> bool {
        (self.bits & (QNAN | TAG_MASK)) == (QNAN | TAG_POINTER) && (self.bits & PAYLOAD_MASK) != 0
    }

    /// Check if this value is a boolean.
    #[inline]
    pub fn is_boolean(self) -> bool {
        (self.bits & (QNAN | TAG_MASK)) == (QNAN | TAG_BOOLEAN)
    }

    /// Check if this value is null.
    #[inline]
    pub fn is_null(self) -> bool {
        self.bits == (QNAN | TAG_NULL)
    }

    /// Check if this value is undefined.
    #[inline]
    pub fn is_undefined(self) -> bool {
        self.bits == (QNAN | TAG_UNDEFINED)
    }

    /// Check if this value is falsy (undefined, null, false, 0, NaN, "").
    #[inline]
    pub fn is_falsy(self) -> bool {
        self.is_undefined()
            || self.is_null()
            || (self.is_boolean() && !self.as_boolean_unchecked())
            || (self.is_number() && {
                let n = self.as_number_unchecked();
                n == 0.0 || n.is_nan()
            })
    }

    // =========================================================================
    // Value Extraction
    // =========================================================================

    /// Get the number value, or None if not a number.
    #[inline]
    pub fn as_number(self) -> Option<f64> {
        if self.is_number() {
            Some(f64::from_bits(self.bits))
        } else {
            None
        }
    }

    /// Get the number value without type checking.
    ///
    /// # Safety
    /// Only call if `is_number()` returns true.
    #[inline]
    pub fn as_number_unchecked(self) -> f64 {
        f64::from_bits(self.bits)
    }

    /// Get the boolean value, or None if not a boolean.
    #[inline]
    pub fn as_boolean(self) -> Option<bool> {
        if self.is_boolean() {
            Some((self.bits & 1) != 0)
        } else {
            None
        }
    }

    /// Get the boolean value without type checking.
    #[inline]
    pub fn as_boolean_unchecked(self) -> bool {
        (self.bits & 1) != 0
    }

    /// Get the pointer value, or None if not a pointer.
    #[inline]
    pub fn as_pointer(self) -> Option<HeapPtr> {
        if self.is_pointer() {
            Some(HeapPtr::from_usize((self.bits & PAYLOAD_MASK) as usize))
        } else {
            None
        }
    }

    /// Get the pointer value without type checking.
    #[inline]
    pub fn as_pointer_unchecked(self) -> HeapPtr {
        HeapPtr::from_usize((self.bits & PAYLOAD_MASK) as usize)
    }

    // =========================================================================
    // Arithmetic Operations (for native stubs)
    // =========================================================================

    /// Add two values (number + number, or string concatenation).
    #[inline]
    pub fn add(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::number(self.as_number_unchecked() + other.as_number_unchecked())
        } else {
            // Fall back to runtime stub for string concat, etc.
            Self::undefined() // Placeholder - actual impl in stubs.rs
        }
    }

    /// Subtract two numbers.
    #[inline]
    pub fn sub(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::number(self.as_number_unchecked() - other.as_number_unchecked())
        } else {
            Self::number(f64::NAN)
        }
    }

    /// Multiply two numbers.
    #[inline]
    pub fn mul(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::number(self.as_number_unchecked() * other.as_number_unchecked())
        } else {
            Self::number(f64::NAN)
        }
    }

    /// Divide two numbers.
    #[inline]
    pub fn div(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::number(self.as_number_unchecked() / other.as_number_unchecked())
        } else {
            Self::number(f64::NAN)
        }
    }

    /// Strict equality (===).
    #[inline]
    pub fn strict_eq(self, other: Self) -> Self {
        // For numbers, need special NaN handling
        if self.is_number() && other.is_number() {
            let a = self.as_number_unchecked();
            let b = other.as_number_unchecked();
            // NaN !== NaN
            if a.is_nan() || b.is_nan() {
                return Self::boolean(false);
            }
            return Self::boolean(a == b);
        }
        // For non-numbers, bit equality works
        Self::boolean(self.bits == other.bits)
    }

    /// Less than comparison.
    #[inline]
    pub fn lt(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::boolean(self.as_number_unchecked() < other.as_number_unchecked())
        } else {
            Self::boolean(false)
        }
    }

    /// Greater than comparison.
    #[inline]
    pub fn gt(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::boolean(self.as_number_unchecked() > other.as_number_unchecked())
        } else {
            Self::boolean(false)
        }
    }

    /// Less than or equal comparison.
    #[inline]
    pub fn lte(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::boolean(self.as_number_unchecked() <= other.as_number_unchecked())
        } else {
            Self::boolean(false)
        }
    }

    /// Greater than or equal comparison.
    #[inline]
    pub fn gte(self, other: Self) -> Self {
        if self.is_number() && other.is_number() {
            Self::boolean(self.as_number_unchecked() >= other.as_number_unchecked())
        } else {
            Self::boolean(false)
        }
    }
}

impl std::fmt::Debug for TsclValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_number() {
            write!(f, "Number({})", self.as_number_unchecked())
        } else if self.is_boolean() {
            write!(f, "Boolean({})", self.as_boolean_unchecked())
        } else if self.is_null() {
            write!(f, "Null")
        } else if self.is_undefined() {
            write!(f, "Undefined")
        } else if self.is_pointer() {
            write!(f, "Pointer({:p})", self.as_pointer_unchecked().as_ptr())
        } else {
            write!(f, "Unknown(0x{:016x})", self.bits)
        }
    }
}

impl Default for TsclValue {
    fn default() -> Self {
        Self::undefined()
    }
}

// =========================================================================
// Conversion from/to JsValue (for VM interop)
// Only included when vm module is available (not in standalone staticlib builds)
// =========================================================================

#[cfg(feature = "vm_interop")]
mod vm_interop {
    use super::*;
    use crate::vm::value::JsValue;

    impl TsclValue {
        /// Convert from the VM's JsValue to TsclValue.
        ///
        /// This is used when transitioning from interpreted to native code.
        pub fn from_js_value(val: &JsValue, _heap_base: *mut u8) -> Self {
            match val {
                JsValue::Number(n) => Self::number(*n),
                JsValue::Boolean(b) => Self::boolean(*b),
                JsValue::Null => Self::null(),
                JsValue::Undefined => Self::undefined(),
                JsValue::Object(idx)
                | JsValue::Function {
                    address: _,
                    env: Some(idx),
                } => Self::pointer(HeapPtr::from_usize(*idx)),
                JsValue::Function { address, env: None } => {
                    Self::pointer(HeapPtr::from_usize(*address))
                }
                JsValue::String(_s) => Self::undefined(),
                JsValue::NativeFunction(idx) => {
                    Self::pointer(HeapPtr::from_usize(*idx | 0x8000_0000_0000))
                }
                JsValue::Accessor(_, _) => Self::undefined(),
                JsValue::Promise(_) => Self::undefined(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_roundtrip() {
        let values = [
            0.0,
            1.0,
            -1.0,
            3.14159,
            f64::MAX,
            f64::MIN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for n in values {
            let v = TsclValue::number(n);
            assert!(v.is_number(), "Expected number for {}", n);
            assert_eq!(v.as_number(), Some(n), "Roundtrip failed for {}", n);
        }
    }

    #[test]
    fn test_boolean() {
        let t = TsclValue::boolean(true);
        let f = TsclValue::boolean(false);

        assert!(t.is_boolean());
        assert!(f.is_boolean());
        assert_eq!(t.as_boolean(), Some(true));
        assert_eq!(f.as_boolean(), Some(false));
        assert!(!t.is_number());
        assert!(!f.is_number());
    }

    #[test]
    fn test_null_undefined() {
        let null = TsclValue::null();
        let undef = TsclValue::undefined();

        assert!(null.is_null());
        assert!(!null.is_undefined());
        assert!(undef.is_undefined());
        assert!(!undef.is_null());
        assert!(!null.is_number());
        assert!(!undef.is_number());
    }

    #[test]
    fn test_arithmetic() {
        let a = TsclValue::number(10.0);
        let b = TsclValue::number(3.0);

        assert_eq!(a.add(b).as_number(), Some(13.0));
        assert_eq!(a.sub(b).as_number(), Some(7.0));
        assert_eq!(a.mul(b).as_number(), Some(30.0));
        // Division result is approximate
        let div = a.div(b).as_number().unwrap();
        assert!((div - 3.333333).abs() < 0.001);
    }

    #[test]
    fn test_comparison() {
        let a = TsclValue::number(5.0);
        let b = TsclValue::number(10.0);

        assert_eq!(a.lt(b).as_boolean(), Some(true));
        assert_eq!(b.lt(a).as_boolean(), Some(false));
        assert_eq!(a.gt(b).as_boolean(), Some(false));
        assert_eq!(b.gt(a).as_boolean(), Some(true));
    }

    #[test]
    fn test_strict_equality() {
        let a = TsclValue::number(5.0);
        let b = TsclValue::number(5.0);
        let c = TsclValue::number(6.0);

        assert_eq!(a.strict_eq(b).as_boolean(), Some(true));
        assert_eq!(a.strict_eq(c).as_boolean(), Some(false));

        // NaN !== NaN
        let nan = TsclValue::number(f64::NAN);
        assert_eq!(nan.strict_eq(nan).as_boolean(), Some(false));

        // Boolean equality
        let t1 = TsclValue::boolean(true);
        let t2 = TsclValue::boolean(true);
        let f = TsclValue::boolean(false);
        assert_eq!(t1.strict_eq(t2).as_boolean(), Some(true));
        assert_eq!(t1.strict_eq(f).as_boolean(), Some(false));
    }

    #[test]
    fn test_falsy() {
        assert!(TsclValue::undefined().is_falsy());
        assert!(TsclValue::null().is_falsy());
        assert!(TsclValue::boolean(false).is_falsy());
        assert!(TsclValue::number(0.0).is_falsy());
        assert!(TsclValue::number(f64::NAN).is_falsy());

        assert!(!TsclValue::boolean(true).is_falsy());
        assert!(!TsclValue::number(1.0).is_falsy());
        assert!(!TsclValue::number(-1.0).is_falsy());
    }
}
