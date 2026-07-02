//! `StructureTransitionTable`: the per-Structure table of outgoing shape
//! transitions (runtime/StructureTransitionTable.h:44-315).
//!
//! A Structure points at the other Structures reachable from it by adding a
//! property, changing a prototype, morphing indexing type, sealing, etc. The
//! table is keyed by `(PointerKey, attributes, TransitionKind)` and is the
//! mechanism that lets two objects that grow the same property in the same order
//! end up sharing one Structure (StructureInlines.h:549-566,
//! Structure.cpp:561-620). JSC stores the common case — a single outgoing
//! transition — inline in one tagged word and only promotes to a hash map on the
//! second transition.
//!
//! C++ -> Rust mapping:
//!   - `enum class TransitionKind`           -> [`TransitionKind`]
//!   - free predicate helpers (h:72-152)     -> module functions
//!   - `StructureTransitionTable::PointerKey` -> [`PointerKey`]
//!   - `Hash::Key` (ADDRESS64 encoding)       -> private [`Key`]
//!   - `WeakGCMap<Key, Structure, ...>`       -> private [`TransitionMap`]
//!   - `class StructureTransitionTable`       -> [`StructureTransitionTable`]
//!
//! Divergences (each also noted at its site):
//!   - C++ packs `m_data` as a tagged `intptr_t` (low bit => inline `Structure*`,
//!     else a `TransitionMap*`). Rust cannot bit-cast/tag a pointer without
//!     `unsafe`, so we model the identical state machine with a safe enum whose
//!     discriminant is the tag bit ([`TransitionData`]).
//!   - The table stores a *weak* `Structure*`; this no-deps unit has no GC
//!     weak-ref wiring nor a live `Structure` cell, so it stores a by-value
//!     [`TransitionStructure`] descriptor (structure identity + the transition's
//!     own key fields) strongly. The write barrier and weak-map finalization
//!     become faithful once the table holds live cells.
//!   - `newIndexingType` (h:89-119) is deliberately omitted: it needs the
//!     `IndexingType` bitfield (UndecidedShape/Int32Shape/... and
//!     `hasIndexedProperties`), which belongs to the IndexingType unit, not here.

#![allow(dead_code)]

use crate::gc::{FxIntBuildHasher, StructureId};
use std::collections::HashMap;

/// Property attributes carried on a transition.
///
/// JSC: `using TransitionPropertyAttributes = uint8_t;`
/// (StructureTransitionTable.h:41 — "In fact, it should be 7 bits.").
pub type TransitionPropertyAttributes = u8;

/// Faithful port of `enum class TransitionKind : uint8_t`
/// (StructureTransitionTable.h:44-68). "This must be 5 bits (less than 32)."
///
/// `#[repr(u8)]` with the exact JSC discriminants so the enum encodes/decodes
/// through [`Key`] identically to the C++ packing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TransitionKind {
    Unknown = 0,
    PropertyAddition = 1,
    PropertyDeletion = 2,
    PropertyAttributeChange = 3,

    // Transitions not related to properties; for these the string portion of the
    // key is 0 (StructureTransitionTable.h:50-51).
    AllocateUndecided = 4,
    AllocateInt32 = 5,
    AllocateDouble = 6,
    AllocateContiguous = 7,
    AllocateArrayStorage = 8,
    AllocateSlowPutArrayStorage = 9,
    SwitchToSlowPutArrayStorage = 10,
    AddIndexedAccessors = 11,
    PreventExtensions = 12,
    Seal = 13,
    Freeze = 14,
    BecomePrototype = 15,
    ChangePrototype = 16,

    // Private-brand transition (StructureTransitionTable.h:66-67).
    SetBrand = 17,
}

