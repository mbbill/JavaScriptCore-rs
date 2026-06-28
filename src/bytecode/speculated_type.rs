//! Faithful port of JSC `SpeculatedType` — the uint64 type-prediction bitset the
//! DFG/FTL speculate on.
//!
//! C++ ground truth: `bytecode/SpeculatedType.h` (the `SpecXxx` flag constants,
//! union masks, `mergeSpeculation(s)`, and the `isXxxSpeculation` predicates) and
//! `bytecode/SpeculatedType.cpp` (the producers `speculationFrom{Value,Cell,
//! Structure,ClassInfoInheritance,JSType}` and `dumpSpeculation`).
//!
//! This is an ADDITIVE, UNWIRED module (`#![allow(dead_code)]`). It is the
//! faithful replacement-in-waiting for the DIVERGENT lattice enum at
//! `src/dfg/speculation.rs:13` (`SpeculatedType`) and the bare `u64` newtype at
//! `src/bytecode/profiling.rs:43` (`SpeculatedTypeSet`). Canonicalizing those two
//! onto this bitset is a SERIAL decision owned by the orchestrator; this module
//! does not touch `src/dfg/` or `src/bytecode/profiling.rs`.
//!
//! C++ `typedef uint64_t SpeculatedType` (SpeculatedType.h:46) is mirrored as a
//! plain `u64` type alias so the merge protocol stays literal `left | right` and
//! every predicate is a direct bit-op port.

#![allow(dead_code)]

use crate::runtime::JsType;
use crate::value::{CellValue, JsValue, NumberValue};

/// `typedef uint64_t SpeculatedType` (SpeculatedType.h:46).
pub type SpeculatedType = u64;

// =============================== Flag constants ==============================
// Exact 1<<n positional flags from SpeculatedType.h:47-135. Bit 3 is unused in
// C++ (SpecFunction is 1<<2, SpecInt8Array is 1<<4); the gap is preserved.

/// "We don't know anything yet." (SpeculatedType.h:47)
pub const SPEC_NONE: SpeculatedType = 0;
/// Definitely a JSFinalObject. (SpeculatedType.h:48)
pub const SPEC_FINAL_OBJECT: SpeculatedType = 1 << 0;
/// Definitely a JSArray. (SpeculatedType.h:49)
pub const SPEC_ARRAY: SpeculatedType = 1 << 1;
/// Definitely a JSFunction. (SpeculatedType.h:50)
pub const SPEC_FUNCTION: SpeculatedType = 1 << 2;
/// Int8Array or subclass. (SpeculatedType.h:51) — bit 3 is intentionally unused.
pub const SPEC_INT8_ARRAY: SpeculatedType = 1 << 4;
/// Int16Array or subclass. (SpeculatedType.h:52)
pub const SPEC_INT16_ARRAY: SpeculatedType = 1 << 5;
/// Int32Array or subclass. (SpeculatedType.h:53)
pub const SPEC_INT32_ARRAY: SpeculatedType = 1 << 6;
/// Uint8Array or subclass. (SpeculatedType.h:54)
pub const SPEC_UINT8_ARRAY: SpeculatedType = 1 << 7;
/// Uint8ClampedArray or subclass. (SpeculatedType.h:55)
pub const SPEC_UINT8_CLAMPED_ARRAY: SpeculatedType = 1 << 8;
/// Uint16Array or subclass. (SpeculatedType.h:56)
pub const SPEC_UINT16_ARRAY: SpeculatedType = 1 << 9;
/// Uint32Array or subclass. (SpeculatedType.h:57)
pub const SPEC_UINT32_ARRAY: SpeculatedType = 1 << 10;
/// Float16Array or subclass. (SpeculatedType.h:58)
pub const SPEC_FLOAT16_ARRAY: SpeculatedType = 1 << 11;
/// Float32Array or subclass. (SpeculatedType.h:59)
pub const SPEC_FLOAT32_ARRAY: SpeculatedType = 1 << 12;
/// Float64Array or subclass. (SpeculatedType.h:60)
pub const SPEC_FLOAT64_ARRAY: SpeculatedType = 1 << 13;
/// BigInt64Array or subclass. (SpeculatedType.h:61)
pub const SPEC_BIG_INT64_ARRAY: SpeculatedType = 1 << 14;
/// BigUint64Array or subclass. (SpeculatedType.h:62)
pub const SPEC_BIG_UINT64_ARRAY: SpeculatedType = 1 << 15;
/// Any typed-array view. (SpeculatedType.h:63)
pub const SPEC_TYPED_ARRAY_VIEW: SpeculatedType = SPEC_INT8_ARRAY
    | SPEC_INT16_ARRAY
    | SPEC_INT32_ARRAY
    | SPEC_UINT8_ARRAY
    | SPEC_UINT8_CLAMPED_ARRAY
    | SPEC_UINT16_ARRAY
    | SPEC_UINT32_ARRAY
    | SPEC_FLOAT16_ARRAY
    | SPEC_FLOAT32_ARRAY
    | SPEC_FLOAT64_ARRAY
    | SPEC_BIG_INT64_ARRAY
    | SPEC_BIG_UINT64_ARRAY;
/// DirectArguments object. (SpeculatedType.h:64)
pub const SPEC_DIRECT_ARGUMENTS: SpeculatedType = 1 << 16;
/// ScopedArguments object. (SpeculatedType.h:65)
pub const SPEC_SCOPED_ARGUMENTS: SpeculatedType = 1 << 17;
/// StringObject. (SpeculatedType.h:66)
pub const SPEC_STRING_OBJECT: SpeculatedType = 1 << 18;
/// RegExpObject (not a subclass). (SpeculatedType.h:67)
pub const SPEC_REG_EXP_OBJECT: SpeculatedType = 1 << 19;
/// Date object or subclass. (SpeculatedType.h:68)
pub const SPEC_DATE_OBJECT: SpeculatedType = 1 << 20;
/// Promise object or subclass. (SpeculatedType.h:69)
pub const SPEC_PROMISE_OBJECT: SpeculatedType = 1 << 21;
/// Map object or subclass. (SpeculatedType.h:70)
pub const SPEC_MAP_OBJECT: SpeculatedType = 1 << 22;
/// Set object or subclass. (SpeculatedType.h:71)
pub const SPEC_SET_OBJECT: SpeculatedType = 1 << 23;
/// Map iterator object or subclass. (SpeculatedType.h:72)
pub const SPEC_MAP_ITERATOR_OBJECT: SpeculatedType = 1 << 24;
/// Set iterator object or subclass. (SpeculatedType.h:73)
pub const SPEC_SET_ITERATOR_OBJECT: SpeculatedType = 1 << 25;
/// WeakMap object or subclass. (SpeculatedType.h:74)
pub const SPEC_WEAK_MAP_OBJECT: SpeculatedType = 1 << 26;
/// WeakSet object or subclass. (SpeculatedType.h:75)
pub const SPEC_WEAK_SET_OBJECT: SpeculatedType = 1 << 27;
/// Proxy object or subclass. (SpeculatedType.h:76)
pub const SPEC_PROXY_OBJECT: SpeculatedType = 1 << 28;
/// Global proxy. (SpeculatedType.h:77)
pub const SPEC_GLOBAL_PROXY: SpeculatedType = 1 << 29;
/// DerivedArray object. (SpeculatedType.h:78)
pub const SPEC_DERIVED_ARRAY: SpeculatedType = 1 << 30;
/// Object but not Final/Array/Function. (SpeculatedType.h:79)
pub const SPEC_OBJECT_OTHER: SpeculatedType = 1 << 31;
/// JSString that is an identifier. (SpeculatedType.h:80)
pub const SPEC_STRING_IDENT: SpeculatedType = 1 << 32;
/// JSString, not ident, resolved. (SpeculatedType.h:81)
pub const SPEC_STRING_RESOLVED_VAR: SpeculatedType = 1 << 33;
/// JSString, not ident, unresolved. (SpeculatedType.h:82)
pub const SPEC_STRING_UNRESOLVED_VAR: SpeculatedType = 1 << 34;
/// JSString, not an identifier. (SpeculatedType.h:83)
pub const SPEC_STRING_VAR: SpeculatedType = SPEC_STRING_UNRESOLVED_VAR | SPEC_STRING_RESOLVED_VAR;
/// JSString that is resolved (ident or not). (SpeculatedType.h:84)
pub const SPEC_STRING_RESOLVED: SpeculatedType = SPEC_STRING_IDENT | SPEC_STRING_RESOLVED_VAR;
/// Definitely a JSString. (SpeculatedType.h:85)
pub const SPEC_STRING: SpeculatedType = SPEC_STRING_IDENT | SPEC_STRING_VAR;
/// Definitely a Symbol. (SpeculatedType.h:86)
pub const SPEC_SYMBOL: SpeculatedType = 1 << 35;
/// JSCell, not a JSObject subclass, not String/BigInt/Symbol. (SpeculatedType.h:87)
pub const SPEC_CELL_OTHER: SpeculatedType = 1 << 36;
/// Int32 with value 0 or 1. (SpeculatedType.h:88)
pub const SPEC_BOOL_INT32: SpeculatedType = 1 << 37;
/// Int32 with value other than 0 or 1. (SpeculatedType.h:89)
pub const SPEC_NON_BOOL_INT32: SpeculatedType = 1 << 38;
/// Definitely an Int32. (SpeculatedType.h:90)
pub const SPEC_INT32_ONLY: SpeculatedType = SPEC_BOOL_INT32 | SPEC_NON_BOOL_INT32;
/// Int52 that fits in an int32. (SpeculatedType.h:92)
pub const SPEC_INT32_AS_INT52: SpeculatedType = 1 << 39;
/// Int52 that can't fit in an int32. (SpeculatedType.h:93)
pub const SPEC_NON_INT32_AS_INT52: SpeculatedType = 1 << 40;
/// Any kind of Int52. (SpeculatedType.h:94)
pub const SPEC_INT52_ANY: SpeculatedType = SPEC_INT32_AS_INT52 | SPEC_NON_INT32_AS_INT52;
/// Int52 inside a double. (SpeculatedType.h:96)
pub const SPEC_ANY_INT_AS_DOUBLE: SpeculatedType = 1 << 41;
/// Real-number double that is not an Int52. (SpeculatedType.h:97)
pub const SPEC_NON_INT_AS_DOUBLE: SpeculatedType = 1 << 42;
/// A non-NaN double. (SpeculatedType.h:98)
pub const SPEC_DOUBLE_REAL: SpeculatedType = SPEC_NON_INT_AS_DOUBLE | SPEC_ANY_INT_AS_DOUBLE;
/// NaN safe to tag (pure). (SpeculatedType.h:99)
pub const SPEC_DOUBLE_PURE_NAN: SpeculatedType = 1 << 43;
/// NaN unsafe to tag (impure). (SpeculatedType.h:100)
pub const SPEC_DOUBLE_IMPURE_NAN: SpeculatedType = 1 << 44;
/// Some kind of NaN. (SpeculatedType.h:101)
pub const SPEC_DOUBLE_NAN: SpeculatedType = SPEC_DOUBLE_PURE_NAN | SPEC_DOUBLE_IMPURE_NAN;
/// Non-NaN or pure-NaN double (not impure NaN). (SpeculatedType.h:102)
pub const SPEC_BYTECODE_DOUBLE: SpeculatedType = SPEC_DOUBLE_REAL | SPEC_DOUBLE_PURE_NAN;
/// Non-NaN or NaN double. (SpeculatedType.h:103)
pub const SPEC_FULL_DOUBLE: SpeculatedType = SPEC_DOUBLE_REAL | SPEC_DOUBLE_NAN;
/// Int32 or DoubleReal. (SpeculatedType.h:104)
pub const SPEC_BYTECODE_REAL_NUMBER: SpeculatedType = SPEC_INT32_ONLY | SPEC_DOUBLE_REAL;
/// Int32 or Int52 or DoubleReal. (SpeculatedType.h:105)
pub const SPEC_FULL_REAL_NUMBER: SpeculatedType =
    SPEC_INT32_ONLY | SPEC_INT52_ANY | SPEC_DOUBLE_REAL;
