//! Faithful port of C++ JSC `runtime/PropertyOffset.h:32-146`.
//!
//! C++ JSC models a property offset as `typedef int PropertyOffset` plus a set of
//! free `constexpr inline` functions that split the offset space into an inline
//! band `[0, firstOutOfLineOffset)` and an out-of-line band `[firstOutOfLineOffset,
//! ...)`. The inline band indexes the object's inline slots directly; the
//! out-of-line band maps to NEGATIVE indices into the Butterfly's property storage
//! (`offsetInOutOfLineStorage`). This module mirrors that header exactly: the same
//! constants, the same predicates, and the same arithmetic.
//!
//! STANDALONE / NOT WIRED. The interpreter currently carries its own
//! `offset_storage_index` (`src/interpreter/mod.rs:6405`) that DIVERGES from this
//! header: it indexes a forward-growing `Vec` with `INLINE_CAPACITY == 0`, so it
//! never exercises the inline/out-of-line split or the negative Butterfly indices.
//! Replacing that with this faithful math is NOT a trivial substitution -- it
//! requires introducing real inline storage and a negative-indexed Butterfly, which
//! is a serial object/structure-model decision owned by the orchestrator. Until
//! that lands, this module is the faithful reference, tested against the C++
//! formulas but not yet a dependency of the interpreter. The items are therefore
//! unreachable from outside `object`; `allow(dead_code)` documents the awaiting-wire
//! state (mirroring the same marker already used in interpreter/mod.rs).
#![allow(dead_code)]

// C++ JSC `PropertyOffset.h:32`: `typedef int PropertyOffset;`
// Faithful mirror: a transparent alias over `i32` (C++ `int`), not a newtype, so the
// free functions below read like the header. (The accidental `object::property::
// PropertyOffset` newtype predates this port; unifying onto this alias is an
// orchestrator integration step, deliberately out of scope for this standalone unit.)
pub type PropertyOffset = i32;

// C++ JSC `PropertyOffset.h:34`: `static constexpr PropertyOffset invalidOffset = -1;`
pub const INVALID_OFFSET: PropertyOffset = -1;

// C++ JSC `PropertyOffset.h:35`: `static constexpr PropertyOffset firstOutOfLineOffset = 64;`
// Offsets `< firstOutOfLineOffset` live inline; `>=` live out-of-line in the Butterfly.
// 64 == the inline-storage capacity boundary shared with the JITs.
pub const FIRST_OUT_OF_LINE_OFFSET: PropertyOffset = 64;

// C++ JSC `PropertyOffset.h:36`: `static constexpr PropertyOffset knownPolyProtoOffset = 0;`
pub const KNOWN_POLY_PROTO_OFFSET: PropertyOffset = 0;

// C++ JSC `PropertyOffset.h:37`: `static_assert(knownPolyProtoOffset < firstOutOfLineOffset, ...)`
// "We assume in all the JITs that the poly proto offset is an inline offset."
const _: () = assert!(
    KNOWN_POLY_PROTO_OFFSET < FIRST_OUT_OF_LINE_OFFSET,
    "We assume in all the JITs that the poly proto offset is an inline offset"
);

// NOTE on `debug_assert!` vs C++ `ASSERT`: C++'s checks are `ASSERT` (debug-only) and
// the functions are `constexpr`. We mirror `ASSERT` with `debug_assert!`. We do NOT
// mark these `const fn`: that is a deliberate language divergence -- `const fn` would
// constrain the `debug_assert!` panics and is unnecessary for the offset math here.
// The formulas and assert semantics are preserved exactly.

/// C++ JSC `PropertyOffset.h:53`: `checkOffset(PropertyOffset)`.
fn check_offset(offset: PropertyOffset) {
    debug_assert!(offset >= INVALID_OFFSET);
}

/// C++ JSC `PropertyOffset.h:59`: `checkOffset(PropertyOffset, int inlineCapacity)`.
/// Rust cannot overload, so the capacity-taking overload is suffixed `_with_capacity`.
fn check_offset_with_capacity(offset: PropertyOffset, inline_capacity: i32) {
    debug_assert!(offset >= INVALID_OFFSET);
    debug_assert!(
        offset == INVALID_OFFSET || offset < inline_capacity || is_out_of_line_offset(offset)
    );
}

/// C++ JSC `PropertyOffset.h:69`: `validateOffset(PropertyOffset)`.
fn validate_offset(offset: PropertyOffset) {
    check_offset(offset);
    debug_assert!(is_valid_offset(offset));
}

/// C++ JSC `PropertyOffset.h:75`: `validateOffset(PropertyOffset, int inlineCapacity)`.
fn validate_offset_with_capacity(offset: PropertyOffset, inline_capacity: i32) {
    check_offset_with_capacity(offset, inline_capacity);
    debug_assert!(is_valid_offset(offset));
}