impl TransitionKind {
    /// Reverse of `static_cast<TransitionKind>(uint8_t)` used by [`Key`] decode.
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::PropertyAddition,
            2 => Self::PropertyDeletion,
            3 => Self::PropertyAttributeChange,
            4 => Self::AllocateUndecided,
            5 => Self::AllocateInt32,
            6 => Self::AllocateDouble,
            7 => Self::AllocateContiguous,
            8 => Self::AllocateArrayStorage,
            9 => Self::AllocateSlowPutArrayStorage,
            10 => Self::SwitchToSlowPutArrayStorage,
            11 => Self::AddIndexedAccessors,
            12 => Self::PreventExtensions,
            13 => Self::Seal,
            14 => Self::Freeze,
            15 => Self::BecomePrototype,
            16 => Self::ChangePrototype,
            17 => Self::SetBrand,
            // Keys are only built from valid kinds; mirrors the C++ contract that
            // the encoded byte is always a real discriminant.
            other => unreachable!("invalid TransitionKind discriminant: {other}"),
        }
    }
}

/// JSC: `static constexpr auto FirstNonPropertyTransitionKind = ...;`
/// (StructureTransitionTable.h:70).
pub const FIRST_NON_PROPERTY_TRANSITION_KIND: TransitionKind = TransitionKind::AllocateUndecided;

/// JSC `changesIndexingType` (StructureTransitionTable.h:72-87).
pub const fn changes_indexing_type(transition: TransitionKind) -> bool {
    matches!(
        transition,
        TransitionKind::AllocateUndecided
            | TransitionKind::AllocateInt32
            | TransitionKind::AllocateDouble
            | TransitionKind::AllocateContiguous
            | TransitionKind::AllocateArrayStorage
            | TransitionKind::AllocateSlowPutArrayStorage
            | TransitionKind::SwitchToSlowPutArrayStorage
            | TransitionKind::AddIndexedAccessors
    )
}

/// JSC `preventsExtensions` (StructureTransitionTable.h:121-131).
pub const fn prevents_extensions(transition: TransitionKind) -> bool {
    matches!(
        transition,
        TransitionKind::PreventExtensions | TransitionKind::Seal | TransitionKind::Freeze
    )
}

/// JSC `setsDontDeleteOnAllProperties` (StructureTransitionTable.h:133-142).
pub const fn sets_dont_delete_on_all_properties(transition: TransitionKind) -> bool {
    matches!(transition, TransitionKind::Seal | TransitionKind::Freeze)
}

/// JSC `setsReadOnlyOnNonAccessorProperties` (StructureTransitionTable.h:144-152).
pub const fn sets_read_only_on_non_accessor_properties(transition: TransitionKind) -> bool {
    matches!(transition, TransitionKind::Freeze)
}

/// Faithful port of `StructureTransitionTable::PointerKey`
/// (StructureTransitionTable.h:157-178): an 8-byte-aligned raw pointer reused as
/// the pointer component of a transition key. In C++ it is a
/// `UniquedStringImpl*` (a property-name uid), a `JSObject*` (a prototype, for
/// `ChangePrototype`), or null (non-property transitions). This no-deps unit has
/// neither cell type wired, so we carry the raw `uintptr_t` exactly as C++ does
/// (`m_pointer`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointerKey {
    pointer: usize,
}

impl PointerKey {
    /// `PointerKey(UniquedStringImpl*)` — a property-name uid.
    pub const fn from_uid(uid: usize) -> Self {
        Self { pointer: uid }
    }

    /// `PointerKey(JSObject*)` — a prototype object (`ChangePrototype`).
    pub const fn from_object(object: usize) -> Self {
        Self { pointer: object }
    }

    /// `constexpr PointerKey(std::nullptr_t)` — the rep for non-property
    /// transitions.
    pub const fn null() -> Self {
        Self { pointer: 0 }
    }

    /// `uintptr_t raw() const`.
    pub const fn raw(self) -> usize {
        self.pointer
    }

    /// `static PointerKey fromRaw(uintptr_t)`.
    pub const fn from_raw(raw: usize) -> Self {
        Self { pointer: raw }
    }
}

