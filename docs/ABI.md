# tscl ABI Specification

**Version:** 1  
**Last Updated:** January 2026

This document defines the **Application Binary Interface (ABI)** for the tscl runtime. The ABI is the contract between compiled tscl code and the runtime library.

## 1. ABI Versioning

```rust
pub const ABI_VERSION: u32 = 1;
pub const ABI_NAME: &str = "tscl";
```

The ABI version is embedded in all compiled binaries and verified at runtime. Breaking changes to the ABI require bumping this version.

## 2. Value Representation (NaN-Boxing)

All tscl values are represented as **64-bit words** using NaN-boxing:

```
64-bit word (u64):
┌─────────────────────────────────────────────────────────────────┐
│  Sign │  Exponent (11 bits)  │  Mantissa (52 bits)             │
│   1   │     11 bits           │        52 bits                   │
└─────────────────────────────────────────────────────────────────┘

Encoding rules:
- Numbers (IEEE 754 double): Canonical representation
- Small integers (-2^32 to 2^32-1): Encoded as immediate
- Pointers: NaN-boxed (exponent = all 1s, mantissa encodes pointer)
- Special values:
  - undefined: 0x7FF8000000000001 (quiet NaN with specific payload)
  - null:      0x7FF8000000000002
  - true:      0x7FF8000000000003
  - false:     0x7FF8000000000004
```

### 2.1 Special Value Encoding

```rust
const UNDEFINED_ENCODING: u64 = 0x7FF8000000000001;
const NULL_ENCODING:      u64 = 0x7FF8000000000002;
const TRUE_ENCODING:      u64 = 0x7FF8000000000003;
const FALSE_ENCODING:     u64 = 0x7FF8000000000004;
```

### 2.2 Pointer Encoding

Pointers are encoded in NaN space (exponent = 0x7FF, mantissa non-zero):

```
Pointer encoding (64 bits):
┌─────────────────────────────────────────────────────────────────┐
│  Sign = 1  │  Exponent = 0x7FF  │  Mantissa[51:0] = Pointer    │
│   1 bit    │     11 bits         │        52 bits               │
└─────────────────────────────────────────────────────────────────┘

The actual pointer is stored in the mantissa bits [51:0].
```

## 3. Runtime Stubs (C Calling Convention)

All runtime functions use the **System V AMD64 ABI** on x86-64 and **aarch64 ABI** on ARM64.

### 3.1 Core Stubs

```c
// === Allocation ===

// Allocate a new empty object
// Returns: u64 (pointer to object)
u64 tscl_alloc_object();

// Allocate a new empty array
// Returns: u64 (pointer to array)
u64 tscl_alloc_array();

// Allocate a new string
// Parameters: ptr = pointer to UTF-8 data, len = length in bytes
// Returns: u64 (pointer to string)
u64 tscl_alloc_string(const char* ptr, size_t len);


// === Property Access ===

// Get property from object
// Parameters: obj = object pointer, key = property name (pointer to string)
// Returns: u64 (property value)
u64 tscl_get_prop(u64 obj, u64 key);

// Set property on object
// Parameters: obj = object pointer, key = property name, val = value
// Returns: u64 (success code)
u64 tscl_set_prop(u64 obj, u64 key, u64 val);

// Get array element
// Parameters: obj = array pointer, idx = element index
// Returns: u64 (element value)
u64 tscl_get_element(u64 obj, u64 idx);

// Set array element
// Parameters: obj = array pointer, idx = element index, val = value
// Returns: u64 (success code)
u64 tscl_set_element(u64 obj, u64 idx, u64 val);


// === Arithmetic (Dynamic/Any Type) ===

// Dynamic addition (numbers or string concatenation)
// Parameters: a = first operand, b = second operand
// Returns: u64 (result)
u64 tscl_add_any(u64 a, u64 b);

u64 tscl_sub_any(u64 a, u64 b);
u64 tscl_mul_any(u64 a, u64 b);
u64 tscl_div_any(u64 a, u64 b);
u64 tscl_mod_any(u64 a, u64 b);


// === Comparison ===

// Strict equality (===)
// Parameters: a, b = values to compare
// Returns: u64 (1 = true, 0 = false)
u64 tscl_eq_strict(u64 a, u64 b);

// Less than (<)
// Parameters: a, b = values to compare
// Returns: u64 (1 = true, 0 = false)
u64 tscl_lt(u64 a, u64 b);


// === Function Calls ===

// Call a function
// Parameters: func = function pointer, args = arguments array, arg_count = count
// Returns: u64 (return value)
u64 tscl_call(u64 func, u64 args, u32 arg_count);


// === Conversions ===

// Convert to boolean
// Parameters: val = value to convert
// Returns: u64 (boolean encoding)
u64 tscl_to_boolean(u64 val);


// === Control ===

// Abort execution with message
// Parameters: msg = message pointer, len = message length
// This function does not return
noreturn void tscl_abort(const char* msg, size_t len);
```

