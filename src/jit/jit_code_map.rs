//! The baseline JIT's bytecode-index -> machine-code landing map.
//!
//! Faithful port of `jit/JITCodeMap.h`: `JITCodeMap` (JITCodeMap.h:46-85, the
//! sorted-key binary-search map `BaselineJITCode::m_jitCodeMap` persists) and
//! `JITCodeMapBuilder` (JITCodeMap.h:87-106). `JIT::link()` builds it from the
//! per-bytecode MAIN-pass labels (`m_labels[bci] = label()`, JIT.cpp:200) —
//! `jitCodeMapBuilder.append(BytecodeIndex(bci), patchBuffer.locationOf(m_labels
//! [bci]))` for every set label (JIT.cpp:954-958) — and stores the finalized map
//! on the `BaselineJITCode` (JIT.cpp:1017). It is the OSR landing table: LLInt
//! loop OSR jumps to `find(loopHintIndex)` and a DFG OSR exit lands mid-function
//! at `find(exitBytecodeIndex)`.
//!
//! DIVERGENCE (entry-relative offsets, commented here once for the type): C++
//! stores ABSOLUTE `CodeLocationLabel<JSEntryPtrTag>` values because the map is
//! built at link time, after `LinkBuffer` placed the code at its final address
//! (`patchBuffer.locationOf`). The Rust baseline pipeline emits a POSITION-
//! INDEPENDENT `FunctionImage` first and installs it into executable memory in a
//! separate step (`finalize_arm64_link_buffer`), so the map is built before the
//! absolute base exists; it therefore stores `u32` byte offsets RELATIVE TO THE
//! FUNCTION ENTRY and the installed image reconstitutes the absolute label as
//! `entry_address + offset` (one add — NOT the delta-compressed-offset decode
//! JSC rejected in 2018, see mcts_mem baseline-jit/unlinked-code-sharing.alt/
//! compact-jit-code-map.md). C++ also packs both arrays into one malloc block
//! (JITCodeMap.h:56-58, a locality optimization); Rust keeps two parallel
//! `Vec`s, which preserves the parallel-array layout without the manual
//! allocation.

#![allow(dead_code)]

use crate::bytecode::BytecodeIndex;

/// `JITCodeMap` (JITCodeMap.h:46-85): sorted `BytecodeIndex` keys with the
/// machine-code location of each bytecode boundary's MAIN-pass label. Keys are
/// strictly increasing (the builder loop in JIT.cpp:955 appends in ascending
/// bytecode order), which is what makes `find`'s binary search sound.
#[derive(Clone, Debug, Default)]
pub(crate) struct JitCodeMap {
    /// == the `m_pointer` indexes array (JITCodeMap.h:76-79).
    indexes: Vec<BytecodeIndex>,
    /// == the `m_pointer` codeLocations array (JITCodeMap.h:71-74), as
    /// entry-relative byte offsets (see the module divergence note).
    code_offsets: Vec<u32>,
}

impl JitCodeMap {
    /// `JITCodeMap::find(BytecodeIndex)` (JITCodeMap.h:59-65): binary-search the
    /// sorted keys; the match's code location, or the empty sentinel on a miss.
    /// C++ uses WTF `binarySearch` (key-must-be-present: asserts in debug and
    /// returns the nearest element in release) and then null-checks; the safe
    /// Rust port uses an exact search so an absent key is a clean `None` rather
    /// than an approximate landing point.
    pub(crate) fn find(&self, bytecode_index: BytecodeIndex) -> Option<u32> {
        self.indexes
            .binary_search(&bytecode_index)
            .ok()
            .map(|position| self.code_offsets[position])
    }

    /// `explicit operator bool()` (JITCodeMap.h:67): whether the map has entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.indexes.is_empty()
    }

    /// Number of (bytecode index, code offset) entries (== `m_size`).
    pub(crate) fn len(&self) -> usize {
        self.indexes.len()
    }

    /// The sorted `(BytecodeIndex, entry-relative code offset)` entries, in key
    /// order. Rust-only iteration surface for the map's structural unit tests
    /// (C++ exposes only `find`; nothing at runtime consumes this).
    pub(crate) fn entries(&self) -> impl Iterator<Item = (BytecodeIndex, u32)> + '_ {
        self.indexes
            .iter()
            .copied()
            .zip(self.code_offsets.iter().copied())
    }
}

/// `JITCodeMapBuilder` (JITCodeMap.h:87-106): accumulates `(BytecodeIndex,
/// location)` pairs in the caller's append order; `finalize` freezes them into
/// the map. As in `JIT::link()`, the caller appends in ascending bytecode-index
/// order.
#[derive(Default)]
pub(crate) struct JitCodeMapBuilder {
    indexes: Vec<BytecodeIndex>,
    code_offsets: Vec<u32>,
}

impl JitCodeMapBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// `append(bytecodeIndex, codeLocation)` (JITCodeMap.h:91-95).
    pub(crate) fn append(&mut self, bytecode_index: BytecodeIndex, code_offset: u32) {
        self.indexes.push(bytecode_index);
        self.code_offsets.push(code_offset);
    }

    /// `finalize()` (JITCodeMap.h:97-100). The strictly-increasing key order the
    /// binary search requires is the builder contract JSC relies on implicitly
    /// (the ascending JIT.cpp:955 loop); assert it here so a misordered Rust
    /// caller fails loudly in debug instead of mis-landing an OSR jump.
    pub(crate) fn finalize(self) -> JitCodeMap {
        debug_assert!(
            self.indexes.windows(2).all(|pair| pair[0] < pair[1]),
            "JITCodeMap keys must be appended in strictly increasing bytecode-index order"
        );
        JitCodeMap {
            indexes: self.indexes,
            code_offsets: self.code_offsets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    #[test]
    fn find_returns_exact_matches_and_none_for_absent_keys() {
        let mut builder = JitCodeMapBuilder::new();
        builder.append(bci(0), 0x10);
        builder.append(bci(3), 0x40);
        builder.append(bci(4), 0x5c);
        builder.append(bci(8), 0x90);
        let map = builder.finalize();

        assert!(!map.is_empty());
        assert_eq!(map.len(), 4);
        assert_eq!(map.find(bci(0)), Some(0x10));
        assert_eq!(map.find(bci(3)), Some(0x40));
        assert_eq!(map.find(bci(8)), Some(0x90));
        // Absent keys (before, between, past the sorted keys) are clean misses.
        assert_eq!(map.find(bci(1)), None);
        assert_eq!(map.find(bci(5)), None);
        assert_eq!(map.find(bci(9)), None);
    }

    #[test]
    fn empty_map_is_empty_and_finds_nothing() {
        let map = JitCodeMapBuilder::new().finalize();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.find(bci(0)), None);
    }
}