/// Faithful port of the ADDRESS64 `StructureTransitionTable::Hash::Key`
/// (StructureTransitionTable.h:181-243): the `(PointerKey, attributes,
/// TransitionKind)` tuple packed into one word — low 48 bits the pointer, bits
/// 48-55 the attributes, bits 56-63 the `TransitionKind`. Equality is word
/// equality, so the map keys and hashes on this single integer, matching the
/// in-tree [`FxIntBuildHasher`] integer fast path (C++ hashes the same word with
/// WTF `IntHash`).
///
/// This is the `#if CPU(ADDRESS64)` encoding; our targets are 64-bit (arm64 /
/// x86-64), so the 48-bit-effective-address assumption holds.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Key {
    encoded_data: usize,
}

impl Key {
    const ATTRIBUTES_SHIFT: u32 = 48;
    const TRANSITION_KIND_SHIFT: u32 = 56;
    const STRING_MASK: usize = (1usize << Self::ATTRIBUTES_SHIFT) - 1;
    // C++ also reserves `hashTableDeletedValue = 0x2` for WTF HashTable's deleted
    // sentinel; `std::HashMap` manages tombstones internally, so it is not
    // modeled here.

    fn new(impl_: PointerKey, attributes: u32, transition_kind: TransitionKind) -> Self {
        // Mirrors the C++ Key ctor ASSERTs (h:199-204). The 8-byte-alignment
        // ASSERT is dropped: the pointer occupies the full low 48 bits including
        // its low 3 bits here, so alignment is not required for the key to
        // round-trip (it is a C++ pointer invariant, not a Key invariant).
        debug_assert!(
            (impl_.raw() & !Self::STRING_MASK) == 0,
            "PointerKey must fit in 48 bits"
        );
        debug_assert!(attributes <= u32::from(u8::MAX));
        debug_assert!(transition_kind != TransitionKind::Unknown);
        Self {
            encoded_data: impl_.raw()
                | ((attributes as usize) << Self::ATTRIBUTES_SHIFT)
                | ((transition_kind as usize) << Self::TRANSITION_KIND_SHIFT),
        }
    }

    /// `PointerKey impl() const`.
    fn impl_(self) -> PointerKey {
        PointerKey::from_raw(self.encoded_data & Self::STRING_MASK)
    }

    /// `TransitionPropertyAttributes attributes() const`.
    fn attributes(self) -> TransitionPropertyAttributes {
        ((self.encoded_data >> Self::ATTRIBUTES_SHIFT) & usize::from(u8::MAX)) as u8
    }

    /// `TransitionKind transitionKind() const`.
    fn transition_kind(self) -> TransitionKind {
        TransitionKind::from_u8((self.encoded_data >> Self::TRANSITION_KIND_SHIFT) as u8)
    }

    /// `static Key createKey(PointerKey, unsigned, TransitionKind)`
    /// (StructureTransitionTable.h:237-240).
    fn create_key(impl_: PointerKey, attributes: u32, transition_kind: TransitionKind) -> Self {
        Self::new(impl_, attributes, transition_kind)
    }
}

/// The subset of a JSC `Structure`'s own fields the table reads back when it
/// stores a transition: the resulting structure's identity plus the transition
/// that produced it.
///
/// In C++ the table stores a `Structure*` and re-derives the key from the
/// structure's `transitionPropertyName()`/`storedPrototype()`,
/// `transitionPropertyAttributes()`, and `transitionKind()`
/// (Structure.h:838,926,1066,1084; StructureInlines.h:580-588). The real cell is
/// not wired into this no-deps unit, so the caller hands the table this
/// descriptor; `structure` is the `Structure*` identity ([`StructureId`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionStructure {
    /// Identity of the structure this transition leads to (C++ `Structure*`).
    pub structure: StructureId,
    /// The `PointerKey` rep this transition was keyed by: the property-name uid
    /// (`m_transitionPropertyName`) for property kinds, the prototype object for
    /// `ChangePrototype`, or null for non-property kinds. C++ splits this across
    /// `m_transitionPropertyName` and `storedPrototype`, switching on
    /// `transitionKind` in `createKeyFromStructure`; we store the already-resolved
    /// rep, so that switch collapses to reading this one field.
    pub transition_property: PointerKey,
    /// `transitionPropertyAttributes()`.
    pub attributes: TransitionPropertyAttributes,
    /// `transitionKind()`.
    pub kind: TransitionKind,
}