/// C++ JSC `PropertyOffset.h:81`: `isValidOffset(PropertyOffset)`.
pub fn is_valid_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    offset != INVALID_OFFSET
}

/// C++ JSC `PropertyOffset.h:87`: `isInlineOffset(PropertyOffset)`.
pub fn is_inline_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    offset < FIRST_OUT_OF_LINE_OFFSET
}

/// C++ JSC `PropertyOffset.h:93`: `isOutOfLineOffset(PropertyOffset)`.
pub fn is_out_of_line_offset(offset: PropertyOffset) -> bool {
    check_offset(offset);
    !is_inline_offset(offset)
}

/// C++ JSC `PropertyOffset.h:99`: `offsetInInlineStorage(PropertyOffset)`.
/// Returns `intptr_t`; mirrored as `isize` (the faithful analog of `intptr_t`).
pub fn offset_in_inline_storage(offset: PropertyOffset) -> isize {
    validate_offset(offset);
    debug_assert!(is_inline_offset(offset));
    offset as isize
}

/// C++ JSC `PropertyOffset.h:106`: `offsetInOutOfLineStorage(PropertyOffset)`.
/// `-static_cast<ptrdiff_t>(offset - firstOutOfLineOffset) - 1`: out-of-line offsets
/// map to NEGATIVE Butterfly indices (the property store grows downward from the
/// Butterfly base). `ptrdiff_t` is mirrored as `isize`.
pub fn offset_in_out_of_line_storage(offset: PropertyOffset) -> isize {
    validate_offset(offset);
    debug_assert!(is_out_of_line_offset(offset));
    -((offset - FIRST_OUT_OF_LINE_OFFSET) as isize) - 1
}

/// C++ JSC `PropertyOffset.h:113`: `offsetInRespectiveStorage(PropertyOffset)`.
pub fn offset_in_respective_storage(offset: PropertyOffset) -> isize {
    if is_inline_offset(offset) {
        offset_in_inline_storage(offset)
    } else {
        offset_in_out_of_line_storage(offset)
    }
}

/// C++ JSC `PropertyOffset.h:120`: `numberOfOutOfLineSlotsForMaxOffset(PropertyOffset)`.
/// Returns `size_t`; mirrored as `usize`.
pub fn number_of_out_of_line_slots_for_max_offset(offset: PropertyOffset) -> usize {
    check_offset(offset);
    if offset < FIRST_OUT_OF_LINE_OFFSET {
        return 0;
    }
    (offset - FIRST_OUT_OF_LINE_OFFSET + 1) as usize
}

/// C++ JSC `PropertyOffset.h:128`: `numberOfSlotsForMaxOffset(PropertyOffset, int inlineCapacity)`.
/// Returns `size_t`; mirrored as `usize`.
pub fn number_of_slots_for_max_offset(offset: PropertyOffset, inline_capacity: i32) -> usize {
    check_offset_with_capacity(offset, inline_capacity);
    if offset < inline_capacity {
        return (offset + 1) as usize;
    }
    inline_capacity as usize + number_of_out_of_line_slots_for_max_offset(offset)
}