/// Int32 or Double (no impure NaN). (SpeculatedType.h:106)
pub const SPEC_BYTECODE_NUMBER: SpeculatedType = SPEC_INT32_ONLY | SPEC_BYTECODE_DOUBLE;
/// Int52/Int32/AnyIntAsDouble. (SpeculatedType.h:107)
pub const SPEC_INT_ANY_FORMAT: SpeculatedType =
    SPEC_INT52_ANY | SPEC_INT32_ONLY | SPEC_ANY_INT_AS_DOUBLE;
/// Int32, Int52, or Double (double can be impure NaN). (SpeculatedType.h:109)
pub const SPEC_FULL_NUMBER: SpeculatedType = SPEC_INT_ANY_FORMAT | SPEC_FULL_DOUBLE;
/// Definitely a Boolean. (SpeculatedType.h:110)
pub const SPEC_BOOLEAN: SpeculatedType = 1 << 45;
/// Definitely Null or Undefined. (SpeculatedType.h:111)
pub const SPEC_OTHER: SpeculatedType = 1 << 46;
/// Boolean, Null, or Undefined. (SpeculatedType.h:112)
pub const SPEC_MISC: SpeculatedType = SPEC_BOOLEAN | SPEC_OTHER;
/// Empty value marker. (SpeculatedType.h:113)
pub const SPEC_EMPTY: SpeculatedType = 1 << 47;
/// Heap-allocated BigInt. (SpeculatedType.h:114)
pub const SPEC_HEAP_BIG_INT: SpeculatedType = 1 << 48;
/// Small BigInt inline in the JSValue. (SpeculatedType.h:115)
pub const SPEC_BIG_INT32: SpeculatedType = 1 << 49;
/// Any BigInt. (SpeculatedType.h:116-121)
///
/// DIVERGENCE (configuration, not behavior): JSC selects `SpecBigInt32 |
/// SpecHeapBigInt` under `USE(BIGINT32)` and `SpecHeapBigInt` otherwise. The
/// port's live value layer has no inline BigInt32 immediate (src/value/repr.rs),
/// i.e. it is the `!USE(BIGINT32)` configuration, so `SpecBigInt` excludes
/// `SpecBigInt32`, exactly as the C++ `#else` arm (SpeculatedType.h:120).
pub const SPEC_BIG_INT: SpeculatedType = SPEC_HEAP_BIG_INT;
/// Definitely a JSDataView. (SpeculatedType.h:122)
pub const SPEC_DATA_VIEW_OBJECT: SpeculatedType = 1 << 50;
/// Any non-Object JSValue. (SpeculatedType.h:123)
pub const SPEC_PRIMITIVE: SpeculatedType =
    SPEC_STRING | SPEC_SYMBOL | SPEC_BYTECODE_NUMBER | SPEC_MISC | SPEC_BIG_INT;
/// Any kind of object. (SpeculatedType.h:124)
pub const SPEC_OBJECT: SpeculatedType = SPEC_FINAL_OBJECT
    | SPEC_ARRAY
    | SPEC_FUNCTION
    | SPEC_TYPED_ARRAY_VIEW
    | SPEC_DIRECT_ARGUMENTS
    | SPEC_SCOPED_ARGUMENTS
    | SPEC_STRING_OBJECT
    | SPEC_REG_EXP_OBJECT
    | SPEC_DATE_OBJECT
    | SPEC_PROMISE_OBJECT
    | SPEC_MAP_OBJECT
    | SPEC_SET_OBJECT
    | SPEC_MAP_ITERATOR_OBJECT
    | SPEC_SET_ITERATOR_OBJECT
    | SPEC_WEAK_MAP_OBJECT
    | SPEC_WEAK_SET_OBJECT
    | SPEC_PROXY_OBJECT
    | SPEC_GLOBAL_PROXY
    | SPEC_DERIVED_ARRAY
    | SPEC_OBJECT_OTHER
    | SPEC_DATA_VIEW_OBJECT;
/// Definitely a JSCell. (SpeculatedType.h:125)
pub const SPEC_CELL: SpeculatedType =
    SPEC_OBJECT | SPEC_STRING | SPEC_SYMBOL | SPEC_CELL_OTHER | SPEC_HEAP_BIG_INT;
/// Anything except SpecInt52Only and SpecDoubleImpureNaN. (SpeculatedType.h:126)
pub const SPEC_HEAP_TOP: SpeculatedType =
    SPEC_CELL | SPEC_BIG_INT32 | SPEC_BYTECODE_NUMBER | SPEC_MISC;
/// What could be found in a bytecode local. (SpeculatedType.h:127)
pub const SPEC_BYTECODE_TOP: SpeculatedType = SPEC_HEAP_TOP | SPEC_EMPTY;
/// Bytecode-visible plus exotic number encodings. (SpeculatedType.h:128)
pub const SPEC_FULL_TOP: SpeculatedType = SPEC_BYTECODE_TOP | SPEC_FULL_NUMBER;
/// Types where typeof might be "function". (SpeculatedType.h:130)
pub const SPEC_TYPEOF_MIGHT_BE_FUNCTION: SpeculatedType =
    SPEC_FUNCTION | SPEC_OBJECT_OTHER | SPEC_PROXY_OBJECT;
/// Values that pass a cell check. (SpeculatedType.h:135)
///
/// C++: `is64Bit() ? (SpecCell | SpecEmpty) : SpecCell`. The port targets 64-bit
/// (arm64/x86-64), where the empty value passes a cell check, so the 64-bit arm
/// is used.
pub const SPEC_CELL_CHECK: SpeculatedType = SPEC_CELL | SPEC_EMPTY;

// =============================== Predicates ==================================
// Direct ports of the `inline bool isXxxSpeculation(...)` family
// (SpeculatedType.h:140-523). `!!x` becomes `x != 0`; `!(x)` becomes `(x) == 0`.

/// Dummy checker. (SpeculatedType.h:140)
pub const fn is_any_speculation(_value: SpeculatedType) -> bool {
    true
}

/// `!(value & ~category) && value`. (SpeculatedType.h:145)
pub const fn is_subtype_speculation(value: SpeculatedType, category: SpeculatedType) -> bool {
    (value & !category) == 0 && value != 0
}

/// `!!(value & category) && value`. (SpeculatedType.h:150)
pub const fn speculation_contains(value: SpeculatedType, category: SpeculatedType) -> bool {
    (value & category) != 0 && value != 0
}

/// (SpeculatedType.h:155)
pub const fn is_cell_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_CELL) != 0 && (value & !SPEC_CELL) == 0
}

/// (SpeculatedType.h:160)
pub const fn is_cell_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !(SPEC_CELL | SPEC_OTHER)) == 0
}

