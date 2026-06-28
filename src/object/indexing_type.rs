//! IndexingType: the one-byte indexing descriptor for objects/arrays.
//!
//! Faithful port of `runtime/IndexingType.h` (WebKit/Source/JavaScriptCore).
//! C++ JSC is the source of truth; line anchors below point at
//! IndexingType.h:63-228.
//!
//! Conceptually the byte is laid out as (IndexingType.h:40-54):
//! ```text
//!   bit 0      isArray
//!   bit 1-3    shape (3 bits; NOT a dense enum — see shape constants)
//!   bit 4      copyOnWrite
//!   bit 5      mayHaveIndexedAccessors
//!   bit 6-7    cellLock bits (IndexingTypeLockIsHeld / ...HasParked)
//! ```
//! `IndexingType` (bits 0-3) ⊂ `IndexingMode` (bits 0-4) ⊂
//! `IndexingModeIncludingHistory` (bits 0-5) ⊂ `IndexingTypeAndMisc` (bits 0-7).
//!
//! TARGET: this byte is the value stored in the arena JSCell header at
//! `gc::heap::marked_block::JsCellHeader::indexing_type_and_misc` (offset @4,
//! `m_indexingTypeAndMisc`; marked_block.rs:147). This unit only defines the
//! byte vocabulary and predicates; wiring the header field to it is a later
//! unit, so the API is intentionally unreferenced for now (see the
//! `allow(dead_code)` below, matching the not-yet-wired S4 core convention).

// Not wired into the JSCell header yet (the S4 cell-arena core landed but is not
// hooked up). Same convention as the other landed-but-unwired heap modules
// (e.g. marked_block.rs:89). Remove once a consumer references this vocabulary.
#![allow(dead_code)]

use super::structure::IndexingMode;

/// `typedef uint8_t IndexingType;` (IndexingType.h:63). The whole byte is reused
/// for shape, copy-on-write, history, and lock bits; callers mask for the
/// portion they want.
pub type IndexingType = u8;

// ============================ Capability flag ============================

/// `IsArray` (IndexingType.h:66) — bit 0. Set iff the cell is a JSArray.
pub const IS_ARRAY: IndexingType = 0x01;

// ===================== Shape of the indexed storage =====================
//
// The shape lives in bits 1-3. The values are an *enumeration of shapes that is
// deliberately not sequential* (IndexingType.h:56-58): e.g. there is no 0x0E
// shape, and DoubleShape (0x06) sits between Int32Shape (0x04) and
// ContiguousShape (0x08). They are therefore NOT bitwise-exclusive and must be
// compared after masking with `INDEXING_SHAPE_MASK`, never tested by `&`.

/// `NoIndexingShape` (IndexingType.h:69) — no indexed storage.
pub const NO_INDEXING_SHAPE: IndexingType = 0x00;
/// `UndecidedShape` (IndexingType.h:70) — array exists but element type is not
/// yet decided. Only useful for arrays.
pub const UNDECIDED_SHAPE: IndexingType = 0x02;
/// `Int32Shape` (IndexingType.h:71).
pub const INT32_SHAPE: IndexingType = 0x04;
/// `DoubleShape` (IndexingType.h:72).
pub const DOUBLE_SHAPE: IndexingType = 0x06;
/// `ContiguousShape` (IndexingType.h:73).
pub const CONTIGUOUS_SHAPE: IndexingType = 0x08;
/// `ArrayStorageShape` (IndexingType.h:74).
pub const ARRAY_STORAGE_SHAPE: IndexingType = 0x0A;
/// `SlowPutArrayStorageShape` (IndexingType.h:75).
pub const SLOW_PUT_ARRAY_STORAGE_SHAPE: IndexingType = 0x0C;