impl TransitionStructure {
    pub const fn new(
        structure: StructureId,
        transition_property: PointerKey,
        attributes: TransitionPropertyAttributes,
        kind: TransitionKind,
    ) -> Self {
        Self {
            structure,
            transition_property,
            attributes,
            kind,
        }
    }
}

/// Faithful port of `TransitionMap`
/// (`WeakGCMap<Hash::Key, Structure, Hash, Hash::KeyTraits>`,
/// StructureTransitionTable.h:269): the promoted, multi-entry form.
///
/// C++ holds *weak* `Structure` values that its finalizer sweeps; until GC
/// weak-ref wiring exists this no-deps unit holds the transition descriptor
/// strongly. Keyed on the encoded integer [`Key`] through the in-tree
/// [`FxIntBuildHasher`] (gc/fast_hash.rs), the WTF `IntHash` analog for
/// VM-internal integer-keyed maps.
#[derive(Clone, Debug, Default)]
struct TransitionMap {
    map: HashMap<Key, TransitionStructure, FxIntBuildHasher>,
}

impl TransitionMap {
    fn new() -> Self {
        Self::default()
    }

    /// `WeakGCMap::get` — the live transition for `key`, or `None`.
    fn get(&self, key: Key) -> Option<&TransitionStructure> {
        self.map.get(&key)
    }

    /// `WeakGCMap::set` — insert or overwrite.
    fn set(&mut self, key: Key, value: TransitionStructure) {
        self.map.insert(key, value);
    }
}

/// The two states of C++ `m_data` (StructureTransitionTable.h:154-315).
///
/// C++ packs both into one tagged `intptr_t`: low bit set => a `Structure*`
/// lives in the upper bits (single-slot, the common case); low bit clear => the
/// word is a `TransitionMap*`. Rust forbids that pointer-tag bit-cast without
/// `unsafe`, so we model the identical state machine with a safe enum — the
/// discriminant is the tag bit. `SingleSlot(None)` is the initial
/// `m_data == UsingSingleSlotFlag` (empty); `SingleSlot(Some(_))` is one inline
/// transition; `Map(_)` is the promoted form. The observable single->map
/// promotion is unchanged.
#[derive(Clone, Debug)]
enum TransitionData {
    SingleSlot(Option<TransitionStructure>),
    Map(Box<TransitionMap>),
}

/// Faithful port of `class StructureTransitionTable`
/// (StructureTransitionTable.h:154-315).
#[derive(Clone, Debug)]
pub struct StructureTransitionTable {
    data: TransitionData,
}

impl Default for StructureTransitionTable {
    /// `intptr_t m_data { UsingSingleSlotFlag }` — start empty in single-slot mode
    /// (StructureTransitionTable.h:314).
    fn default() -> Self {
        Self {
            data: TransitionData::SingleSlot(None),
        }
    }
}

impl StructureTransitionTable {
    /// `StructureTransitionTable() = default;` (StructureTransitionTable.h:272).
    pub fn new() -> Self {
        Self::default()
    }

    // --- private state accessors mirroring the C++ private helpers ---

    /// `bool isUsingSingleSlot() const` — `m_data & UsingSingleSlotFlag`
    /// (StructureTransitionTable.h:293-296).
    fn is_using_single_slot(&self) -> bool {
        matches!(self.data, TransitionData::SingleSlot(_))
    }