/// (SpeculatedType.h:165)
pub const fn is_not_cell_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_CELL_CHECK) == 0 && value != 0
}

/// (SpeculatedType.h:170)
pub const fn is_not_cell_nor_big_int_speculation(value: SpeculatedType) -> bool {
    (value & (SPEC_CELL_CHECK | SPEC_BIG_INT)) == 0 && value != 0
}

/// (SpeculatedType.h:175)
pub const fn is_object_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_OBJECT) != 0 && (value & !SPEC_OBJECT) == 0
}

/// (SpeculatedType.h:180)
pub const fn is_object_or_other_speculation(value: SpeculatedType) -> bool {
    (value & (SPEC_OBJECT | SPEC_OTHER)) != 0 && (value & !(SPEC_OBJECT | SPEC_OTHER)) == 0
}

/// (SpeculatedType.h:185)
pub const fn is_final_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FINAL_OBJECT
}

/// (SpeculatedType.h:190)
pub const fn is_final_object_or_other_speculation(value: SpeculatedType) -> bool {
    (value & (SPEC_FINAL_OBJECT | SPEC_OTHER)) != 0
        && (value & !(SPEC_FINAL_OBJECT | SPEC_OTHER)) == 0
}

/// (SpeculatedType.h:195)
pub const fn is_string_ident_speculation(value: SpeculatedType) -> bool {
    value == SPEC_STRING_IDENT
}

/// (SpeculatedType.h:200)
pub const fn is_not_string_var_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_STRING_VAR) == 0
}

/// (SpeculatedType.h:205)
pub const fn is_string_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_STRING) == value
}

/// (SpeculatedType.h:210)
pub const fn is_not_string_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_STRING) == 0
}

/// (SpeculatedType.h:215)
pub const fn is_string_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & (SPEC_STRING | SPEC_OTHER)) == value
}

/// (SpeculatedType.h:220)
pub const fn is_symbol_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SYMBOL
}

/// (SpeculatedType.h:225)
pub const fn is_big_int32_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_INT32
}

/// (SpeculatedType.h:230)
pub const fn is_heap_big_int_speculation(value: SpeculatedType) -> bool {
    value == SPEC_HEAP_BIG_INT
}

/// (SpeculatedType.h:235)
pub const fn is_big_int_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_BIG_INT) == value
}

/// (SpeculatedType.h:240)
pub const fn is_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_ARRAY
}

/// (SpeculatedType.h:245)
pub const fn is_function_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FUNCTION
}

/// (SpeculatedType.h:250)
pub const fn is_proxy_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_PROXY_OBJECT
}

/// (SpeculatedType.h:255)
pub const fn is_set_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SET_OBJECT
}

/// (SpeculatedType.h:260)
pub const fn is_global_proxy_speculation(value: SpeculatedType) -> bool {
    value == SPEC_GLOBAL_PROXY
}

/// (SpeculatedType.h:265)
pub const fn is_derived_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_DERIVED_ARRAY
}

/// (SpeculatedType.h:270)
pub const fn is_int8_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT8_ARRAY
}

/// (SpeculatedType.h:275)
pub const fn is_int16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT16_ARRAY
}

/// (SpeculatedType.h:280)
pub const fn is_int32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_INT32_ARRAY
}

/// (SpeculatedType.h:285)
pub const fn is_uint8_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT8_ARRAY
}

/// (SpeculatedType.h:290)
pub const fn is_uint8_clamped_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT8_CLAMPED_ARRAY
}

/// (SpeculatedType.h:295)
pub const fn is_uint16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT16_ARRAY
}

/// (SpeculatedType.h:300)
pub const fn is_uint32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_UINT32_ARRAY
}

/// (SpeculatedType.h:305)
pub const fn is_float16_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT16_ARRAY
}

/// (SpeculatedType.h:310)
pub const fn is_float32_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT32_ARRAY
}

/// (SpeculatedType.h:315)
pub const fn is_float64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_FLOAT64_ARRAY
}

/// (SpeculatedType.h:320)
pub const fn is_big_int64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_INT64_ARRAY
}

/// (SpeculatedType.h:325)
pub const fn is_big_uint64_array_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BIG_UINT64_ARRAY
}

/// (SpeculatedType.h:330)
pub const fn is_direct_arguments_speculation(value: SpeculatedType) -> bool {
    value == SPEC_DIRECT_ARGUMENTS
}

/// (SpeculatedType.h:335)
pub const fn is_scoped_arguments_speculation(value: SpeculatedType) -> bool {
    value == SPEC_SCOPED_ARGUMENTS
}

/// (SpeculatedType.h:340)
pub const fn is_array_or_other_speculation(value: SpeculatedType) -> bool {
    (value & (SPEC_ARRAY | SPEC_OTHER)) != 0 && (value & !(SPEC_ARRAY | SPEC_OTHER)) == 0
}

/// (SpeculatedType.h:345)
pub const fn is_string_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_STRING_OBJECT
}

/// (SpeculatedType.h:350)
pub const fn is_string_or_string_object_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !(SPEC_STRING | SPEC_STRING_OBJECT)) == 0
}

/// (SpeculatedType.h:355)
pub const fn is_reg_exp_object_speculation(value: SpeculatedType) -> bool {
    value == SPEC_REG_EXP_OBJECT
}

/// (SpeculatedType.h:360)
pub const fn is_bool_int32_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BOOL_INT32
}

/// (SpeculatedType.h:365)
pub const fn is_int32_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !SPEC_INT32_ONLY) == 0
}

/// (SpeculatedType.h:370)
pub const fn is_not_int32_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_INT32_ONLY) == 0
}

/// (SpeculatedType.h:375)
pub const fn is_int32_or_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !(SPEC_BOOLEAN | SPEC_INT32_ONLY)) == 0
}

/// (SpeculatedType.h:380)
pub const fn is_int32_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    (value & (SPEC_FULL_DOUBLE | SPEC_NON_INT32_AS_INT52 | SPEC_BIG_INT)) == 0
}

/// (SpeculatedType.h:385)
pub const fn is_int32_or_boolean_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    (value & (SPEC_FULL_DOUBLE | SPEC_NON_INT32_AS_INT52 | SPEC_BIG_INT)) == 0
}

/// (SpeculatedType.h:390)
pub const fn is_int32_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !(SPEC_INT32_ONLY | SPEC_OTHER)) == 0
}

/// (SpeculatedType.h:395)
pub const fn is_int32_or_boolean_speculation_expecting_defined(value: SpeculatedType) -> bool {
    is_int32_or_boolean_speculation(value & !SPEC_OTHER)
}

/// (SpeculatedType.h:400)
pub const fn is_any_int52_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_INT52_ANY) == value
}

/// (SpeculatedType.h:405)
pub const fn is_int32_or_int52_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & (SPEC_INT32_ONLY | SPEC_INT52_ANY)) == value
}

/// (SpeculatedType.h:410)
pub const fn is_int32_or_int52_or_other_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & (SPEC_INT32_ONLY | SPEC_INT52_ANY | SPEC_OTHER)) == value
}

/// (SpeculatedType.h:415)
pub const fn is_int_any_format(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_INT_ANY_FORMAT) == value
}

/// (SpeculatedType.h:420)
pub const fn is_any_int_as_double_speculation(value: SpeculatedType) -> bool {
    value == SPEC_ANY_INT_AS_DOUBLE
}

/// (SpeculatedType.h:425)
pub const fn is_double_real_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_DOUBLE_REAL) == value
}

/// (SpeculatedType.h:430)
pub const fn is_double_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_FULL_DOUBLE) == value
}

/// (SpeculatedType.h:435)
pub const fn is_double_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    (value & SPEC_FULL_DOUBLE) != 0
}

/// (SpeculatedType.h:440)
pub const fn is_bytecode_real_number_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_BYTECODE_REAL_NUMBER) != 0 && (value & !SPEC_BYTECODE_REAL_NUMBER) == 0
}

/// (SpeculatedType.h:445)
pub const fn is_full_real_number_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_FULL_REAL_NUMBER) != 0 && (value & !SPEC_FULL_REAL_NUMBER) == 0
}

/// (SpeculatedType.h:450)
pub const fn is_bytecode_number_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_BYTECODE_NUMBER) != 0 && (value & !SPEC_BYTECODE_NUMBER) == 0
}

/// (SpeculatedType.h:455)
pub const fn is_full_number_speculation(value: SpeculatedType) -> bool {
    (value & SPEC_FULL_NUMBER) != 0 && (value & !SPEC_FULL_NUMBER) == 0
}

/// (SpeculatedType.h:460)
pub const fn is_full_number_or_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN)) == 0
}

/// (SpeculatedType.h:465)
pub const fn is_full_number_or_boolean_speculation_expecting_defined(
    value: SpeculatedType,
) -> bool {
    is_full_number_or_boolean_speculation(value & !SPEC_OTHER)
}

/// (SpeculatedType.h:470)
pub const fn is_boolean_speculation(value: SpeculatedType) -> bool {
    value == SPEC_BOOLEAN
}

/// (SpeculatedType.h:475)
pub const fn is_not_boolean_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & SPEC_BOOLEAN) == 0
}

/// (SpeculatedType.h:480)
pub const fn is_not_double_speculation(ty: SpeculatedType) -> bool {
    (ty & SPEC_FULL_DOUBLE) == 0
}