/// `IndexingShapeMask` (IndexingType.h:77) — bits 1-3 select the shape.
pub const INDEXING_SHAPE_MASK: IndexingType = 0x0E;
/// `IndexingShapeShift` (IndexingType.h:78) — right-shift to densify a shape
/// into a 0-based index (used by `array_index_from_indexing_type`).
pub const INDEXING_SHAPE_SHIFT: IndexingType = 1;
/// `NumberOfIndexingShapes` (IndexingType.h:79).
pub const NUMBER_OF_INDEXING_SHAPES: IndexingType = 7;
/// `IndexingTypeMask` (IndexingType.h:80) — shape bits plus the IsArray bit.
pub const INDEXING_TYPE_MASK: IndexingType = INDEXING_SHAPE_MASK | IS_ARRAY;

// ===================== Copy-on-write / indexing mode =====================

/// `CopyOnWrite` (IndexingType.h:83) — bit 4. When set, the butterfly is a
/// shared `JSCellButterfly`; only ever set when there are no named properties.
pub const COPY_ON_WRITE: IndexingType = 0x10;
/// `IndexingShapeAndWritabilityMask` (IndexingType.h:84).
pub const INDEXING_SHAPE_AND_WRITABILITY_MASK: IndexingType = COPY_ON_WRITE | INDEXING_SHAPE_MASK;
/// `IndexingModeMask` (IndexingType.h:85) — IsArray + shape + copyOnWrite.
pub const INDEXING_MODE_MASK: IndexingType = COPY_ON_WRITE | INDEXING_TYPE_MASK;
/// `NumberOfCopyOnWriteIndexingModes` (IndexingType.h:86) — only int32, double,
/// and contiguous shapes have a copy-on-write form.
pub const NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES: IndexingType = 3;
/// `NumberOfArrayIndexingModes` (IndexingType.h:87).
pub const NUMBER_OF_ARRAY_INDEXING_MODES: IndexingType =
    NUMBER_OF_INDEXING_SHAPES + NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES;

// ===================== History bit (usually masked off) =====================

/// `MayHaveIndexedAccessors` (IndexingType.h:93) — bit 5. Tracks whether the
/// prototype chain may intercept indexed properties.
pub const MAY_HAVE_INDEXED_ACCESSORS: IndexingType = 0x20;

// ===================== Lock bits (top two bits) =====================
//
// The IndexingType byte is stolen for a cell lock (IndexingType.h:95-98). The
// `LockAlgorithm<IndexingType, IndexingTypeLockIsHeld, IndexingTypeLockHasParked>`
// (IndexingType.h:230) is a separate WTF concept and is NOT ported here; only
// the two bit positions are defined so masks that exclude them stay faithful.

/// `IndexingTypeLockIsHeld` (IndexingType.h:97) — bit 6.
pub const INDEXING_TYPE_LOCK_IS_HELD: IndexingType = 0x40;
/// `IndexingTypeLockHasParked` (IndexingType.h:98) — bit 7.
pub const INDEXING_TYPE_LOCK_HAS_PARKED: IndexingType = 0x80;

// ===================== List of acceptable array types =====================
// (IndexingType.h:101-116) Named, fully-formed IndexingType bytes.

