//! `DFG::FrozenValue` and its strength lattice.
//!
//! Faithful port of `dfg/DFGFrozenValue.h:39-126` (the `FrozenValue` class)
//! and `dfg/DFGValueStrength.h:34-56` (the `ValueStrength` enum + `merge`).
//! C++ JSC admits a heap constant into a DFG graph ONLY by freezing it
//! through `Graph::freeze`/`Graph::freezeStrong` (dfg/DFGGraph.cpp:1633-1664)
//! into the graph-owned `m_frozenValues` arena; this is GC-audit hard blocker
//! #1 / divergence #3 (raw un-frozen cell refs must never appear in a DFG
//! graph). The arena itself (`m_frozenValueMap` dedup + `m_frozenValues`
//! storage + `freeze`/`freezeStrong`) lives directly on `DfgGraph`
//! (dfg/graph.rs), mirroring `Graph` owning both fields directly in C++
//! rather than introducing a separate Rust-only arena wrapper type.
//!
//! KEEP-ALIVE scope note: this module supplies the value type and the
//! strength lattice; the graph-owned arena's `gather_frozen_roots` (in
//! `dfg/graph.rs`) is the `Graph::visitChildren` analog (DFGGraph.cpp:
//! 1621-1628) that must root every cell-valued frozen entry for the lifetime
//! of a live `DfgPlan`/`DfgGraph`. See that method's doc for the current
//! wiring status (no live-plan registry exists yet, so the production
//! collection safepoint fold cannot reach it — noted there, not papered
//! over).

use crate::gc::StructureId;
use crate::value::JsValue;

/// `enum ValueStrength` (dfg/DFGValueStrength.h:34-43).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueStrength {
    /// "The value has been used for optimization and it arose through
    /// inference. We don't want the fact that we optimized the code to
    /// result in the GC keeping this value alive unnecessarily, so we'd
    /// rather kill the code and recompile than keep the object alive
    /// longer." (DFGValueStrength.h:35-38.) This is `Graph::freeze`'s
    /// default strength.
    WeakValue,
    /// "The code will keep this value alive. This is true of constants that
    /// were present in the source. String constants tend to be strong."
    /// (DFGValueStrength.h:40-42.) `Graph::freezeStrong` upgrades to this.
    StrongValue,
}

/// `inline ValueStrength merge(ValueStrength a, ValueStrength b)`
/// (dfg/DFGValueStrength.h:45-56): `StrongValue` dominates.
pub fn merge_strength(a: ValueStrength, b: ValueStrength) -> ValueStrength {
    match a {
        ValueStrength::WeakValue => b,
        ValueStrength::StrongValue => ValueStrength::StrongValue,
    }
}

/// Stable identity for an entry in `DfgGraph::frozen_values`. C++ hands out a
/// stable `FrozenValue*` from the `SegmentedVector<FrozenValue, 16>` arena
/// (dfg/DFGGraph.h:1376, `Graph::freeze`'s `&m_frozenValues.alloc(...)`,
/// DFGGraph.cpp:1656); safe Rust hands out this index instead, the same
/// index-is-identity idiom `DfgNodeId`/`DfgBasicBlockId`/`DfgEdgeId` already
/// use (dfg/graph.rs).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FrozenValueId(pub u32);

/// Faithful port of `class FrozenValue` (dfg/DFGFrozenValue.h:39-126).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrozenValue {
    /// `JSValue m_value` (DFGFrozenValue.h:123).
    value: JsValue,
    /// `Structure* m_structure` (DFGFrozenValue.h:124).
    ///
    /// C++ resolves this by reading the live cell's structure pointer
    /// directly at freeze time (`value.asCell()->structure()`,
    /// DFGFrozenValue.h:119) — every `JSCell`, including `JSString`, carries
    /// a non-null `Structure*`. `DfgGraph` has no heap access (cells live in
    /// the host object store, not reachable from this pure data structure),
    /// so `DfgGraph::freeze`/`freeze_strong` take the resolved `StructureId`
    /// as an explicit parameter from a caller that DOES have store access
    /// (language/architecture divergence: split store/graph ownership, not a
    /// behavior change — see `DfgGraph::freeze`).
    ///
    /// A SECOND, narrower divergence widens this to `None` even for some
    /// cell values: this crate's leaf cells (String/Symbol/BigInt) are not
    /// admitted into the object arena's `Structure`-bearing cell table at all
    /// yet (`CoreObjectStore::cell_at`'s doc, interpreter/object_store.rs:
    /// "a leaf-cell (string/symbol/bigint) ... lies in no arena block", and
    /// `CoreObjectStore::structure_id` returns `None` for any value `find`
    /// cannot resolve). JSC gives every `JSCell` — including strings — a
    /// `Structure*` (`JSString`'s `StructureID` still names a `Structure`
    /// describing `StringType`); this crate does not model that for leaf
    /// cells yet, a pre-existing narrower-scope divergence this unit does not
    /// widen or fix. `FrozenValue` therefore accepts `structure: None` for
    /// ANY cell (not only non-cells), unlike DFGFrozenValue.h:60's
    /// `ASSERT((!!value && value.isCell()) == !!structure)`; the converse
    /// direction of that assert (a structure is never attached to a
    /// non-cell) IS still enforced below.
    structure: Option<StructureId>,
    /// `ValueStrength m_strength` (DFGFrozenValue.h:125).
    strength: ValueStrength,
}