/// (SpeculatedType.h:485)
pub const fn is_neither_double_nor_heap_big_int_nor_string_speculation(ty: SpeculatedType) -> bool {
    (ty & (SPEC_FULL_DOUBLE | SPEC_HEAP_BIG_INT | SPEC_STRING)) == 0
}

/// (SpeculatedType.h:490)
pub const fn is_neither_double_nor_heap_big_int_speculation(ty: SpeculatedType) -> bool {
    (ty & (SPEC_FULL_DOUBLE | SPEC_HEAP_BIG_INT)) == 0
}

/// (SpeculatedType.h:495)
pub const fn is_other_speculation(value: SpeculatedType) -> bool {
    value == SPEC_OTHER
}

/// (SpeculatedType.h:500)
pub const fn is_misc_speculation(value: SpeculatedType) -> bool {
    value != 0 && (value & !SPEC_MISC) == 0
}

/// (SpeculatedType.h:505)
pub const fn is_other_or_empty_speculation(value: SpeculatedType) -> bool {
    value == 0 || value == SPEC_OTHER
}

/// (SpeculatedType.h:510)
pub const fn is_empty_speculation(value: SpeculatedType) -> bool {
    value == SPEC_EMPTY
}

/// (SpeculatedType.h:515)
pub const fn is_untyped_speculation_for_arithmetic(value: SpeculatedType) -> bool {
    (value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN)) != 0
}

/// (SpeculatedType.h:520)
pub const fn is_untyped_speculation_for_bit_ops(value: SpeculatedType) -> bool {
    (value & !(SPEC_FULL_NUMBER | SPEC_BOOLEAN | SPEC_OTHER)) != 0
}

// ============================== Merge protocol ==============================

/// `mergeSpeculations(left, right) = left | right`. (SpeculatedType.h:541)
pub const fn merge_speculations(left: SpeculatedType, right: SpeculatedType) -> SpeculatedType {
    left | right
}

/// `mergeSpeculation(T& left, right)`: OR `right` into `left`, returning whether
/// it changed. (SpeculatedType.h:546-553) The C++ template is instantiated here
/// for the `SpeculatedType` (u64) case.
pub fn merge_speculation(left: &mut SpeculatedType, right: SpeculatedType) -> bool {
    let new_speculation = merge_speculations(*left, right);
    let result = new_speculation != *left;
    *left = new_speculation;
    result
}

/// `speculationChecked(actual, desired) = (actual | desired) == desired`.
/// (SpeculatedType.h:555)
pub const fn speculation_checked(actual: SpeculatedType, desired: SpeculatedType) -> bool {
    (actual | desired) == desired
}

// ============================ JSType producers ==============================

/// `std::optional<SpeculatedType> speculationFromJSType(JSType)`
/// (SpeculatedType.cpp:700-740): a switch returning the direct prediction for a
/// handful of cell/object JSTypes, and `nullopt` (here `None`) for all others.
///
/// The Rust `JsType` (runtime/js_type.rs:32) is a deliberately PARTIAL mirror of
/// the full C++ `enum JSType` (JSType.h:164), so only the modeled variants have
/// arms. `Object`/`FinalObject` map to `None` exactly as the C++ `default` arm.
/// As the `JsType` enum grows (ArrayType, RegExpObjectType, ...), add their arms
/// from the C++ switch — this is missing variants, not a behavioral change.
pub fn speculation_from_js_type(ty: JsType) -> Option<SpeculatedType> {
    match ty {
        JsType::String => Some(SPEC_STRING),
        JsType::Symbol => Some(SPEC_SYMBOL),
        JsType::HeapBigInt => Some(SPEC_HEAP_BIG_INT),
        JsType::Object | JsType::FinalObject => None,
    }
}

/// The `speculatedTypeMapping[type]` table lookup (SpeculatedType.cpp:576-592).
///
/// C++ builds a 256-entry array, initializes every slot to `SpecObjectOther`,
/// then overrides each defined JSType per `FOR_EACH_JS_TYPE` (JSType.h:30-161).
/// Because `FOR_EACH_JS_TYPE` covers every real JSType, the `SpecObjectOther`
/// default only survives in the embedder/gap byte range. This port realizes the
/// rows for the JSTypes the partial Rust `JsType` enum models (their exact
/// `FOR_EACH_JS_TYPE` predictions); the match is exhaustive so no `default` arm
/// is reachable, but the documented fallthrough remains `SpecObjectOther`.
pub fn speculated_type_from_js_type_mapping(ty: JsType) -> SpeculatedType {
    match ty {
        // FOR_EACH_JS_TYPE rows (JSType.h:37,38,40,77,78).
        JsType::String => SPEC_STRING,
        JsType::HeapBigInt => SPEC_HEAP_BIG_INT,
        JsType::Symbol => SPEC_SYMBOL,
        JsType::Object => SPEC_OBJECT_OTHER, // macro(ObjectType, SpecObjectOther)
        JsType::FinalObject => SPEC_FINAL_OBJECT, // macro(FinalObjectType, SpecFinalObject)
    }
}

/// `speculationFromStructure(Structure*)` (SpeculatedType.cpp:588-592):
/// `speculatedTypeMapping[structure->typeInfo().type()]`.
///
/// DIVERGENCE (reachability, not behavior): C++ reads the JSType out of the
/// structure's `TypeInfoBlob`. The Rust `Structure` leaf exposes that same byte
/// via `TypeInfoBlob::ty()` (object/structure_cell.rs:204-207); the caller
/// bridges it to a `JsType` and passes it here, so the mapping semantics are
/// identical.
pub fn speculation_from_structure(structure_type: JsType) -> SpeculatedType {
    speculated_type_from_js_type_mapping(structure_type)
}

/// The string sub-classification `speculationFromCell` derives from a JSString
/// via `JSString::tryGetValueImpl()` and `StringImpl::isAtom()`
/// (SpeculatedType.cpp:602-614). Modeled as an explicit input because the port's
/// opaque cell value cannot reach the live `StringImpl` (see
/// `speculation_from_cell`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringSpeculationKind {
    /// `impl->isAtom()` => `SpecStringIdent`. (SpeculatedType.cpp:609-610)
    Atom,
    /// `impl && !isAtom()` => `SpecStringResolved`. (SpeculatedType.cpp:611)
    ResolvedNonAtom,
    /// `tryGetValueImpl() == nullptr` (rope) => `SpecString`. (SpeculatedType.cpp:613)
    Rope,
}

/// `speculationFromCell(JSCell*)` (SpeculatedType.cpp:594-618): strings get the
/// ident/resolved/rope sub-classification; every other cell uses the JSType
/// mapping table.
///
/// DIVERGENCE (reachability, not behavior): C++ reads `cell->isString()` /
/// `cell->type()` and, for strings, the live `StringImpl`'s atom flag directly
/// off the heap `JSCell*`. The port's `value::CellValue` is opaque payload bits
/// during D1a coexistence (the in-cell `JSCell::m_type` lives in the gc metadata
/// side table — see runtime/js_type.rs header), so a wired caller decodes the
/// `JsType` (and, for `JsType::String`, the `StringSpeculationKind`) and passes
/// them — the same inputs the C++ derives from the cell. The `isSanePointer`
/// `SpecNone` guards (SpeculatedType.cpp:596-608) are pointer-integrity checks
/// with no analog here and are intentionally not modeled.
pub fn speculation_from_cell(
    ty: JsType,
    string_kind: Option<StringSpeculationKind>,
) -> SpeculatedType {
    if let JsType::String = ty {
        return match string_kind {
            Some(StringSpeculationKind::Atom) => SPEC_STRING_IDENT,
            Some(StringSpeculationKind::ResolvedNonAtom) => SPEC_STRING_RESOLVED,
            // No impl / rope, or caller did not supply the sub-kind: SpecString,
            // matching the `return SpecString;` fallthrough (SpeculatedType.cpp:613).
            Some(StringSpeculationKind::Rope) | None => SPEC_STRING,
        };
    }
    speculated_type_from_js_type_mapping(ty)
}

// ============================ Value producers ===============================

/// `JSValue::isAnyInt()` (JSCJSValue.h:1223-1230): int32, or a number-double that
/// is an exact Int52. Ported locally because the value layer does not yet expose
/// it; it should migrate to `value::repr` when that layer gains the accessor.
fn value_is_any_int(value: JsValue) -> bool {
    if value.is_int32() {
        return true;
    }
    match value.as_number() {
        Some(NumberValue::DoubleBits(bits)) => is_int52(bits.to_f64()),
        // Non-number (or, unreachably, an int32 already handled above): not anyInt.
        _ => false,
    }
}

/// `JSValue::asAnyInt()` (JSCJSValue.h:1232-1238): the integer value of an
/// already-known anyInt.
fn value_as_any_int(value: JsValue) -> i64 {
    match value.as_number() {
        Some(NumberValue::Int32(i)) => i as i64,
        Some(NumberValue::DoubleBits(bits)) => bits.to_f64() as i64,
        None => 0,
    }
}