/// `NonArray` (IndexingType.h:101).
pub const NON_ARRAY: IndexingType = 0x0;
/// `NonArrayWithInt32` (IndexingType.h:102).
pub const NON_ARRAY_WITH_INT32: IndexingType = INT32_SHAPE;
/// `NonArrayWithDouble` (IndexingType.h:103).
pub const NON_ARRAY_WITH_DOUBLE: IndexingType = DOUBLE_SHAPE;
/// `NonArrayWithContiguous` (IndexingType.h:104).
pub const NON_ARRAY_WITH_CONTIGUOUS: IndexingType = CONTIGUOUS_SHAPE;
/// `NonArrayWithArrayStorage` (IndexingType.h:105).
pub const NON_ARRAY_WITH_ARRAY_STORAGE: IndexingType = ARRAY_STORAGE_SHAPE;
/// `NonArrayWithSlowPutArrayStorage` (IndexingType.h:106).
pub const NON_ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = SLOW_PUT_ARRAY_STORAGE_SHAPE;
/// `ArrayClass` (IndexingType.h:107) — IsArray with no shape decided.
pub const ARRAY_CLASS: IndexingType = IS_ARRAY;
/// `ArrayWithUndecided` (IndexingType.h:108).
pub const ARRAY_WITH_UNDECIDED: IndexingType = IS_ARRAY | UNDECIDED_SHAPE;
/// `ArrayWithInt32` (IndexingType.h:109).
pub const ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE;
/// `ArrayWithDouble` (IndexingType.h:110).
pub const ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE;
/// `ArrayWithContiguous` (IndexingType.h:111).
pub const ARRAY_WITH_CONTIGUOUS: IndexingType = IS_ARRAY | CONTIGUOUS_SHAPE;
/// `ArrayWithArrayStorage` (IndexingType.h:112).
pub const ARRAY_WITH_ARRAY_STORAGE: IndexingType = IS_ARRAY | ARRAY_STORAGE_SHAPE;
/// `ArrayWithSlowPutArrayStorage` (IndexingType.h:113).
pub const ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE: IndexingType = IS_ARRAY | SLOW_PUT_ARRAY_STORAGE_SHAPE;
/// `CopyOnWriteArrayWithInt32` (IndexingType.h:114).
pub const COPY_ON_WRITE_ARRAY_WITH_INT32: IndexingType = IS_ARRAY | INT32_SHAPE | COPY_ON_WRITE;
/// `CopyOnWriteArrayWithDouble` (IndexingType.h:115).
pub const COPY_ON_WRITE_ARRAY_WITH_DOUBLE: IndexingType = IS_ARRAY | DOUBLE_SHAPE | COPY_ON_WRITE;
/// `CopyOnWriteArrayWithContiguous` (IndexingType.h:116).
pub const COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS: IndexingType =
    IS_ARRAY | CONTIGUOUS_SHAPE | COPY_ON_WRITE;

// ===================== Aggregate masks (IndexingType.h:225-228) =====================

/// `AllWritableArrayTypes` (IndexingType.h:225).
pub const ALL_WRITABLE_ARRAY_TYPES: IndexingType = INDEXING_SHAPE_MASK | IS_ARRAY;
/// `AllArrayTypes` (IndexingType.h:226).
pub const ALL_ARRAY_TYPES: IndexingType = ALL_WRITABLE_ARRAY_TYPES | COPY_ON_WRITE;
/// `AllWritableArrayTypesAndHistory` (IndexingType.h:227).
pub const ALL_WRITABLE_ARRAY_TYPES_AND_HISTORY: IndexingType =
    ALL_WRITABLE_ARRAY_TYPES | MAY_HAVE_INDEXED_ACCESSORS;
/// `AllArrayTypesAndHistory` (IndexingType.h:228).
pub const ALL_ARRAY_TYPES_AND_HISTORY: IndexingType = ALL_ARRAY_TYPES | MAY_HAVE_INDEXED_ACCESSORS;

// ============================ Predicates ============================
// Faithful ports of the inline predicates (IndexingType.h:158-213). All operate
// on the masked shape, never on raw bit tests, because the shapes are not
// bitwise-exclusive.

/// `hasIndexedProperties` (IndexingType.h:158-161).
pub const fn has_indexed_properties(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) != NO_INDEXING_SHAPE
}

/// `hasUndecided` (IndexingType.h:163-166).
pub const fn has_undecided(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == UNDECIDED_SHAPE
}

/// `hasInt32` (IndexingType.h:168-171).
pub const fn has_int32(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == INT32_SHAPE
}

/// `hasDouble` (IndexingType.h:173-176).
pub const fn has_double(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == DOUBLE_SHAPE
}

/// `hasContiguous` (IndexingType.h:178-181).
pub const fn has_contiguous(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == CONTIGUOUS_SHAPE
}