    /// `Structure* trySingleTransition() const` (StructureInlines.h:590-596):
    /// the inline transition when single-slot and occupied, else `None` (an empty
    /// single slot or the promoted map both yield `nullptr` in C++).
    ///
    /// Public, matching the C++ `public:` declaration (h:286). Returns the full
    /// [`TransitionStructure`] (our stand-in for the `Structure*`), from which a
    /// caller can read the same fields C++ reads off the returned cell.
    pub fn try_single_transition(&self) -> Option<TransitionStructure> {
        match &self.data {
            TransitionData::SingleSlot(slot) => *slot,
            TransitionData::Map(_) => None,
        }
    }

    /// `TransitionMap* map() const` — valid only when not single-slot
    /// (StructureTransitionTable.h:298-302).
    fn map(&self) -> &TransitionMap {
        match &self.data {
            TransitionData::Map(m) => m,
            // Mirrors C++ `ASSERT(!isUsingSingleSlot())`; every caller gates on
            // `!is_using_single_slot()` first.
            TransitionData::SingleSlot(_) => unreachable!("map() requires !isUsingSingleSlot()"),
        }
    }

    fn map_mut(&mut self) -> &mut TransitionMap {
        match &mut self.data {
            TransitionData::Map(m) => m,
            TransitionData::SingleSlot(_) => unreachable!("map() requires !isUsingSingleSlot()"),
        }
    }

    /// `void setMap(TransitionMap*)` — promote single-slot to map; clears the
    /// flag (StructureTransitionTable.h:304-310).
    fn set_map(&mut self, map: Box<TransitionMap>) {
        debug_assert!(self.is_using_single_slot()); // C++ ASSERT(isUsingSingleSlot())
        self.data = TransitionData::Map(map);
        debug_assert!(!self.is_using_single_slot()); // C++ ASSERT(!isUsingSingleSlot())
    }

    /// `void setSingleTransition(VM&, JSCell* owner, Structure*)`
    /// (Structure.cpp:85-90).
    fn set_single_transition(&mut self, structure: TransitionStructure) {
        debug_assert!(self.is_using_single_slot()); // C++ ASSERT(isUsingSingleSlot())
        self.data = TransitionData::SingleSlot(Some(structure));
        // C++ also issues `vm.writeBarrier(owner, structure)`. Omitted: no GC
        // write barriers are wired in this no-deps unit (faithful once the table
        // holds live `Structure` cells).
    }

    /// `Hash::Key createKeyFromStructure(Structure*)` (StructureInlines.h:580-588).
    ///
    /// C++ switches on `transitionKind` to read `storedPrototype`
    /// (`ChangePrototype`) or `m_transitionPropertyName` (otherwise); both yield a
    /// `PointerKey`. We store the resolved rep in [`TransitionStructure`], so the
    /// switch collapses to reading `transition_property`.
    fn create_key_from_structure(structure: &TransitionStructure) -> Key {
        Key::create_key(
            structure.transition_property,
            u32::from(structure.attributes),
            structure.kind,
        )
    }

    // --- public interface (StructureTransitionTable.h:282-288) ---

    /// `void add(VM&, JSCell* owner, Structure*)` (Structure.cpp:101-120).
    ///
    /// First transition stays inline (single slot); the second promotes to a map
    /// and re-inserts the existing transition before adding the new one. Once a
    /// map, it stays a map.
    pub fn add(&mut self, structure: TransitionStructure) {
        if self.is_using_single_slot() {
            match self.try_single_transition() {
                // This handles the first transition being added.
                None => {
                    self.set_single_transition(structure);
                    return;
                }
                // This handles the second transition being added (or the first
                // being despecified): allocate the map, then re-add the existing
                // single transition (which now takes the map branch).
                Some(existing) => {
                    self.set_map(Box::new(TransitionMap::new()));
                    self.add(existing);
                }
            }
        }

        // Add the structure to the map.
        let key = Self::create_key_from_structure(&structure);
        self.map_mut().set(key, structure);
    }