/// `isInt52(double)` via `tryConvertToInt52` (JSCJSValue.h:1193-1221), with
/// `numberOfInt52Bits == 52` (JSCJSValue.h:356), so the range is `[-2^51, 2^51)`.
///
/// DIVERGENCE (benign): C++ uses `static_cast<int64_t>(number)`, which is UB for
/// out-of-range / infinite inputs (the win32 path special-cases `isinf`). Rust's
/// `as i64` saturates instead, and the subsequent `as_i64 as f64 != number`
/// round-trip guard rejects every out-of-range/inf input either way, so the
/// accepted set is identical. The `!asInt64 && signbit` test excludes `-0.0`.
fn is_int52(number: f64) -> bool {
    if number.is_nan() {
        return false;
    }
    let as_i64 = number as i64;
    if (as_i64 as f64) != number {
        return false;
    }
    if as_i64 == 0 && number.is_sign_negative() {
        return false;
    }
    if as_i64 >= (1_i64 << 51) {
        return false;
    }
    if as_i64 < -(1_i64 << 51) {
        return false;
    }
    true
}

/// `speculationFromValue(JSValue)` (SpeculatedType.cpp:620-645).
///
/// DIVERGENCE (reachability, not behavior): the C++ cell arm calls
/// `speculationFromCell(value.asCell())` directly because the `JSCell*` is
/// reachable. The port's `value::CellValue` is opaque payload bits during D1a
/// coexistence and cannot reach the in-cell JSType, so the cell -> SpeculatedType
/// step is threaded through `resolve_cell` (a wired caller passes e.g.
/// `|c| speculation_from_cell(js_type_of(c), string_kind_of(c))`). Every
/// immediate arm is byte-faithful.
///
/// The `value.isBigInt32()` arm (SpeculatedType.cpp:637-638) is omitted: the live
/// value layer has no inline BigInt32 immediate (the `!USE(BIGINT32)`
/// configuration), so that case never arises.
pub fn speculation_from_value(
    value: JsValue,
    resolve_cell: impl FnOnce(CellValue) -> SpeculatedType,
) -> SpeculatedType {
    // `value.isEmpty()` == ValueEmpty (SpeculatedType.cpp:622). `JsValue` exposes
    // this exactly via `is_empty_or_deleted_sentinel`, which matches only the
    // 0x0 Empty sentinel (value/repr.rs:732-734).
    if value.is_empty_or_deleted_sentinel() {
        return SPEC_EMPTY;
    }
    if value.is_int32() {
        // `value.asInt32() & ~1`: any bit other than bit 0 => non-bool int32.
        if let Some(NumberValue::Int32(i)) = value.as_number() {
            if (i & !1) != 0 {
                return SPEC_NON_BOOL_INT32;
            }
            return SPEC_BOOL_INT32;
        }
    }
    if value.is_double() {
        if let Some(NumberValue::DoubleBits(bits)) = value.as_number() {
            let number = bits.to_f64();
            if number.is_nan() {
                return SPEC_DOUBLE_PURE_NAN;
            }
            if is_int52(number) {
                return SPEC_ANY_INT_AS_DOUBLE;
            }
            return SPEC_NON_INT_AS_DOUBLE;
        }
    }
    if let Some(cell) = value.as_cell() {
        return resolve_cell(cell);
    }
    if value.is_boolean() {
        return SPEC_BOOLEAN;
    }
    // ASSERT(value.isUndefinedOrNull()) (SpeculatedType.cpp:643).
    debug_assert!(value.is_undefined_or_null());
    SPEC_OTHER
}

/// `int52AwareSpeculationFromValue(JSValue)` (SpeculatedType.cpp:647-657): if the
/// value is an anyInt, return from the Int52 lattice; otherwise defer to
/// `speculation_from_value`. See that function for the `resolve_cell` rationale.
pub fn int52_aware_speculation_from_value(
    value: JsValue,
    resolve_cell: impl FnOnce(CellValue) -> SpeculatedType,
) -> SpeculatedType {
    if !value_is_any_int(value) {
        return speculation_from_value(value, resolve_cell);
    }
    let int_value = value_as_any_int(value);
    let is_i32 = (int_value as i32 as i64) == int_value;
    if is_i32 {
        SPEC_INT32_AS_INT52
    } else {
        SPEC_NON_INT32_AS_INT52
    }
}

// ======================= ClassInfo-inheritance producer =====================

/// The JSC class-info leaves `speculationFromClassInfoInheritance` distinguishes
/// (SpeculatedType.cpp:483-573), named for the real JSC classes. Used only as the
/// argument to [`ClassInfoInheritance`]; this is the minimal seam, NOT a class
/// hierarchy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassInfoLeaf {
    JSString,
    Symbol,
    JSBigInt,
    JSFinalObject,
    DirectArguments,
    ScopedArguments,
    RegExpObject,
    DateInstance,
    JSMap,
    JSMapIterator,
    JSSet,
    JSSetIterator,
    JSWeakMap,
    JSWeakSet,
    ProxyObject,
    JSGlobalProxy,
    JSDataView,
    StringObject,
    JSArray,
    JSFunction,
    JSPromise,
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float16Array,
    Float32Array,
    Float64Array,
    BigInt64Array,
    BigUint64Array,
    JSObject,
}

/// The two `ClassInfo` operations `speculationFromClassInfoInheritance` needs:
/// pointer-identity (`classInfo == X::info()`) and `ClassInfo::isSubClassOf`
/// (ClassInfo.h). The port has no `ClassInfo` type yet; when one is ported it
/// implements this trait, and the producer below works unchanged.
pub trait ClassInfoInheritance {
    /// `classInfo == leaf::info()`.
    fn is(&self, leaf: ClassInfoLeaf) -> bool;
    /// `classInfo->isSubClassOf(leaf::info())`.
    fn is_sub_class_of(&self, leaf: ClassInfoLeaf) -> bool;
}

/// `speculationFromClassInfoInheritance(const ClassInfo*)`
/// (SpeculatedType.cpp:483-573). The exact branch order is preserved, including
/// the JSC subtlety that `JSMapIterator`/`JSSetIterator` return `SpecObjectOther`
/// here (SpeculatedType.cpp:523,531) rather than their dedicated iterator flags
/// (which `speculationFromJSType` does return) — the rope/`tryGetValueImpl`
/// `static_assert`s are compile-time only and have no runtime analog.
pub fn speculation_from_class_info_inheritance(
    class_info: &impl ClassInfoInheritance,
) -> SpeculatedType {
    use ClassInfoLeaf::*;
    if class_info.is(JSString) {
        return SPEC_STRING;
    }
    if class_info.is(Symbol) {
        return SPEC_SYMBOL;
    }
    if class_info.is(JSBigInt) {
        return SPEC_HEAP_BIG_INT;
    }
    if class_info.is(JSFinalObject) {
        return SPEC_FINAL_OBJECT;
    }
    if class_info.is(DirectArguments) {
        return SPEC_DIRECT_ARGUMENTS;
    }
    if class_info.is(ScopedArguments) {
        return SPEC_SCOPED_ARGUMENTS;
    }
    if class_info.is(RegExpObject) {
        return SPEC_REG_EXP_OBJECT;
    }
    if class_info.is(DateInstance) {
        return SPEC_DATE_OBJECT;
    }
    if class_info.is(JSMap) {
        return SPEC_MAP_OBJECT;
    }
    if class_info.is(JSMapIterator) {
        return SPEC_OBJECT_OTHER; // SpeculatedType.cpp:523
    }
    if class_info.is(JSSet) {
        return SPEC_SET_OBJECT;
    }
    if class_info.is(JSSetIterator) {
        return SPEC_OBJECT_OTHER; // SpeculatedType.cpp:531
    }
    if class_info.is(JSWeakMap) {
        return SPEC_WEAK_MAP_OBJECT;
    }
    if class_info.is(JSWeakSet) {
        return SPEC_WEAK_SET_OBJECT;
    }
    if class_info.is(ProxyObject) {
        return SPEC_PROXY_OBJECT;
    }
    if class_info.is_sub_class_of(JSGlobalProxy) {
        return SPEC_GLOBAL_PROXY;
    }
    if class_info.is_sub_class_of(JSDataView) {
        return SPEC_DATA_VIEW_OBJECT;
    }
    if class_info.is_sub_class_of(StringObject) {
        return SPEC_STRING_OBJECT | SPEC_OBJECT_OTHER;
    }
    if class_info.is_sub_class_of(JSArray) {
        return SPEC_ARRAY | SPEC_DERIVED_ARRAY;
    }
    if class_info.is_sub_class_of(JSFunction) {
        return SPEC_FUNCTION;
    }
    if class_info.is_sub_class_of(JSPromise) {
        return SPEC_PROMISE_OBJECT;
    }
    // FOR_EACH_TYPED_ARRAY_TYPE_EXCLUDING_DATA_VIEW (SpeculatedType.cpp:563-567),
    // in JSType.h:99-110 order.
    if class_info.is_sub_class_of(Int8Array) {
        return SPEC_INT8_ARRAY;
    }
    if class_info.is_sub_class_of(Uint8Array) {
        return SPEC_UINT8_ARRAY;
    }
    if class_info.is_sub_class_of(Uint8ClampedArray) {
        return SPEC_UINT8_CLAMPED_ARRAY;
    }
    if class_info.is_sub_class_of(Int16Array) {
        return SPEC_INT16_ARRAY;
    }
    if class_info.is_sub_class_of(Uint16Array) {
        return SPEC_UINT16_ARRAY;
    }
    if class_info.is_sub_class_of(Int32Array) {
        return SPEC_INT32_ARRAY;
    }
    if class_info.is_sub_class_of(Uint32Array) {
        return SPEC_UINT32_ARRAY;
    }
    if class_info.is_sub_class_of(Float16Array) {
        return SPEC_FLOAT16_ARRAY;
    }
    if class_info.is_sub_class_of(Float32Array) {
        return SPEC_FLOAT32_ARRAY;
    }
    if class_info.is_sub_class_of(Float64Array) {
        return SPEC_FLOAT64_ARRAY;
    }
    if class_info.is_sub_class_of(BigInt64Array) {
        return SPEC_BIG_INT64_ARRAY;
    }
    if class_info.is_sub_class_of(BigUint64Array) {
        return SPEC_BIG_UINT64_ARRAY;
    }
    if class_info.is_sub_class_of(JSObject) {
        return SPEC_OBJECT_OTHER;
    }
    SPEC_CELL_OTHER
}