### 3.2 Arithmetic Stubs (Specialized)

For type-specialized operations (when types are known at compile time):

```c
// Numeric operations (all parameters and return are IEEE 754 doubles)
double tscl_add_num(double a, double b);
double tscl_sub_num(double a, double b);
double tscl_mul_num(double a, double b);
double tscl_div_num(double a, double b);
double tscl_neg_num(double a);
```

### 3.3 String Operations

```c
// Get string length (in bytes)
// Parameters: str = string pointer
// Returns: u64 (length)
u64 tscl_string_len(u64 str);

// Get string character at index
// Parameters: str = string pointer, idx = character index
// Returns: u64 (character code)
u64 tscl_string_char_at(u64 str, u64 idx);

// String concatenation
// Parameters: a, b = strings
// Returns: u64 (new string)
u64 tscl_string_concat(u64 a, u64 b);

// String comparison
// Parameters: a, b = strings
// Returns: u64 (comparison result)
u64 tscl_string_compare(u64 a, u64 b);
```

## 4. Object Layout

### 4.1 Object Header

```c
struct TsclObject {
    // Header (16 bytes on 64-bit)
    uint32_t type_tag;      // Object type identifier
    uint32_t flags;         // Property attributes
    uint64_t prop_table;    // Pointer to property table

    // Properties follow (flexible array)
    // Property entries: { key_hash: u64, key_ptr: u64, value: u64 }
};
```

### 4.2 Array Layout

```c
struct TsclArray {
    // Header (24 bytes on 64-bit)
    uint32_t type_tag;      // Always 1 for arrays
    uint32_t flags;
    uint64_t length;        // Array length
    uint64_t capacity;      // Allocated capacity

    // Elements follow (flexible array)
    // Elements: u64 (NaN-boxed values)
};
```

### 4.3 String Layout

```c
struct TsclString {
    // Header (24 bytes on 64-bit)
    uint32_t type_tag;      // Always 2 for strings
    uint32_t flags;
    uint64_t length;        // Length in bytes (not UTF-16!)

    // UTF-8 data follows (not null-terminated)
    // bytes: uint8_t[length]
};
```

## 5. Function Call Convention

### 5.1 Function Types

```c
// Native function (extern "C")
typedef u64 (*TsclNativeFn)(u64 self, u64* args, u32 arg_count);

// Closure (captures environment)
struct TsclClosure {
    uint64_t func_ptr;      // Pointer to function code
    uint64_t env_ptr;       // Pointer to captured environment
};
```

### 5.2 Call Stack Layout

```
High addresses
┌─────────────────────────────────────┐
│  Return Address                     │  <- Stack pointer
├─────────────────────────────────────┤
│  Previous Frame Pointer (RBP)       │
├─────────────────────────────────────┤
│  Local Variables                    │
├─────────────────────────────────────┤
│  Arguments                          │
└─────────────────────────────────────┘
Low addresses
```

## 6. Error Handling

### 6.1 Error Codes

```c
#define TSCL_OK          0
#define TSCL_ERROR       1
#define TSCL TypeError   2
#define TSCL_RANGE_ERROR 3
#define TSCL_REF_ERROR   4
```

### 6.2 Exception Propagation

Exceptions are propagated using the Rust panic mechanism in the runtime, with `tscl_abort()` called for unhandled exceptions.

## 7. Memory Layout Guarantees

The following layout is guaranteed stable:

1. **Value encoding** (Section 2): Never changes
2. **Object header** (Section 4.1): Fields in this order, sizes as specified
3. **Array header** (Section 4.2): Fields in this order, sizes as specified
4. **String header** (Section 4.3): Fields in this order, sizes as specified
5. **Stub signatures** (Section 3): Never change without ABI version bump

## 8. Backward Compatibility

- **ABI Version 1**: Current version
- Future versions must:
  1. Increment `ABI_VERSION` for any breaking change
  2. Maintain backward compatibility within the same major version
  3. Document all changes in this file

## 9. Platform Notes

### 9.1 x86-64 (AMD64)
- Uses System V AMD64 ABI
- Arguments: RDI, RSI, RDX, RCX, R8, R9
- Return value: RAX
- Callee-saved: RBX, RBP, R12, R13, R14, R15

### 9.2 ARM64 (Aarch64)
- Uses AAPCS64 ABI
- Arguments: X0-X7
- Return value: X0
- Callee-saved: X19-X30

## 10. Testing ABI Compatibility

```bash
# Run ABI compatibility tests
cargo test abi_compatibility

# Verify ABI version in binary
./tscl build app.tscl --dist -o app
strings app | grep ABI_VERSION
# Expected: ABI_VERSION=1

# Verify determinism
./tscl build app.tscl --dist -o app1
./tscl build app.tscl --dist -o app2
diff <(sha256sum app1) <(sha256sum app2)
# Must be identical
```

---

**Questions or issues?** Report at: https://github.com/anomalyco/tscl/issues