    /// `bool contains(PointerKey, unsigned attributes, TransitionKind) const`
    /// (Structure.cpp:92-99).
    pub fn contains(
        &self,
        rep: PointerKey,
        attributes: u32,
        transition_kind: TransitionKind,
    ) -> bool {
        if self.is_using_single_slot() {
            return match self.try_single_transition() {
                // C++ `contains` compares `m_transitionPropertyName` directly,
                // whereas `get` routes `ChangePrototype` through `storedPrototype`
                // via `createKeyFromStructure` — a latent C++ asymmetry for
                // `ChangePrototype` single slots. Our single resolved rep keeps the
                // two consistent.
                Some(transition) => {
                    transition.transition_property == rep
                        && u32::from(transition.attributes) == attributes
                        && transition.kind == transition_kind
                }
                None => false,
            };
        }
        self.map()
            .get(Key::create_key(rep, attributes, transition_kind))
            .is_some()
    }

    /// `Structure* get(PointerKey, unsigned attributes, TransitionKind) const`
    /// (StructureInlines.h:598-609). Returns the target structure's identity.
    pub fn get(
        &self,
        rep: PointerKey,
        attributes: u32,
        transition_kind: TransitionKind,
    ) -> Option<StructureId> {
        if self.is_using_single_slot() {
            let transition = self.try_single_transition()?;
            if Self::create_key_from_structure(&transition)
                != Key::create_key(rep, attributes, transition_kind)
            {
                return None;
            }
            return Some(transition.structure);
        }
        self.map()
            .get(Key::create_key(rep, attributes, transition_kind))
            .map(|transition| transition.structure)
    }