/// `hasArrayStorage` (IndexingType.h:183-186).
pub const fn has_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == ARRAY_STORAGE_SHAPE
}

/// `hasAnyArrayStorage` (IndexingType.h:188-191). True for either plain or
/// slow-put array storage; relies on those being the two highest shapes.
pub const fn has_any_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) >= ARRAY_STORAGE_SHAPE
}

/// `hasSlowPutArrayStorage` (IndexingType.h:193-196).
pub const fn has_slow_put_array_storage(indexing_type: IndexingType) -> bool {
    (indexing_type & INDEXING_SHAPE_MASK) == SLOW_PUT_ARRAY_STORAGE_SHAPE
}

/// `shouldUseSlowPut` (IndexingType.h:198-201).
pub const fn should_use_slow_put(indexing_type: IndexingType) -> bool {
    has_slow_put_array_storage(indexing_type)
}

/// `isCopyOnWrite` (IndexingType.h:203-206). `constexpr` in C++; the param is
/// the full indexing mode, not just the shape.
pub const fn is_copy_on_write(indexing_mode: IndexingType) -> bool {
    (indexing_mode & COPY_ON_WRITE) != 0
}

/// `arrayIndexFromIndexingType` (IndexingType.h:208-213). Densifies an
/// indexing type into a 0-based array index in `0..NUMBER_OF_ARRAY_INDEXING_MODES`
/// (writable shapes occupy 0..=6; the three copy-on-write modes occupy 7..=9).
///
/// C++ does this arithmetic in `unsigned` after integer promotion of the
/// `uint8_t` operands; the copy-on-write branch can exceed a byte before the
/// shift, so we widen the masked shape to `u32` to match (no truncation).
pub const fn array_index_from_indexing_type(indexing_type: IndexingType) -> u32 {
    if is_copy_on_write(indexing_type) {
        ((indexing_type & INDEXING_SHAPE_MASK) as u32 - UNDECIDED_SHAPE as u32
            + SLOW_PUT_ARRAY_STORAGE_SHAPE as u32)
            >> INDEXING_SHAPE_SHIFT
    } else {
        ((indexing_type & INDEXING_SHAPE_MASK) as u32) >> INDEXING_SHAPE_SHIFT
    }
}

