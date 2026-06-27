//! Faithful Rust mirror of JSC's `enum JSType : uint8_t` (runtime/JSType.h:164).
//!
//! C++ JSC ground truth: every `JSCell` carries a one-byte `JSType m_type` in its
//! header (runtime/JSCell.h:298), read at a fixed offset via
//! `JSCell::typeInfoTypeOffset()` (runtime/JSCell.h:246-248) and exposed by
//! `JSType type() const { return m_type; }` (runtime/JSCell.h:154). The type tag
//! is what makes `isString()/isHeapBigInt()/isSymbol()` direct equality compares
//! (runtime/JSCell.h:127-129) and `isObject()` a `m_type >= ObjectType` range
//! check (runtime/JSType.h:204) — i.e. the in-cell tag that lets code decide a
//! cell's kind BEFORE downcasting/dereferencing it as a concrete subclass.
//!
//! This is the faithful analog of `JSCell::m_type`. It is intentionally DISTINCT
//! from the Rust-only coarse heap-side tag `gc::CellType` (src/gc/cell.rs:50),
//! which lives in the heap metadata side table (`CellMetadata.type_info.cell_type`)
//! rather than inside the cell at a fixed offset. `cell_type()` documents the
//! bridge between the two; folding `gc::CellType` into this faithful tag is a
//! later step and deliberately NOT done here.

use crate::gc::CellType;

/// Faithful mirror of JSC `enum JSType : uint8_t` (runtime/JSType.h:164),
/// restricted to the cell kinds this port currently allocates. The u8
/// discriminants are the TRUE C++ positional values from `FOR_EACH_JS_TYPE`
/// (runtime/JSType.h:30-161) so that the `>= ObjectType` object-range predicate
/// (runtime/JSType.h:204) stays valid as more kinds are added.
///
/// Only the kinds with a corresponding cell in the port are listed; new kinds
/// are added as the port grows cells for them (per the "keep it small, named for
/// the JSC concept" contract), never invented ahead of need.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Default)]
pub enum JsType {
    /// JSC `StringType` (runtime/JSType.h:37). `JSCell::isString()`.
    String = 2,
    /// JSC `HeapBigIntType` (runtime/JSType.h:38). `JSCell::isHeapBigInt()`.
    HeapBigInt = 3,
    /// JSC `SymbolType` (runtime/JSType.h:40). `JSCell::isSymbol()`.
    Symbol = 4,
    /// JSC `ObjectType` (runtime/JSType.h:77): the first JSObject type. Used as
    /// the object-range umbrella for object kinds whose faithful per-subclass
    /// JSType (ArrayType/JSFunctionType/JSPromiseType/...) is not yet modeled;
    /// `is_object()` keys off `>= Object` exactly like C++ `>= ObjectType`.
    Object = 32,
    /// JSC `FinalObjectType` (runtime/JSType.h:78): a plain ordinary `{}` object.
    /// This is the `#[default]` ONLY to satisfy `#[derive(Default)]` on the
    /// primitive cells (CoreStringCell/CoreSymbolCell); every real primitive
    /// constructor sets `js_type` explicitly, so this default is never published.
    #[default]
    FinalObject = 33,
}

impl JsType {
    /// C++ `TypeInfo::isObject(type)` / `isObjectType` == `type >= ObjectType`
    /// (runtime/JSType.h:204, runtime/JSTypeInfo.h:87-88). The object range is a
    /// half-open tail of the enum, so a single `>=` compares against the umbrella.
    pub fn is_object(self) -> bool {
        (self as u8) >= JsType::Object as u8
    }

    /// Bridge from the faithful in-cell `JSCell::m_type` analog to the Rust-only
    /// coarse heap-side `gc::CellType` (src/gc/cell.rs:50) each cell kind is
    /// published with. Used by the cross-check that proves the new in-cell header
    /// agrees with the pre-existing heap-side type discrimination. This bridge
    /// exists because the two tags are deliberately kept separate in this step;
    /// reconciling them is deferred.
    pub fn cell_type(self) -> CellType {
        match self {
            JsType::String => CellType::String,
            JsType::Symbol => CellType::Symbol,
            JsType::HeapBigInt => CellType::BigInt,
            JsType::Object | JsType::FinalObject => CellType::Object,
        }
    }
}