// ================================== Dump ====================================

/// `dumpSpeculation(PrintStream&, SpeculatedType)` (SpeculatedType.cpp:77-366),
/// rendered to a `String`. The branch structure and `isTop` bookkeeping are
/// preserved exactly; the C++ `CommaPrinter("|")` is the `"|"`-joined `pieces`.
pub fn dump_speculation(value: SpeculatedType) -> String {
    if value == SPEC_NONE {
        return "None".to_string();
    }

    let mut pieces: Vec<&'static str> = Vec::new();
    let mut is_top = true;

    if (value & SPEC_CELL) == SPEC_CELL {
        pieces.push("Cell");
    } else {
        if (value & SPEC_OBJECT) == SPEC_OBJECT {
            pieces.push("Object");
        } else {
            if value & SPEC_CELL_OTHER != 0 {
                pieces.push("OtherCell");
            } else {
                is_top = false;
            }
            if value & SPEC_OBJECT_OTHER != 0 {
                pieces.push("OtherObj");
            } else {
                is_top = false;
            }
            if value & SPEC_FINAL_OBJECT != 0 {
                pieces.push("Final");
            } else {
                is_top = false;
            }
            if value & SPEC_ARRAY != 0 {
                pieces.push("Array");
            } else {
                is_top = false;
            }
            if value & SPEC_INT8_ARRAY != 0 {
                pieces.push("Int8Array");
            } else {
                is_top = false;
            }
            if value & SPEC_INT16_ARRAY != 0 {
                pieces.push("Int16Array");
            } else {
                is_top = false;
            }
            if value & SPEC_INT32_ARRAY != 0 {
                pieces.push("Int32Array");
            } else {
                is_top = false;
            }
            if value & SPEC_UINT8_ARRAY != 0 {
                pieces.push("Uint8Array");
            } else {
                is_top = false;
            }
            if value & SPEC_UINT8_CLAMPED_ARRAY != 0 {
                pieces.push("Uint8ClampedArray");
            } else {
                is_top = false;
            }
            if value & SPEC_UINT16_ARRAY != 0 {
                pieces.push("Uint16Array");
            } else {
                is_top = false;
            }
            if value & SPEC_UINT32_ARRAY != 0 {
                pieces.push("Uint32Array");
            } else {
                is_top = false;
            }
            if value & SPEC_FLOAT16_ARRAY != 0 {
                pieces.push("Float16array");
            } else {
                is_top = false;
            }
            if value & SPEC_FLOAT32_ARRAY != 0 {
                pieces.push("Float32array");
            } else {
                is_top = false;
            }
            if value & SPEC_FLOAT64_ARRAY != 0 {
                pieces.push("Float64Array");
            } else {
                is_top = false;
            }
            if value & SPEC_BIG_INT64_ARRAY != 0 {
                pieces.push("BigInt64Array");
            } else {
                is_top = false;
            }
            if value & SPEC_BIG_UINT64_ARRAY != 0 {
                pieces.push("BigUint64Array");
            } else {
                is_top = false;
            }
            if value & SPEC_FUNCTION != 0 {
                pieces.push("Function");
            } else {
                is_top = false;
            }
            if value & SPEC_DIRECT_ARGUMENTS != 0 {
                pieces.push("DirectArguments");
            } else {
                is_top = false;
            }
            if value & SPEC_SCOPED_ARGUMENTS != 0 {
                pieces.push("ScopedArguments");
            } else {
                is_top = false;
            }
            if value & SPEC_STRING_OBJECT != 0 {
                pieces.push("StringObject");
            } else {
                is_top = false;
            }
            if value & SPEC_REG_EXP_OBJECT != 0 {
                pieces.push("RegExpObject");
            } else {
                is_top = false;
            }
            if value & SPEC_DATE_OBJECT != 0 {
                pieces.push("DateObject");
            } else {
                is_top = false;
            }
            if value & SPEC_PROMISE_OBJECT != 0 {
                pieces.push("PromiseObject");
            } else {
                is_top = false;
            }
            if value & SPEC_MAP_OBJECT != 0 {
                pieces.push("MapObject");
            } else {
                is_top = false;
            }
            if value & SPEC_SET_OBJECT != 0 {
                pieces.push("SetObject");
            } else {
                is_top = false;
            }
            if value & SPEC_WEAK_MAP_OBJECT != 0 {
                pieces.push("WeakMapObject");
            } else {
                is_top = false;
            }
            if value & SPEC_WEAK_SET_OBJECT != 0 {
                pieces.push("WeakSetObject");
            } else {
                is_top = false;
            }
            if value & SPEC_PROXY_OBJECT != 0 {
                pieces.push("ProxyObject");
            } else {
                is_top = false;
            }
            if value & SPEC_GLOBAL_PROXY != 0 {
                pieces.push("GlobalProxy");
            } else {
                is_top = false;
            }
            if value & SPEC_DERIVED_ARRAY != 0 {
                pieces.push("DerivedArray");
            } else {
                is_top = false;
            }
            if value & SPEC_DATA_VIEW_OBJECT != 0 {
                pieces.push("DataView");
            } else {
                is_top = false;
            }
        }

        if (value & SPEC_STRING) == SPEC_STRING {
            pieces.push("String");
        } else if (value & SPEC_STRING_RESOLVED) == SPEC_STRING_RESOLVED {
            pieces.push("StringResolved");
        } else {
            if value & SPEC_STRING_IDENT != 0 {
                pieces.push("StringIdent");
            } else {
                is_top = false;
            }
            if (value & SPEC_STRING_VAR) == SPEC_STRING_VAR {
                pieces.push("StringVar");
            } else {
                if value & SPEC_STRING_RESOLVED_VAR != 0 {
                    pieces.push("StringResolvedVar");
                } else {
                    is_top = false;
                }
                if value & SPEC_STRING_UNRESOLVED_VAR != 0 {
                    pieces.push("StringUnresolvedVar");
                } else {
                    is_top = false;
                }
            }
        }

        if value & SPEC_SYMBOL != 0 {
            pieces.push("Symbol");
        } else {
            is_top = false;
        }

        if value & SPEC_HEAP_BIG_INT != 0 {
            pieces.push("HeapBigInt");
        } else {
            is_top = false;
        }
    }

    if value == SPEC_INT32_ONLY {
        pieces.push("Int32");
    } else {
        if value & SPEC_BOOL_INT32 != 0 {
            pieces.push("BoolInt32");
        } else {
            is_top = false;
        }
        if value & SPEC_NON_BOOL_INT32 != 0 {
            pieces.push("NonBoolInt32");
        } else {
            is_top = false;
        }
    }

    if (value & SPEC_BYTECODE_DOUBLE) == SPEC_BYTECODE_DOUBLE {
        pieces.push("BytecodeDouble");
    } else {
        if value & SPEC_ANY_INT_AS_DOUBLE != 0 {
            pieces.push("AnyIntAsDouble");
        } else {
            is_top = false;
        }
        if value & SPEC_NON_INT_AS_DOUBLE != 0 {
            pieces.push("NonIntAsDouble");
        } else {
            is_top = false;
        }
        if value & SPEC_DOUBLE_PURE_NAN != 0 {
            pieces.push("DoublePureNaN");
        } else {
            is_top = false;
        }
    }

    // USE(BIGINT32) is off in this configuration, so the `SpecBigInt32` block
    // (SpeculatedType.cpp:321-326) is omitted, matching the `!USE(BIGINT32)`
    // build (it does not touch `isTop`).

    if value & SPEC_DOUBLE_IMPURE_NAN != 0 {
        pieces.push("DoubleImpureNaN");
    }

    if value & SPEC_BOOLEAN != 0 {
        pieces.push("Bool");
    } else {
        is_top = false;
    }

    if value & SPEC_OTHER != 0 {
        pieces.push("Other");
    } else {
        is_top = false;
    }

    if value & SPEC_EMPTY != 0 {
        pieces.push("Empty");
    } else {
        is_top = false;
    }

    if value & SPEC_INT52_ANY != 0 {
        if (value & SPEC_INT52_ANY) == SPEC_INT52_ANY {
            pieces.push("Int52Any");
        } else if value & SPEC_INT32_AS_INT52 != 0 {
            pieces.push("Int32AsInt52");
        } else if value & SPEC_NON_INT32_AS_INT52 != 0 {
            pieces.push("NonInt32AsInt52");
        }
    } else {
        is_top = false;
    }

    if value == SPEC_BYTECODE_TOP {
        "BytecodeTop".to_string()
    } else if value == SPEC_HEAP_TOP {
        "HeapTop".to_string()
    } else if value == SPEC_FULL_TOP {
        "FullTop".to_string()
    } else if is_top {
        "Top".to_string()
    } else {
        pieces.join("|")
    }
}