// ============== Mapping the existing IndexingMode enum onto the byte ==============
//
// `object::structure::IndexingMode` (structure.rs:16-30) is the pre-arena
// stand-in for this byte. It enumerates the `IndexingShapeWithWritability`
// portion (shape bits 1-3 + copyOnWrite bit 4) as Rust variants, PLUS two
// Rust-only variants (`Dictionary`, `IntegerIndexedExotic`) that JSC does NOT
// encode in the IndexingType byte at all:
//
//   * `Dictionary` is a Structure *dictionary kind* (StructureDictionaryKind /
//     `Structure::m_dictionaryKind`), orthogonal to the indexing shape.
//   * `IntegerIndexedExotic` is the typed-array integer-indexed-exotic behavior,
//     which JSC tracks via the cell's JSType + TypedArrayMode, not via the
//     IndexingType shape.
//
// So the mapping is partial: the genuine shapes map onto a shape-with-writability
// byte; the two Rust-only modes have no IndexingType byte and map to `None`.
// This function is the bridge an arena-wiring unit will use; it does not mutate
// the enum (out of scope for this unit). When the JSCell header byte becomes the
// source of truth, the enum's Dictionary/IntegerIndexedExotic cases should move
// to their faithful homes (dictionary kind / typed-array mode) and this partial
// mapping should disappear.
//
// NOTE: the returned byte carries only the shape + copyOnWrite bits (i.e. the
// `IndexingShapeAndWritabilityMask` portion); it does NOT set `IS_ARRAY`, since
// the enum does not record array-ness. Callers OR in `IS_ARRAY` themselves.
pub const fn indexing_shape_and_writability_for_mode(mode: IndexingMode) -> Option<IndexingType> {
    let byte = match mode {
        IndexingMode::None => NO_INDEXING_SHAPE,
        IndexingMode::UndecidedArray => UNDECIDED_SHAPE,
        IndexingMode::Int32 => INT32_SHAPE,
        IndexingMode::Double => DOUBLE_SHAPE,
        IndexingMode::Contiguous => CONTIGUOUS_SHAPE,
        IndexingMode::ArrayStorage => ARRAY_STORAGE_SHAPE,
        IndexingMode::SlowPutArrayStorage => SLOW_PUT_ARRAY_STORAGE_SHAPE,
        IndexingMode::CopyOnWriteInt32 => INT32_SHAPE | COPY_ON_WRITE,
        IndexingMode::CopyOnWriteDouble => DOUBLE_SHAPE | COPY_ON_WRITE,
        IndexingMode::CopyOnWriteContiguous => CONTIGUOUS_SHAPE | COPY_ON_WRITE,
        // Rust-only modes with no IndexingType byte (see block comment above).
        IndexingMode::Dictionary | IndexingMode::IntegerIndexedExotic => return None,
    };
    Some(byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Bit layout matches IndexingType.h exactly ----

    #[test]
    fn flag_and_shape_bit_values() {
        assert_eq!(IS_ARRAY, 0x01);
        assert_eq!(NO_INDEXING_SHAPE, 0x00);
        assert_eq!(UNDECIDED_SHAPE, 0x02);
        assert_eq!(INT32_SHAPE, 0x04);
        assert_eq!(DOUBLE_SHAPE, 0x06);
        assert_eq!(CONTIGUOUS_SHAPE, 0x08);
        assert_eq!(ARRAY_STORAGE_SHAPE, 0x0A);
        assert_eq!(SLOW_PUT_ARRAY_STORAGE_SHAPE, 0x0C);
        assert_eq!(INDEXING_SHAPE_MASK, 0x0E);
        assert_eq!(INDEXING_SHAPE_SHIFT, 1);
        assert_eq!(COPY_ON_WRITE, 0x10);
        assert_eq!(MAY_HAVE_INDEXED_ACCESSORS, 0x20);
        assert_eq!(INDEXING_TYPE_LOCK_IS_HELD, 0x40);
        assert_eq!(INDEXING_TYPE_LOCK_HAS_PARKED, 0x80);
    }

    #[test]
    fn derived_masks() {
        assert_eq!(INDEXING_TYPE_MASK, 0x0F); // shape | IsArray
        assert_eq!(INDEXING_SHAPE_AND_WRITABILITY_MASK, 0x1E); // CoW | shape
        assert_eq!(INDEXING_MODE_MASK, 0x1F); // CoW | IsArray | shape
        assert_eq!(ALL_WRITABLE_ARRAY_TYPES, 0x0F);
        assert_eq!(ALL_ARRAY_TYPES, 0x1F);
        assert_eq!(ALL_WRITABLE_ARRAY_TYPES_AND_HISTORY, 0x2F);
        assert_eq!(ALL_ARRAY_TYPES_AND_HISTORY, 0x3F);
        assert_eq!(NUMBER_OF_INDEXING_SHAPES, 7);
        assert_eq!(NUMBER_OF_COPY_ON_WRITE_INDEXING_MODES, 3);
        assert_eq!(NUMBER_OF_ARRAY_INDEXING_MODES, 10);
    }

    #[test]
    fn named_array_types() {
        // IndexingType.h:107-116.
        assert_eq!(ARRAY_CLASS, 0x01);
        assert_eq!(ARRAY_WITH_UNDECIDED, 0x03);
        assert_eq!(ARRAY_WITH_INT32, 0x05);
        assert_eq!(ARRAY_WITH_DOUBLE, 0x07);
        assert_eq!(ARRAY_WITH_CONTIGUOUS, 0x09);
        assert_eq!(ARRAY_WITH_ARRAY_STORAGE, 0x0B);
        assert_eq!(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE, 0x0D);
        assert_eq!(COPY_ON_WRITE_ARRAY_WITH_INT32, 0x15);
        assert_eq!(COPY_ON_WRITE_ARRAY_WITH_DOUBLE, 0x17);
        assert_eq!(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS, 0x19);
    }

    // ---- Predicates select on the masked shape ----

    #[test]
    fn has_indexed_properties_predicate() {
        assert!(!has_indexed_properties(NON_ARRAY));
        assert!(!has_indexed_properties(ARRAY_CLASS)); // IsArray but NoIndexingShape
        assert!(has_indexed_properties(ARRAY_WITH_INT32));
        assert!(has_indexed_properties(NON_ARRAY_WITH_DOUBLE));
        assert!(has_indexed_properties(ARRAY_WITH_ARRAY_STORAGE));
        // History/lock bits must not register as a shape.
        assert!(!has_indexed_properties(MAY_HAVE_INDEXED_ACCESSORS));
        assert!(!has_indexed_properties(INDEXING_TYPE_LOCK_IS_HELD));
    }

    #[test]
    fn shape_predicates_are_exclusive_after_masking() {
        // Each genuine shape matches exactly one shape predicate.
        for &ty in &[
            ARRAY_WITH_UNDECIDED,
            ARRAY_WITH_INT32,
            ARRAY_WITH_DOUBLE,
            ARRAY_WITH_CONTIGUOUS,
            ARRAY_WITH_ARRAY_STORAGE,
            ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE,
        ] {
            let hits = [
                has_undecided(ty),
                has_int32(ty),
                has_double(ty),
                has_contiguous(ty),
                has_array_storage(ty),
                has_slow_put_array_storage(ty),
            ]
            .iter()
            .filter(|b| **b)
            .count();
            assert_eq!(hits, 1, "exactly one shape predicate for {ty:#04x}");
        }
    }

    #[test]
    fn any_array_storage_and_slow_put() {
        assert!(has_any_array_storage(ARRAY_WITH_ARRAY_STORAGE));
        assert!(has_any_array_storage(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE));
        assert!(!has_any_array_storage(ARRAY_WITH_CONTIGUOUS));
        // shouldUseSlowPut <=> slow-put array storage.
        assert!(should_use_slow_put(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE));
        assert!(!should_use_slow_put(ARRAY_WITH_ARRAY_STORAGE));
        // hasArrayStorage is the plain (non-slow-put) variant only.
        assert!(has_array_storage(ARRAY_WITH_ARRAY_STORAGE));
        assert!(!has_array_storage(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE));
    }

    #[test]
    fn copy_on_write_predicate() {
        assert!(!is_copy_on_write(ARRAY_WITH_INT32));
        assert!(is_copy_on_write(COPY_ON_WRITE_ARRAY_WITH_INT32));
        assert!(is_copy_on_write(COPY_ON_WRITE_ARRAY_WITH_DOUBLE));
        assert!(is_copy_on_write(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS));
        // Copy-on-write does not disturb the shape bits.
        assert!(has_int32(COPY_ON_WRITE_ARRAY_WITH_INT32));
        assert!(has_contiguous(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS));
    }

    #[test]
    fn array_index_densification() {
        // Writable shapes densify to 0..=6 in shape order (IndexingType.h:212).
        assert_eq!(array_index_from_indexing_type(NON_ARRAY), 0);
        assert_eq!(array_index_from_indexing_type(ARRAY_WITH_UNDECIDED), 1);
        assert_eq!(array_index_from_indexing_type(ARRAY_WITH_INT32), 2);
        assert_eq!(array_index_from_indexing_type(ARRAY_WITH_DOUBLE), 3);
        assert_eq!(array_index_from_indexing_type(ARRAY_WITH_CONTIGUOUS), 4);
        assert_eq!(array_index_from_indexing_type(ARRAY_WITH_ARRAY_STORAGE), 5);
        assert_eq!(
            array_index_from_indexing_type(ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE),
            6
        );
        // The three copy-on-write modes densify to 7..=9 (IndexingType.h:210-211).
        assert_eq!(
            array_index_from_indexing_type(COPY_ON_WRITE_ARRAY_WITH_INT32),
            7
        );
        assert_eq!(
            array_index_from_indexing_type(COPY_ON_WRITE_ARRAY_WITH_DOUBLE),
            8
        );
        assert_eq!(
            array_index_from_indexing_type(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS),
            9
        );
        // The dense range is exactly NumberOfArrayIndexingModes wide and unique.
        let mut indices: Vec<u32> = [
            NON_ARRAY,
            ARRAY_WITH_UNDECIDED,
            ARRAY_WITH_INT32,
            ARRAY_WITH_DOUBLE,
            ARRAY_WITH_CONTIGUOUS,
            ARRAY_WITH_ARRAY_STORAGE,
            ARRAY_WITH_SLOW_PUT_ARRAY_STORAGE,
            COPY_ON_WRITE_ARRAY_WITH_INT32,
            COPY_ON_WRITE_ARRAY_WITH_DOUBLE,
            COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS,
        ]
        .iter()
        .map(|&ty| array_index_from_indexing_type(ty))
        .collect();
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(indices.len(), NUMBER_OF_ARRAY_INDEXING_MODES as usize);
        assert_eq!(
            *indices.last().unwrap(),
            NUMBER_OF_ARRAY_INDEXING_MODES as u32 - 1
        );
    }

    // ---- IndexingMode enum -> byte bridge ----

    #[test]
    fn indexing_mode_shape_mapping() {
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::None),
            Some(NO_INDEXING_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::UndecidedArray),
            Some(UNDECIDED_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::Int32),
            Some(INT32_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::Double),
            Some(DOUBLE_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::Contiguous),
            Some(CONTIGUOUS_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::ArrayStorage),
            Some(ARRAY_STORAGE_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::SlowPutArrayStorage),
            Some(SLOW_PUT_ARRAY_STORAGE_SHAPE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::CopyOnWriteInt32),
            Some(INT32_SHAPE | COPY_ON_WRITE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::CopyOnWriteDouble),
            Some(DOUBLE_SHAPE | COPY_ON_WRITE)
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::CopyOnWriteContiguous),
            Some(CONTIGUOUS_SHAPE | COPY_ON_WRITE)
        );
    }

    #[test]
    fn rust_only_modes_have_no_indexing_byte() {
        // Dictionary and IntegerIndexedExotic are not IndexingType shapes in JSC.
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::Dictionary),
            None
        );
        assert_eq!(
            indexing_shape_and_writability_for_mode(IndexingMode::IntegerIndexedExotic),
            None
        );
    }

    #[test]
    fn mapping_agrees_with_enum_predicates() {
        // The byte's predicates agree with the enum's own predicates for every
        // mode that has a faithful IndexingType byte.
        for mode in [
            IndexingMode::None,
            IndexingMode::UndecidedArray,
            IndexingMode::Int32,
            IndexingMode::Double,
            IndexingMode::Contiguous,
            IndexingMode::ArrayStorage,
            IndexingMode::SlowPutArrayStorage,
            IndexingMode::CopyOnWriteInt32,
            IndexingMode::CopyOnWriteDouble,
            IndexingMode::CopyOnWriteContiguous,
        ] {
            let byte = indexing_shape_and_writability_for_mode(mode).unwrap();
            assert_eq!(
                has_indexed_properties(byte),
                mode.has_indexed_properties(),
                "has_indexed_properties mismatch for {mode:?}"
            );
            assert_eq!(
                is_copy_on_write(byte),
                mode.is_copy_on_write(),
                "is_copy_on_write mismatch for {mode:?}"
            );
        }
    }
}