impl Default for FrozenValue {
    /// `FrozenValue()` (DFGFrozenValue.h:41-45): empty value, null structure,
    /// `WeakValue` strength. `FrozenValue::emptySingleton()`
    /// (DFGFrozenValue.cpp:35-39) is exactly one instance of this default.
    fn default() -> Self {
        Self {
            value: JsValue::default(),
            structure: None,
            strength: ValueStrength::WeakValue,
        }
    }
}

impl FrozenValue {
    /// `FrozenValue(JSValue value)` (DFGFrozenValue.h:47-53): non-cell values
    /// only (`RELEASE_ASSERT(!value || !value.isCell())`, :52).
    pub fn immediate(value: JsValue) -> Self {
        debug_assert!(
            value == JsValue::default() || !value.is_cell(),
            "FrozenValue::immediate is for non-cell values only \
             (RELEASE_ASSERT(!value || !value.isCell()), DFGFrozenValue.h:52)"
        );
        Self {
            value,
            structure: None,
            strength: ValueStrength::WeakValue,
        }
    }

    /// `FrozenValue(JSValue value, Structure* structure, ValueStrength strength)`
    /// (DFGFrozenValue.h:55-63). See the struct's `structure` doc for the two
    /// documented narrowings of DFGFrozenValue.h:60's assert this crate
    /// accepts (Rust's split store/graph ownership, and leaf cells not yet
    /// carrying a `StructureId`).
    pub fn with_structure(
        value: JsValue,
        structure: Option<StructureId>,
        strength: ValueStrength,
    ) -> Self {
        let is_cell = value != JsValue::default() && value.is_cell();
        debug_assert!(
            structure.is_none() || is_cell,
            "a structure must never be attached to a non-cell value \
             (DFGFrozenValue.h:60, converse direction)"
        );
        debug_assert!(
            structure.is_some() || strength == ValueStrength::WeakValue,
            "ASSERT(!!structure || (strength == WeakValue)), DFGFrozenValue.h:62"
        );
        Self {
            value,
            structure,
            strength,
        }
    }

    /// `value()` (DFGFrozenValue.h:69).
    pub fn value(&self) -> JsValue {
        self.value
    }

    /// `structure()` (DFGFrozenValue.h:86).
    pub fn structure(&self) -> Option<StructureId> {
        self.structure
    }

    /// `strength()` (DFGFrozenValue.h:97).
    pub fn strength(&self) -> ValueStrength {
        self.strength
    }

    /// `pointsToHeap()` (DFGFrozenValue.h:94): `!!value() && value().isCell()`.
    pub fn points_to_heap(&self) -> bool {
        self.value != JsValue::default() && self.value.is_cell()
    }

    /// `strengthenTo` (DFGFrozenValue.h:88-92): only a cell value's strength
    /// is ever merged; a non-cell stays `WeakValue` regardless of how many
    /// times `freezeStrong` dedups onto it (matching C++: `if (!!m_value &&
    /// m_value.isCell()) m_strength = merge(m_strength, strength);`).
    pub fn strengthen_to(&mut self, strength: ValueStrength) {
        if self.points_to_heap() {
            self.strength = merge_strength(self.strength, strength);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_empty_singleton_shape() {
        let empty = FrozenValue::default();
        assert_eq!(empty.value(), JsValue::default());
        assert_eq!(empty.structure(), None);
        assert_eq!(empty.strength(), ValueStrength::WeakValue);
        assert!(!empty.points_to_heap());
    }

    #[test]
    fn immediate_records_weak_strength_and_no_structure() {
        let five = FrozenValue::immediate(JsValue::from_i32(5));
        assert_eq!(five.value(), JsValue::from_i32(5));
        assert_eq!(five.structure(), None);
        assert_eq!(five.strength(), ValueStrength::WeakValue);
        assert!(!five.points_to_heap());
    }

    #[test]
    #[should_panic(expected = "non-cell values only")]
    fn immediate_rejects_a_cell_value_in_debug_builds() {
        // A synthetic cell-tagged encoding is enough to exercise the guard;
        // no real cell needs to back it for this invariant check.
        let cell_like = JsValue::from_encoded(crate::value::EncodedJsValue(0x1_0000_0020));
        assert!(
            cell_like.is_cell(),
            "fixture must actually decode as a cell"
        );
        FrozenValue::immediate(cell_like);
    }

    #[test]
    fn strengthen_to_is_a_no_op_for_non_cell_values() {
        // DFGFrozenValue.h:88-92: strengthenTo only merges for cell values.
        let mut five = FrozenValue::immediate(JsValue::from_i32(5));
        five.strengthen_to(ValueStrength::StrongValue);
        assert_eq!(
            five.strength(),
            ValueStrength::WeakValue,
            "a non-cell FrozenValue must stay WeakValue no matter how many times it is \
             strengthened (matches JSC's isCell() guard in strengthenTo)"
        );
    }

    #[test]
    fn merge_strength_matches_the_dominance_table() {
        use ValueStrength::*;
        assert_eq!(merge_strength(WeakValue, WeakValue), WeakValue);
        assert_eq!(merge_strength(WeakValue, StrongValue), StrongValue);
        assert_eq!(merge_strength(StrongValue, WeakValue), StrongValue);
        assert_eq!(merge_strength(StrongValue, StrongValue), StrongValue);
    }
}