/// `speculationToAbbreviatedString(SpeculatedType)` (SpeculatedType.cpp:370-441),
/// exposed by `dumpSpeculationAbbreviated` (SpeculatedType.cpp:443-446).
pub fn speculation_to_abbreviated_string(prediction: SpeculatedType) -> &'static str {
    if is_final_object_speculation(prediction) {
        return "<Final>";
    }
    if is_array_speculation(prediction) {
        return "<Array>";
    }
    if is_string_ident_speculation(prediction) {
        return "<StringIdent>";
    }
    if is_string_speculation(prediction) {
        return "<String>";
    }
    if is_function_speculation(prediction) {
        return "<Function>";
    }
    if is_int8_array_speculation(prediction) {
        return "<Int8array>";
    }
    if is_int16_array_speculation(prediction) {
        return "<Int16array>";
    }
    if is_int32_array_speculation(prediction) {
        return "<Int32array>";
    }
    if is_uint8_array_speculation(prediction) {
        return "<Uint8array>";
    }
    if is_uint16_array_speculation(prediction) {
        return "<Uint16array>";
    }
    if is_uint32_array_speculation(prediction) {
        return "<Uint32array>";
    }
    if is_float16_array_speculation(prediction) {
        return "<Float16array>";
    }
    if is_float32_array_speculation(prediction) {
        return "<Float32array>";
    }
    if is_float64_array_speculation(prediction) {
        return "<Float64array>";
    }
    if is_big_int64_array_speculation(prediction) {
        return "<BigInt64array>";
    }
    if is_big_uint64_array_speculation(prediction) {
        return "<BigUint64array>";
    }
    if is_direct_arguments_speculation(prediction) {
        return "<DirectArguments>";
    }
    if is_scoped_arguments_speculation(prediction) {
        return "<ScopedArguments>";
    }
    if is_string_object_speculation(prediction) {
        return "<StringObject>";
    }
    if is_reg_exp_object_speculation(prediction) {
        return "<RegExpObject>";
    }
    if is_string_or_string_object_speculation(prediction) {
        return "<StringOrStringObject>";
    }
    if is_object_speculation(prediction) {
        return "<Object>";
    }
    if is_cell_speculation(prediction) {
        return "<Cell>";
    }
    if is_bool_int32_speculation(prediction) {
        return "<BoolInt32>";
    }
    if is_int32_speculation(prediction) {
        return "<Int32>";
    }
    if is_any_int_as_double_speculation(prediction) {
        return "<AnyIntAsDouble>";
    }
    if prediction == SPEC_NON_INT32_AS_INT52 {
        return "<NonInt32AsInt52>";
    }
    if prediction == SPEC_INT32_AS_INT52 {
        return "<Int32AsInt52>";
    }
    if is_any_int52_speculation(prediction) {
        return "<Int52Any>";
    }
    if is_double_speculation(prediction) {
        return "<Double>";
    }
    if is_full_number_speculation(prediction) {
        return "<Number>";
    }
    if is_boolean_speculation(prediction) {
        return "<Boolean>";
    }
    if is_other_speculation(prediction) {
        return "<Other>";
    }
    if is_misc_speculation(prediction) {
        return "<Misc>";
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::static_value_representation_layout;

    // ---- Flag constants pinned to SpeculatedType.h ----

    #[test]
    fn flag_constants_match_header_bit_positions() {
        // SpeculatedType.h:48-50.
        assert_eq!(SPEC_FINAL_OBJECT, 1 << 0);
        assert_eq!(SPEC_ARRAY, 1 << 1);
        assert_eq!(SPEC_FUNCTION, 1 << 2);
        // Bit 3 is unused; SpecInt8Array is 1<<4 (SpeculatedType.h:51).
        assert_eq!(SPEC_INT8_ARRAY, 1 << 4);
        // SpeculatedType.h:79,87.
        assert_eq!(SPEC_OBJECT_OTHER, 1 << 31);
        assert_eq!(SPEC_CELL_OTHER, 1 << 36);
        // SpeculatedType.h:88-90.
        assert_eq!(SPEC_BOOL_INT32, 1 << 37);
        assert_eq!(SPEC_NON_BOOL_INT32, 1 << 38);
        assert_eq!(SPEC_INT32_ONLY, (1 << 37) | (1 << 38));
        // SpeculatedType.h:92-94.
        assert_eq!(SPEC_INT52_ANY, (1 << 39) | (1 << 40));
        // SpeculatedType.h:96-98.
        assert_eq!(SPEC_DOUBLE_REAL, (1 << 42) | (1 << 41));
        // SpeculatedType.h:113-115,122.
        assert_eq!(SPEC_EMPTY, 1 << 47);
        assert_eq!(SPEC_HEAP_BIG_INT, 1 << 48);
        assert_eq!(SPEC_BIG_INT32, 1 << 49);
        assert_eq!(SPEC_DATA_VIEW_OBJECT, 1 << 50);
    }

    #[test]
    fn union_masks_match_header() {
        // SpeculatedType.h:85 SpecString = ident | unresolvedVar | resolvedVar.
        assert_eq!(
            SPEC_STRING,
            (1 << 32) | (1 << 33) | (1 << 34),
            "SpeculatedType.h:80-85"
        );
        // SpeculatedType.h:106 SpecBytecodeNumber = Int32Only | BytecodeDouble.
        assert_eq!(SPEC_BYTECODE_NUMBER, SPEC_INT32_ONLY | SPEC_BYTECODE_DOUBLE);
        // SpeculatedType.h:120 !USE(BIGINT32) => SpecBigInt = SpecHeapBigInt only.
        assert_eq!(SPEC_BIG_INT, SPEC_HEAP_BIG_INT);
        // SpeculatedType.h:124 object union excludes the primitive flags.
        assert_eq!(SPEC_OBJECT & SPEC_STRING, 0);
        assert_eq!(SPEC_OBJECT & SPEC_CELL_OTHER, 0);
        // SpeculatedType.h:125 SpecCell = Object|String|Symbol|CellOther|HeapBigInt.
        assert_eq!(
            SPEC_CELL,
            SPEC_OBJECT | SPEC_STRING | SPEC_SYMBOL | SPEC_CELL_OTHER | SPEC_HEAP_BIG_INT
        );
        // SpeculatedType.h:126-127.
        assert_eq!(
            SPEC_HEAP_TOP,
            SPEC_CELL | SPEC_BIG_INT32 | SPEC_BYTECODE_NUMBER | SPEC_MISC
        );
        assert_eq!(SPEC_BYTECODE_TOP, SPEC_HEAP_TOP | SPEC_EMPTY);
        // SpeculatedType.h:135 64-bit arm: SpecCellCheck = SpecCell | SpecEmpty.
        assert_eq!(SPEC_CELL_CHECK, SPEC_CELL | SPEC_EMPTY);
        // SpeculatedType.h:63 typed-array view contains all 12 view flags.
        assert!(SPEC_TYPED_ARRAY_VIEW & SPEC_INT8_ARRAY != 0);
        assert!(SPEC_TYPED_ARRAY_VIEW & SPEC_BIG_UINT64_ARRAY != 0);
        assert_eq!(SPEC_TYPED_ARRAY_VIEW.count_ones(), 12);
    }

    // ---- Predicates pinned to SpeculatedType.h ----

    #[test]
    fn predicates_match_header_semantics() {
        // SpeculatedType.h:145,150.
        assert!(is_subtype_speculation(SPEC_ARRAY, SPEC_OBJECT));
        assert!(!is_subtype_speculation(SPEC_STRING, SPEC_OBJECT));
        assert!(!is_subtype_speculation(SPEC_NONE, SPEC_OBJECT));
        assert!(speculation_contains(SPEC_ARRAY | SPEC_STRING, SPEC_OBJECT));
        assert!(!speculation_contains(SPEC_STRING, SPEC_OBJECT));
        // SpeculatedType.h:155,175.
        assert!(is_cell_speculation(SPEC_STRING));
        assert!(!is_cell_speculation(SPEC_INT32_ONLY));
        assert!(is_object_speculation(SPEC_ARRAY));
        assert!(!is_object_speculation(SPEC_STRING));
        // SpeculatedType.h:360,365.
        assert!(is_bool_int32_speculation(SPEC_BOOL_INT32));
        assert!(is_int32_speculation(SPEC_BOOL_INT32));
        assert!(is_int32_speculation(SPEC_INT32_ONLY));
        assert!(!is_int32_speculation(SPEC_NONE));
        // SpeculatedType.h:510,505.
        assert!(is_empty_speculation(SPEC_EMPTY));
        assert!(is_other_or_empty_speculation(SPEC_NONE));
        assert!(is_other_or_empty_speculation(SPEC_OTHER));
    }

    // ---- Merge protocol pinned to SpeculatedType.h:541-558 ----

    #[test]
    fn merge_is_bitwise_or_and_reports_change() {
        assert_eq!(
            merge_speculations(SPEC_INT32_ONLY, SPEC_DOUBLE_REAL),
            SPEC_INT32_ONLY | SPEC_DOUBLE_REAL
        );

        let mut acc = SPEC_INT32_ONLY;
        assert!(merge_speculation(&mut acc, SPEC_DOUBLE_REAL)); // changed
        assert_eq!(acc, SPEC_INT32_ONLY | SPEC_DOUBLE_REAL);
        assert!(!merge_speculation(&mut acc, SPEC_BOOL_INT32)); // already present
        assert_eq!(acc, SPEC_INT32_ONLY | SPEC_DOUBLE_REAL);

        // SpeculatedType.h:555.
        assert!(speculation_checked(SPEC_ARRAY, SPEC_OBJECT));
        assert!(!speculation_checked(SPEC_OBJECT, SPEC_ARRAY));
    }

    // ---- Producers pinned to SpeculatedType.cpp ----

    #[test]
    fn speculation_from_value_immediate_arms() {
        // SpeculatedType.cpp:624-628.
        assert_eq!(
            speculation_from_value(JsValue::from_i32(0), |_| SPEC_NONE),
            SPEC_BOOL_INT32
        );
        assert_eq!(
            speculation_from_value(JsValue::from_i32(1), |_| SPEC_NONE),
            SPEC_BOOL_INT32
        );
        assert_eq!(
            speculation_from_value(JsValue::from_i32(5), |_| SPEC_NONE),
            SPEC_NON_BOOL_INT32
        );
        // SpeculatedType.cpp:629-635. A NaN -> pure NaN; a non-int double ->
        // NonIntAsDouble; an out-of-int32-range integral double stays a double
        // and is anyInt -> AnyIntAsDouble.
        assert_eq!(
            speculation_from_value(JsValue::from_double(f64::NAN), |_| SPEC_NONE),
            SPEC_DOUBLE_PURE_NAN
        );
        assert_eq!(
            speculation_from_value(JsValue::from_double(2.5), |_| SPEC_NONE),
            SPEC_NON_INT_AS_DOUBLE
        );
        assert_eq!(
            speculation_from_value(JsValue::from_double(2.0_f64.powi(40)), |_| SPEC_NONE),
            SPEC_ANY_INT_AS_DOUBLE
        );
        // SpeculatedType.cpp:641-644.
        assert_eq!(
            speculation_from_value(JsValue::from_bool(true), |_| SPEC_NONE),
            SPEC_BOOLEAN
        );
        assert_eq!(
            speculation_from_value(JsValue::undefined(), |_| SPEC_NONE),
            SPEC_OTHER
        );
        assert_eq!(
            speculation_from_value(JsValue::null(), |_| SPEC_NONE),
            SPEC_OTHER
        );
        // SpeculatedType.cpp:622-623: the empty marker.
        assert_eq!(
            speculation_from_value(JsValue::default(), |_| SPEC_NONE),
            SPEC_EMPTY
        );
    }

    #[test]
    fn speculation_from_value_cell_arm_delegates_to_resolver() {
        // SpeculatedType.cpp:639-640: a cell defers to speculationFromCell. The
        // resolver stands in for the heap-reachable cell classification.
        let cell = JsValue::from_encoded(
            static_value_representation_layout()
                .encode_cell_payload(0x1234)
                .unwrap(),
        );
        assert!(cell.is_cell());
        assert_eq!(speculation_from_value(cell, |_| SPEC_ARRAY), SPEC_ARRAY);
    }

    #[test]
    fn int52_aware_speculation_from_value_uses_int52_lattice() {
        // SpeculatedType.cpp:647-656.
        assert_eq!(
            int52_aware_speculation_from_value(JsValue::from_i32(5), |_| SPEC_NONE),
            SPEC_INT32_AS_INT52
        );
        // 2^40 is an anyInt that does not fit in int32 -> NonInt32AsInt52.
        assert_eq!(
            int52_aware_speculation_from_value(JsValue::from_double(2.0_f64.powi(40)), |_| {
                SPEC_NONE
            }),
            SPEC_NON_INT32_AS_INT52
        );
        // 2.5 is not an anyInt -> falls back to speculation_from_value.
        assert_eq!(
            int52_aware_speculation_from_value(JsValue::from_double(2.5), |_| SPEC_NONE),
            SPEC_NON_INT_AS_DOUBLE
        );
    }

    #[test]
    fn speculation_from_js_type_matches_switch() {
        // SpeculatedType.cpp:703-708 + default (700-739).
        assert_eq!(speculation_from_js_type(JsType::String), Some(SPEC_STRING));
        assert_eq!(speculation_from_js_type(JsType::Symbol), Some(SPEC_SYMBOL));
        assert_eq!(
            speculation_from_js_type(JsType::HeapBigInt),
            Some(SPEC_HEAP_BIG_INT)
        );
        // ObjectType/FinalObjectType hit the `default` -> nullopt.
        assert_eq!(speculation_from_js_type(JsType::Object), None);
        assert_eq!(speculation_from_js_type(JsType::FinalObject), None);
    }

    #[test]
    fn structure_mapping_matches_speculated_type_mapping() {
        // SpeculatedType.cpp:578-592 with FOR_EACH_JS_TYPE rows (JSType.h:37,77,78).
        assert_eq!(
            speculation_from_structure(JsType::FinalObject),
            SPEC_FINAL_OBJECT
        );
        assert_eq!(
            speculation_from_structure(JsType::Object),
            SPEC_OBJECT_OTHER
        );
        assert_eq!(speculation_from_structure(JsType::String), SPEC_STRING);
    }

    #[test]
    fn cell_string_sub_classification() {
        // SpeculatedType.cpp:602-617.
        assert_eq!(
            speculation_from_cell(JsType::String, Some(StringSpeculationKind::Atom)),
            SPEC_STRING_IDENT
        );
        assert_eq!(
            speculation_from_cell(JsType::String, Some(StringSpeculationKind::ResolvedNonAtom)),
            SPEC_STRING_RESOLVED
        );
        assert_eq!(
            speculation_from_cell(JsType::String, Some(StringSpeculationKind::Rope)),
            SPEC_STRING
        );
        // Non-string cell falls to the JSType mapping.
        assert_eq!(
            speculation_from_cell(JsType::FinalObject, None),
            SPEC_FINAL_OBJECT
        );
    }

    /// Minimal `ClassInfoInheritance` mock keyed on a single matching leaf, used
    /// to pin the branch order of `speculation_from_class_info_inheritance`.
    struct MockClassInfo {
        exact: Option<ClassInfoLeaf>,
        sub_class_of: Option<ClassInfoLeaf>,
    }

    impl ClassInfoInheritance for MockClassInfo {
        fn is(&self, leaf: ClassInfoLeaf) -> bool {
            self.exact == Some(leaf)
        }
        fn is_sub_class_of(&self, leaf: ClassInfoLeaf) -> bool {
            self.sub_class_of == Some(leaf)
        }
    }

    #[test]
    fn class_info_inheritance_branch_returns() {
        // SpeculatedType.cpp:485-486.
        let s = MockClassInfo {
            exact: Some(ClassInfoLeaf::JSString),
            sub_class_of: None,
        };
        assert_eq!(speculation_from_class_info_inheritance(&s), SPEC_STRING);

        // SpeculatedType.cpp:523: JSMapIterator returns SpecObjectOther, NOT
        // SpecMapIteratorObject.
        let it = MockClassInfo {
            exact: Some(ClassInfoLeaf::JSMapIterator),
            sub_class_of: None,
        };
        assert_eq!(
            speculation_from_class_info_inheritance(&it),
            SPEC_OBJECT_OTHER
        );

        // SpeculatedType.cpp:554-555: JSArray subclass returns Array|DerivedArray.
        let arr = MockClassInfo {
            exact: None,
            sub_class_of: Some(ClassInfoLeaf::JSArray),
        };
        assert_eq!(
            speculation_from_class_info_inheritance(&arr),
            SPEC_ARRAY | SPEC_DERIVED_ARRAY
        );

        // SpeculatedType.cpp:570-571: a plain JSObject subclass.
        let obj = MockClassInfo {
            exact: None,
            sub_class_of: Some(ClassInfoLeaf::JSObject),
        };
        assert_eq!(
            speculation_from_class_info_inheritance(&obj),
            SPEC_OBJECT_OTHER
        );

        // SpeculatedType.cpp:573: the final fallthrough.
        let none = MockClassInfo {
            exact: None,
            sub_class_of: None,
        };
        assert_eq!(
            speculation_from_class_info_inheritance(&none),
            SPEC_CELL_OTHER
        );
    }

    // ---- Dump pinned to SpeculatedType.cpp ----

    #[test]
    fn dump_speculation_renders_named_forms() {
        // SpeculatedType.cpp:83-86.
        assert_eq!(dump_speculation(SPEC_NONE), "None");
        // SpeculatedType.cpp:288-289.
        assert_eq!(dump_speculation(SPEC_INT32_ONLY), "Int32");
        // SpeculatedType.cpp:111-114.
        assert_eq!(dump_speculation(SPEC_ARRAY), "Array");
        // SpeculatedType.cpp:356-357 named tops.
        assert_eq!(dump_speculation(SPEC_BYTECODE_TOP), "BytecodeTop");
        assert_eq!(dump_speculation(SPEC_HEAP_TOP), "HeapTop");
        assert_eq!(dump_speculation(SPEC_FULL_TOP), "FullTop");
        // SpeculatedType.cpp:90-91.
        assert_eq!(dump_speculation(SPEC_CELL), "Cell");
        // Abbreviated (SpeculatedType.cpp:374,414,416).
        assert_eq!(speculation_to_abbreviated_string(SPEC_ARRAY), "<Array>");
        assert_eq!(speculation_to_abbreviated_string(SPEC_OBJECT), "<Object>");
        assert_eq!(speculation_to_abbreviated_string(SPEC_CELL), "<Cell>");
    }
}