/// C++ JSC `PropertyOffset.h:136`: `offsetForPropertyNumber(int propertyNumber, int inlineCapacity)`.
/// The first `inlineCapacity` property numbers map 1:1 to inline offsets; the next
/// property number JUMPS to `firstOutOfLineOffset` to begin the out-of-line band.
pub fn offset_for_property_number(property_number: i32, inline_capacity: i32) -> PropertyOffset {
    let mut offset: PropertyOffset = property_number;
    if offset >= inline_capacity {
        offset += FIRST_OUT_OF_LINE_OFFSET;
        offset -= inline_capacity;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    // C++ JSC PropertyOffset.h:34-36 constant values.
    #[test]
    fn constants_match_header() {
        assert_eq!(INVALID_OFFSET, -1);
        assert_eq!(FIRST_OUT_OF_LINE_OFFSET, 64);
        assert_eq!(KNOWN_POLY_PROTO_OFFSET, 0);
        // The static_assert at PropertyOffset.h:37.
        assert!(KNOWN_POLY_PROTO_OFFSET < FIRST_OUT_OF_LINE_OFFSET);
    }

    // C++ JSC PropertyOffset.h:81 isValidOffset.
    #[test]
    fn is_valid_offset_only_rejects_invalid() {
        assert!(!is_valid_offset(INVALID_OFFSET));
        assert!(is_valid_offset(0));
        assert!(is_valid_offset(63));
        assert!(is_valid_offset(64));
        assert!(is_valid_offset(1000));
    }

    // C++ JSC PropertyOffset.h:87,93 isInlineOffset / isOutOfLineOffset boundary.
    #[test]
    fn inline_vs_out_of_line_split_at_64() {
        assert!(is_inline_offset(0));
        assert!(is_inline_offset(63));
        assert!(!is_inline_offset(64));
        assert!(!is_inline_offset(65));

        assert!(!is_out_of_line_offset(0));
        assert!(!is_out_of_line_offset(63));
        assert!(is_out_of_line_offset(64));
        assert!(is_out_of_line_offset(65));

        // The two predicates partition the valid offset space.
        for offset in 0..200 {
            assert_ne!(is_inline_offset(offset), is_out_of_line_offset(offset));
        }
    }

    // C++ JSC PropertyOffset.h:99 offsetInInlineStorage: identity within the inline band.
    #[test]
    fn offset_in_inline_storage_is_identity() {
        assert_eq!(offset_in_inline_storage(0), 0);
        assert_eq!(offset_in_inline_storage(5), 5);
        assert_eq!(offset_in_inline_storage(63), 63);
    }

    // C++ JSC PropertyOffset.h:106 offsetInOutOfLineStorage:
    // -(offset - firstOutOfLineOffset) - 1.
    #[test]
    fn offset_in_out_of_line_storage_is_negative_butterfly_index() {
        assert_eq!(offset_in_out_of_line_storage(64), -1);
        assert_eq!(offset_in_out_of_line_storage(65), -2);
        assert_eq!(offset_in_out_of_line_storage(66), -3);
        // General formula check across the band.
        for offset in 64..200 {
            let expected = -((offset - FIRST_OUT_OF_LINE_OFFSET) as isize) - 1;
            assert_eq!(offset_in_out_of_line_storage(offset), expected);
        }
    }

    // C++ JSC PropertyOffset.h:113 offsetInRespectiveStorage dispatches on the band.
    #[test]
    fn offset_in_respective_storage_dispatches() {
        assert_eq!(offset_in_respective_storage(5), 5);
        assert_eq!(offset_in_respective_storage(63), 63);
        assert_eq!(offset_in_respective_storage(64), -1);
        assert_eq!(offset_in_respective_storage(65), -2);
    }

    // C++ JSC PropertyOffset.h:120 numberOfOutOfLineSlotsForMaxOffset.
    #[test]
    fn number_of_out_of_line_slots_for_max_offset_matches() {
        assert_eq!(number_of_out_of_line_slots_for_max_offset(0), 0);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(63), 0);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(64), 1);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(65), 2);
        assert_eq!(number_of_out_of_line_slots_for_max_offset(127), 64);
    }

    // C++ JSC PropertyOffset.h:128 numberOfSlotsForMaxOffset.
    #[test]
    fn number_of_slots_for_max_offset_matches() {
        // offset < inlineCapacity: offset + 1.
        assert_eq!(number_of_slots_for_max_offset(0, 6), 1);
        assert_eq!(number_of_slots_for_max_offset(5, 6), 6);
        // offset >= inlineCapacity: inlineCapacity + numberOfOutOfLineSlotsForMaxOffset.
        assert_eq!(number_of_slots_for_max_offset(64, 6), 6 + 1);
        assert_eq!(number_of_slots_for_max_offset(65, 6), 6 + 2);
        // With inlineCapacity 0 (matches the interpreter's first-cut constant): every
        // valid offset is out-of-line, so it equals numberOfOutOfLineSlotsForMaxOffset
        // shifted by firstOutOfLineOffset.
        assert_eq!(number_of_slots_for_max_offset(64, 0), 0 + 1);
    }

    // C++ JSC PropertyOffset.h:136 offsetForPropertyNumber, including the jump-to-64.
    #[test]
    fn offset_for_property_number_jumps_to_first_out_of_line() {
        let cap = 6;
        // The first `inlineCapacity` property numbers are identity inline offsets.
        assert_eq!(offset_for_property_number(0, cap), 0);
        assert_eq!(offset_for_property_number(5, cap), 5);
        // The next property number jumps to firstOutOfLineOffset (64).
        assert_eq!(offset_for_property_number(6, cap), 64);
        assert_eq!(offset_for_property_number(7, cap), 65);

        // inlineCapacity == 0: property number 0 jumps straight to 64.
        assert_eq!(offset_for_property_number(0, 0), 64);
        assert_eq!(offset_for_property_number(1, 0), 65);
    }

    // Round-trip: offsetForPropertyNumber then classify; the first `cap` are inline,
    // the rest out-of-line, and out-of-line slot counts are contiguous from 0.
    #[test]
    fn property_number_round_trip_classification() {
        let cap = 8;
        for property_number in 0..40 {
            let offset = offset_for_property_number(property_number, cap);
            if property_number < cap {
                assert!(is_inline_offset(offset));
                assert_eq!(offset_in_inline_storage(offset), offset as isize);
            } else {
                assert!(is_out_of_line_offset(offset));
                // out_index is 0,1,2,... in allocation order; offsetInOutOfLineStorage
                // is the negative Butterfly index for that slot.
                let out_index = property_number - cap;
                assert_eq!(
                    offset_in_out_of_line_storage(offset),
                    -(out_index as isize) - 1
                );
            }
        }
    }
}