    /// `void finalizeUnconditionally(VM&, CollectionScope)`
    /// (StructureInlines.h:611-617): clear the inline single-slot transition when
    /// its target structure is unmarked (`m_data = UsingSingleSlotFlag`).
    ///
    /// MAP TIER (structures-as-cells Step 4, design §5): once promoted, the map
    /// is a `WeakGCMap<Key, Structure>` in C++ (StructureTransitionTable.h:269)
    /// whose dead entries are pruned by its OWN separately-registered end-of-cycle
    /// hook — `WeakGCMap::pruneStaleEntries()` removes every entry whose weak
    /// `Structure` died (WeakGCMapInlines.h:71-76), driven from
    /// `Heap::pruneStaleEntriesFromWeakGCHashTables()` at the same
    /// `Heap::runEndPhase` seam (heap/Heap.cpp:1751, one line before this
    /// finalize's own driver at :1754). The port has no generic weak-map type
    /// (the ratified GC-U7 precedent: minimal prune-on-finalize, no
    /// `WeakGCMap` hierarchy), so the map arm is pruned HERE, in the one
    /// finalize call, by the identical liveness predicate — observation-
    /// equivalent to C++ (both prunes run between mark end and sweep, against
    /// the same mark bits). `is_marked` is the caller-supplied analog of
    /// `vm.heap.isMarked(transition)`.
    pub fn finalize_unconditionally(&mut self, is_marked: impl Fn(StructureId) -> bool) {
        match &mut self.data {
            TransitionData::SingleSlot(slot) => {
                if let Some(transition) = slot {
                    if !is_marked(transition.structure) {
                        // m_data = UsingSingleSlotFlag
                        *slot = None;
                    }
                }
            }
            TransitionData::Map(map) => {
                // WeakGCMap::pruneStaleEntries (WeakGCMapInlines.h:71-76):
                // `m_map.removeIf([](entry) { return !entry.value; })` — a dead
                // weak value IS an unmarked Structure at this seam position.
                map.map
                    .retain(|_, transition| is_marked(transition.structure));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(
        id: u32,
        rep: PointerKey,
        attributes: TransitionPropertyAttributes,
        kind: TransitionKind,
    ) -> TransitionStructure {
        TransitionStructure::new(StructureId::new(id), rep, attributes, kind)
    }

    #[test]
    fn key_round_trips_pointer_attributes_and_kind() {
        let key = Key::create_key(
            PointerKey::from_uid(0x12340),
            7,
            TransitionKind::PropertyAttributeChange,
        );
        assert_eq!(key.impl_().raw(), 0x12340);
        assert_eq!(key.attributes(), 7);
        assert_eq!(
            key.transition_kind(),
            TransitionKind::PropertyAttributeChange
        );

        // Distinct components produce distinct keys.
        let other_attrs = Key::create_key(
            PointerKey::from_uid(0x12340),
            8,
            TransitionKind::PropertyAttributeChange,
        );
        let other_kind = Key::create_key(
            PointerKey::from_uid(0x12340),
            7,
            TransitionKind::PropertyAddition,
        );
        assert_ne!(key, other_attrs);
        assert_ne!(key, other_kind);
    }

    #[test]
    fn default_table_is_an_empty_single_slot() {
        let table = StructureTransitionTable::new();
        assert!(table.try_single_transition().is_none());
        assert_eq!(
            table.get(
                PointerKey::from_uid(0x1000),
                0,
                TransitionKind::PropertyAddition
            ),
            None
        );
        assert!(!table.contains(
            PointerKey::from_uid(0x1000),
            0,
            TransitionKind::PropertyAddition
        ));
    }

    #[test]
    fn first_add_uses_the_inline_single_slot() {
        let mut table = StructureTransitionTable::new();
        let uid = PointerKey::from_uid(0x1000);
        table.add(ts(1, uid, 0, TransitionKind::PropertyAddition));

        // Still inline.
        assert!(table.try_single_transition().is_some());
        assert_eq!(
            table.get(uid, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(1))
        );
        assert!(table.contains(uid, 0, TransitionKind::PropertyAddition));

        // Different key in the same single slot does not match.
        assert_eq!(
            table.get(
                PointerKey::from_uid(0x2000),
                0,
                TransitionKind::PropertyAddition
            ),
            None
        );
        assert_eq!(table.get(uid, 1, TransitionKind::PropertyAddition), None);
        assert_eq!(table.get(uid, 0, TransitionKind::PropertyDeletion), None);
    }

    #[test]
    fn second_add_promotes_single_slot_to_map() {
        let mut table = StructureTransitionTable::new();
        let uid_a = PointerKey::from_uid(0x1000);
        let uid_b = PointerKey::from_uid(0x2000);

        table.add(ts(1, uid_a, 0, TransitionKind::PropertyAddition));
        assert!(table.try_single_transition().is_some()); // single slot

        table.add(ts(2, uid_b, 0, TransitionKind::PropertyAddition));
        // Promoted: single-slot probe now reports nothing.
        assert!(table.try_single_transition().is_none());

        // Both transitions remain reachable through the map.
        assert_eq!(
            table.get(uid_a, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(1))
        );
        assert_eq!(
            table.get(uid_b, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(2))
        );
        assert!(table.contains(uid_a, 0, TransitionKind::PropertyAddition));
        assert!(table.contains(uid_b, 0, TransitionKind::PropertyAddition));
        assert!(!table.contains(
            PointerKey::from_uid(0x3000),
            0,
            TransitionKind::PropertyAddition
        ));
    }

    #[test]
    fn re_adding_the_same_key_overwrites_after_promotion() {
        // C++ `map()->set` overwrites; the second add always promotes, then the
        // identical key replaces the first value.
        let mut table = StructureTransitionTable::new();
        let uid = PointerKey::from_uid(0x1000);
        table.add(ts(1, uid, 0, TransitionKind::PropertyAddition));
        table.add(ts(2, uid, 0, TransitionKind::PropertyAddition));

        assert!(table.try_single_transition().is_none()); // promoted
        assert_eq!(
            table.get(uid, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(2))
        );
    }

    #[test]
    fn non_property_transition_uses_null_rep() {
        // Non-property transitions carry a null string portion (h:50-51).
        let mut table = StructureTransitionTable::new();
        let null = PointerKey::null();
        table.add(ts(9, null, 0, TransitionKind::PreventExtensions));
        assert_eq!(
            table.get(null, 0, TransitionKind::PreventExtensions),
            Some(StructureId::new(9))
        );
        assert!(table.contains(null, 0, TransitionKind::PreventExtensions));
        // A different non-property kind does not collide.
        assert_eq!(table.get(null, 0, TransitionKind::Seal), None);
    }

    #[test]
    fn finalize_clears_dead_single_slot_and_keeps_live() {
        let uid = PointerKey::from_uid(0x1000);

        let mut dead = StructureTransitionTable::new();
        dead.add(ts(5, uid, 0, TransitionKind::PropertyAddition));
        dead.finalize_unconditionally(|id| id != StructureId::new(5));
        assert!(dead.try_single_transition().is_none());
        assert_eq!(dead.get(uid, 0, TransitionKind::PropertyAddition), None);

        let mut live = StructureTransitionTable::new();
        live.add(ts(5, uid, 0, TransitionKind::PropertyAddition));
        live.finalize_unconditionally(|_| true);
        assert!(live.try_single_transition().is_some());
        assert_eq!(
            live.get(uid, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(5))
        );
    }

    // Structures-as-cells Step 4 (design §5): the promoted map tier is pruned
    // the WeakGCMap way — dead-target entries removed, live ones kept
    // (WeakGCMap::pruneStaleEntries, WeakGCMapInlines.h:71-76, driven at the
    // same end-of-cycle seam, Heap.cpp:1751).
    #[test]
    fn finalize_prunes_dead_map_entries_and_keeps_live_ones() {
        let uid_a = PointerKey::from_uid(0x1000);
        let uid_b = PointerKey::from_uid(0x2000);
        let uid_c = PointerKey::from_uid(0x3000);

        let mut table = StructureTransitionTable::new();
        table.add(ts(1, uid_a, 0, TransitionKind::PropertyAddition));
        table.add(ts(2, uid_b, 0, TransitionKind::PropertyAddition)); // promotes to map
        table.add(ts(3, uid_c, 0, TransitionKind::PropertyAddition));
        assert!(table.try_single_transition().is_none()); // promoted

        // Structures 1 and 3 survived the mark; 2 is dead.
        table.finalize_unconditionally(|id| id != StructureId::new(2));

        assert_eq!(
            table.get(uid_a, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(1)),
            "a live entry must be kept"
        );
        assert_eq!(
            table.get(uid_b, 0, TransitionKind::PropertyAddition),
            None,
            "a dead-target entry must be pruned (pruneStaleEntries)"
        );
        assert_eq!(
            table.get(uid_c, 0, TransitionKind::PropertyAddition),
            Some(StructureId::new(3))
        );
        assert!(!table.contains(uid_b, 0, TransitionKind::PropertyAddition));
    }

    #[test]
    fn transition_kind_predicates_match_jsc() {
        assert!(changes_indexing_type(TransitionKind::AllocateInt32));
        assert!(changes_indexing_type(TransitionKind::AddIndexedAccessors));
        assert!(!changes_indexing_type(TransitionKind::PropertyAddition));
        assert!(!changes_indexing_type(TransitionKind::Seal));

        assert!(prevents_extensions(TransitionKind::PreventExtensions));
        assert!(prevents_extensions(TransitionKind::Seal));
        assert!(prevents_extensions(TransitionKind::Freeze));
        assert!(!prevents_extensions(TransitionKind::PropertyAddition));

        assert!(sets_dont_delete_on_all_properties(TransitionKind::Seal));
        assert!(sets_dont_delete_on_all_properties(TransitionKind::Freeze));
        assert!(!sets_dont_delete_on_all_properties(
            TransitionKind::PreventExtensions
        ));

        assert!(sets_read_only_on_non_accessor_properties(
            TransitionKind::Freeze
        ));
        assert!(!sets_read_only_on_non_accessor_properties(
            TransitionKind::Seal
        ));

        assert_eq!(
            FIRST_NON_PROPERTY_TRANSITION_KIND,
            TransitionKind::AllocateUndecided
        );
    }
}
