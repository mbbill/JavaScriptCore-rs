//! Faithful port of the `Structure` JSCell and its `StructureID` surface
//! (`runtime/Structure.h:197-1101`, `runtime/StructureID.h:37-124`,
//! `runtime/StructureInlines.h`, `runtime/Structure.cpp`).
//!
//! A `Structure` is JSC's shape descriptor: the immutable layout a set of
//! objects share. Objects carry a 32-bit `StructureID` in their cell header
//! (`JSCell::m_structureID` at offset 0); the JIT inline-cache guard loads that
//! word (`load32 [cell + 0]`) and compares it against the cached `StructureID`
//! to validate a shape before reading a property at a fixed offset. Structures
//! form a transition tree: adding a property to a structure produces a child
//! structure, and two objects that grow the same properties in the same order
//! converge on the SAME child (so the IC stays monomorphic). Each structure
//! lazily materializes a `PropertyTable` for named lookup, and the table
//! ownership is *moved* down the transition edge (with a clone only when pinned),
//! so a chain of structures shares one table and any structure can rebuild its
//! own by replaying the transitions from the nearest ancestor that still owns
//! the table.
//!
//! ## C++ -> Rust structural mapping
//!
//! - `class StructureID` (StructureID.h:37) -> [`StructureId`] (the encoded
//!   header word + nuke bit + JIT guard surface).
//! - `class Structure : public JSCell` (Structure.h:197) -> [`Structure`].
//! - `class TypeInfoBlob` (TypeInfoBlob.h:34) -> [`TypeInfoBlob`].
//! - The per-VM `Structure*` <-> `StructureID` mapping (in C++ implicit in the
//!   Structure heap address) -> [`StructureIdTable`], the registry handle table.
//!
//! The four committed leaf ports are integrated directly:
//! - [`PropertyOffset`](super::property_offset) for the offset math
//!   (`firstOutOfLineOffset == 64`, inline/out-of-line split).
//! - [`PropertyTable`](super::property_table) as the materialized lookup table.
//! - [`IndexingType`](super::indexing_type) for the shape byte of the blob.
//! - [`StructureTransitionTable`](super::structure_transition_table) for the
//!   outgoing transition edges (single-slot -> map promotion, sibling sharing).
//!
//! ## Serial-decision-driven ownership skeleton (NOT C++ pointer aliasing)
//!
//! The committed serial decisions for this rewrite are: *Heap OWNS the
//! StructureIDTable; `StructureID` is a registry handle (a `Vec` index), not a
//! masked heap address; `StructureID` nuke reserves the low bit.* C++ derives a
//! `StructureID` from the structure's heap address (`StructureID::encode`,
//! StructureID.h:90-97) because Structures live in a dedicated aligned heap;
//! its low bit is free *because of that alignment* and is reused as the nuke
//! flag. The Structure cells are not arena-resident in this port yet, so:
//!   - [`StructureIdTable`] owns the `Structure` cells in a `Vec` and hands out
//!     1-based [`gc::StructureId`] handles (slot 0 is the null/invalid handle,
//!     matching `gc::StructureId::INVALID` and C++ `StructureID(0)`).
//!   - [`StructureId::encode`] turns a handle into the header word by shifting
//!     the index up one bit, so bit 0 is *explicitly* reserved for the nuke flag
//!     (the faithful analog of C++'s alignment-reserved low bit). [`StructureId::
//!     decode`] reverses it.
//!   - Structures reference each other (`previousID`, transition targets) by
//!     handle, never by owning pointer, which is also why the
//!     `StructureTransitionTable` leaf already stores a [`gc::StructureId`].
//! Because the cells live in the table, the transition/materialization
//! operations that C++ writes as `static Structure*` methods taking a
//! `Structure*` are ported as [`StructureIdTable`] methods taking a handle.
//!
//! WIRED (gc-r4 Batch 2): the interpreter's `CoreObjectStore` now mounts a
//! [`StructureIdTable`] as the SINGLE structure-id + property-offset authority
//! (replacing the per-cell `property_offsets`/`next_property_offset` divergence);
//! named-property offsets flow from `add_property_transition`/`materialize_property
//! _table` here. The Rust-only shape DSL (`object/structure.rs`) is a separate,
//! still-standalone descriptor surface. `#![allow(dead_code)]` is retained because
//! several faithful accessors (TypeInfoBlob getters, indexing helpers) have no
//! interpreter consumer YET; they stay as the faithful reference.
#![allow(dead_code)]

use super::indexing_type::{
    IndexingType, ALL_ARRAY_TYPES, ALL_WRITABLE_ARRAY_TYPES, COPY_ON_WRITE,
};
use super::property_offset::{number_of_slots_for_max_offset, PropertyOffset, INVALID_OFFSET};
use super::property_table::{PropertyTable, PropertyTableEntry};
use super::structure_transition_table::{
    PointerKey, StructureTransitionTable, TransitionKind, TransitionPropertyAttributes,
    TransitionStructure,
};
use crate::gc::StructureId as StructureHandle;
use crate::strings::AtomId;

// =============================== StructureID ===============================

/// Faithful port of `class StructureID` (StructureID.h:37-68): the 32-bit word
/// stored in a JSCell header and compared by the JIT shape guard.
///
/// In C++ the value is a masked heap address; here it is a registry handle
/// shifted up one bit (see the module note on the serial decision). The nuke
/// bit, validity (`operator bool`), and the encode/decode round-trip are modeled
/// exactly; the `tryDecode` heap-bounds check (StructureID.h:80-88) is the only
/// arm omitted, because there is no contiguous structure heap to bounds-check
/// against in the registry model.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct StructureId {
    /// `uint32_t m_bits { 0 }` (StructureID.h:67).
    bits: u32,
}

impl StructureId {
    /// `static constexpr uint32_t nukedStructureIDBit = 1;` (StructureID.h:39).
    pub const NUKED_STRUCTURE_ID_BIT: u32 = 1;

    /// The byte offset of `JSCell::m_structureID` within a cell. The JIT shape
    /// guard emits `load32 [cell + 0]` to read this word (the StructureID is the
    /// first field of every JSCell), so the guard offset is 0.
    pub const CELL_STRUCTURE_ID_OFFSET: usize = 0;

    /// `explicit constexpr StructureID(uint32_t bits)` (StructureID.h:65). The
    /// private raw ctor; public construction goes through [`Self::encode`].
    const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// `StructureID nuke() const` (StructureID.h:49): set the reserved low bit.
    pub const fn nuke(self) -> Self {
        Self::from_bits(self.bits | Self::NUKED_STRUCTURE_ID_BIT)
    }

    /// `bool isNuked() const` (StructureID.h:50).
    pub const fn is_nuked(self) -> bool {
        self.bits & Self::NUKED_STRUCTURE_ID_BIT != 0
    }

    /// `StructureID decontaminate() const` (StructureID.h:51): clear the nuke bit.
    pub const fn decontaminate(self) -> Self {
        Self::from_bits(self.bits & !Self::NUKED_STRUCTURE_ID_BIT)
    }

    /// `explicit operator bool() const { return !!m_bits; }` (StructureID.h:57).
    pub const fn is_valid(self) -> bool {
        self.bits != 0
    }

    /// `constexpr uint32_t bits() const` (StructureID.h:59). This is the word the
    /// JIT shape guard loads from `[cell + 0]` and compares; exposing it names
    /// the load-and-compare contract without a JIT wired.
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// `static StructureID encode(const Structure*)` (StructureID.h:90-97).
    ///
    /// DIVERGENCE (registry-handle model, see module note): C++ masks the
    /// structure's heap address, whose low bit is free by alignment. We instead
    /// shift the registry handle up by one bit so bit 0 is reserved for the nuke
    /// flag. A null handle (`gc::StructureId::INVALID`, slot 0) encodes to 0,
    /// matching C++ `StructureID(0)` / `!operator bool`.
    pub const fn encode(handle: StructureHandle) -> Self {
        Self::from_bits(handle.raw() << 1)
    }

    /// `inline Structure* decode() const` (StructureID.h:73-78): recover the
    /// referent. Here it yields the registry handle (the analog of the
    /// `Structure*`), decontaminating first exactly as C++'s `decode` does
    /// (`ASSERT(decontaminate())` then read the address bits).
    pub const fn decode(self) -> StructureHandle {
        StructureHandle::new(self.decontaminate().bits >> 1)
    }
}

// =============================== TypeInfoBlob ==============================

/// Faithful port of `class TypeInfoBlob` (TypeInfoBlob.h:34-91): the 32-bit
/// `m_blob` word packing, little-endian, `[indexingModeIncludingHistory(byte0),
/// type(byte1), inlineTypeFlags(byte2), defaultCellState(byte3)]`.
///
/// C++ stores this as a `union { struct fields; uint32_t word; }` and relies on
/// the union to read either view; safe Rust packs/unpacks the bytes explicitly
/// (observation-identical for little-endian targets, which is what the JITs
/// assume — see `TypeInfoBlob::typeInfoBlob`, TypeInfoBlob.h:61-68). The shape
/// byte is an [`IndexingType`] (integrating that leaf port); the type byte is a
/// raw `JSType` value (runtime/JSType.h) carried as `u8` since the per-subclass
/// `JSType` enum is not modeled in this unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TypeInfoBlob {
    word: u32,
}

impl TypeInfoBlob {
    /// `CellState::DefinitelyWhite == 1` (heap/CellState.h:37-38), the value the
    /// C++ blob ctor stores in `u.fields.defaultCellState` (TypeInfoBlob.h:44).
    const DEFAULT_CELL_STATE: u8 = 1;

    /// `TypeInfoBlob(IndexingType, const TypeInfo&)` (TypeInfoBlob.h:39-45).
    pub const fn new(
        indexing_mode_including_history: IndexingType,
        ty: u8,
        inline_type_flags: u8,
    ) -> Self {
        // Little-endian byte packing, matching TypeInfoBlob::typeInfoBlob
        // (TypeInfoBlob.h:61-68, CPU(LITTLE_ENDIAN) arm).
        let word = (indexing_mode_including_history as u32)
            | ((ty as u32) << 8)
            | ((inline_type_flags as u32) << 16)
            | ((Self::DEFAULT_CELL_STATE as u32) << 24);
        Self { word }
    }

    /// `IndexingType indexingModeIncludingHistory() const` (TypeInfoBlob.h:49).
    pub const fn indexing_mode_including_history(self) -> IndexingType {
        self.word as u8
    }

    /// `void setIndexingModeIncludingHistory(IndexingType)` (TypeInfoBlob.h:54).
    pub fn set_indexing_mode_including_history(&mut self, indexing: IndexingType) {
        self.word = (self.word & !0xFF) | indexing as u32;
    }

    /// `JSType type() const` (TypeInfoBlob.h:55), carried as the raw `u8`.
    pub const fn ty(self) -> u8 {
        (self.word >> 8) as u8
    }

    /// `TypeInfo::InlineTypeFlags inlineTypeFlags() const` (TypeInfoBlob.h:56).
    pub const fn inline_type_flags(self) -> u8 {
        (self.word >> 16) as u8
    }

    /// `CellState defaultCellState() const` (TypeInfoBlob.h:59).
    pub const fn default_cell_state(self) -> u8 {
        (self.word >> 24) as u8
    }

    /// `uint32_t blob() const` (TypeInfoBlob.h:70).
    pub const fn blob(self) -> u32 {
        self.word
    }
}

// ============================= PrototypePointer ===========================

/// The prototype reference a structure stores (`m_prototype`, Structure.h:1079).
///
/// C++ holds a `WriteBarrier<Unknown>` (a `JSValue`, usually a `JSObject*` or
/// `jsNull()`). No `JSValue`/`JSObject` cell is wired into this leaf unit, so we
/// carry the prototype object's pointer rep as a `usize` (0 == `jsNull()`), which
/// is also exactly the rep the `StructureTransitionTable` keys `ChangePrototype`
/// transitions on (`PointerKey::from_object`). The write barrier is a no-op here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrototypePointer(usize);

impl PrototypePointer {
    /// `jsNull()` — the absent prototype.
    pub const fn null() -> Self {
        Self(0)
    }

    /// A prototype object's pointer rep.
    pub const fn from_object(object: usize) -> Self {
        Self(object)
    }

    pub const fn raw(self) -> usize {
        self.0
    }
}

// ================================ Structure ===============================

/// Faithful port of `class Structure : public JSCell` (Structure.h:197-1101),
/// restricted to the fields and operations this leaf unit covers: the shape
/// header, the prototype/type info, the property-table slot, the transition
/// table, and the offset bookkeeping that drives property addition,
/// materialization, and table stealing.
///
/// Fields not modeled here (each is a documented out-of-scope dependency):
/// `m_realm`/`m_cachedPrototypeChain` (need `JSGlobalObject`/`StructureChain`
/// cells), `StructureRareData` (`m_previousOrRareData` is reduced to a plain
/// `previous` handle — C++ packs rare data into the same slot once offsets
/// overflow `uint16_t`, a memory optimization), `m_lock` (concurrency is not
/// modeled in this single-threaded leaf), `m_seenProperties` (the
/// `TinyBloomFilter` fast-reject, which only accelerates negative lookups), the
/// dictionary kinds, and the watchpoint sets.
///
/// `Debug` is hand-written (not derived) because the committed `PropertyTable`
/// leaf port does not implement `Debug` and must not be edited from this unit;
/// the impl reports whether a table is materialized rather than its contents.
///
/// `Clone` is derived (now that `PropertyTable`/`StructureTransitionTable` are
/// `Clone`) so the [`StructureIdTable`] can be cloned wholesale — the interpreter's
/// `CoreObjectStore` snapshot/test path deep-clones the structure registry, and
/// every cell's `StructureID` handle stays valid because the clone preserves slot
/// order.
#[derive(Clone)]
pub struct Structure {
    /// `StructureID id()` is `StructureID::encode(this)` in C++ (Structure.h:252).
    /// In the registry model the handle is assigned by [`StructureIdTable::
    /// register`] on insertion; before that it is the null handle.
    handle: StructureHandle,

    /// `TypeInfoBlob m_blob` (Structure.h:1058): the shape/type word.
    blob: TypeInfoBlob,

    /// `uint8_t m_inlineCapacity` (Structure.h:1061).
    inline_capacity: u8,

    /// `WriteBarrier<Unknown> m_prototype` (Structure.h:1079).
    prototype: PrototypePointer,

    /// `WriteBarrier<JSCell> m_previousOrRareData` reduced to the `previousID`
    /// handle (Structure.h:1082, :1129-1138). `None` is the root structure.
    previous: Option<StructureHandle>,

    /// `CompactRefPtr<UniquedStringImpl> m_transitionPropertyName`
    /// (Structure.h:1084): the property added/removed on the edge that produced
    /// this structure. `None` for the root and non-property transitions.
    transition_property_name: Option<AtomId>,

    /// `TransitionPropertyAttributes m_transitionPropertyAttributes`
    /// (Structure.h:1066).
    transition_property_attributes: TransitionPropertyAttributes,

    /// `TransitionKind` stored in `m_bitField` (Structure.h:1065,
    /// `setTransitionKind`). Carried as its own field here.
    transition_kind: TransitionKind,

    /// `uint16_t m_transitionOffset` (Structure.h:1071). DIVERGENCE: C++ packs
    /// this into 16 bits with a `useRareDataFlag` overflow path to
    /// `StructureRareData` (Structure.h:1174-1197); we keep the full
    /// [`PropertyOffset`] (`i32`) directly. Observation-neutral: it only removes
    /// the rare-data memory optimization, not any offset value.
    transition_offset: PropertyOffset,

    /// `uint16_t m_maxOffset` (Structure.h:1072). Same divergence as
    /// `transition_offset`.
    max_offset: PropertyOffset,

    /// `uint32_t m_propertyHash` (Structure.h:1074).
    property_hash: u32,

    /// `StructureTransitionTable m_transitionTable` (Structure.h:1088): outgoing
    /// edges keyed by `(property uid / prototype / null, attributes, kind)`.
    transition_table: StructureTransitionTable,

    /// `WriteBarrier<PropertyTable> m_propertyTableUnsafe` (Structure.h:1092).
    /// `None` means "not materialized" (the table was moved to a child via a
    /// transition, or never built); rebuild with [`StructureIdTable::
    /// materialize_property_table`]. Owned directly (`Option` for the move-out
    /// steal), since the safe-Rust port cannot share a raw `PropertyTable*`.
    property_table: Option<PropertyTable>,

    /// `isPinnedPropertyTable` bit of `m_bitField` (Structure.cpp:250,
    /// StructureInlines.h:477-483): a pinned table belongs to a dictionary and
    /// must be CLONED rather than moved on a transition.
    pinned_property_table: bool,

    /// `didTransition` bit of `m_bitField` (Structure.cpp:263/355): set on
    /// structures produced by a transition (i.e. non-root).
    did_transition: bool,
}

impl std::fmt::Debug for Structure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Structure")
            .field("handle", &self.handle)
            .field("blob", &self.blob)
            .field("inline_capacity", &self.inline_capacity)
            .field("previous", &self.previous)
            .field("transition_property_name", &self.transition_property_name)
            .field("transition_kind", &self.transition_kind)
            .field("transition_offset", &self.transition_offset)
            .field("max_offset", &self.max_offset)
            // The committed PropertyTable leaf has no Debug; report presence only.
            .field("has_property_table", &self.property_table.is_some())
            .field("pinned_property_table", &self.pinned_property_table)
            .finish_non_exhaustive()
    }
}

impl Structure {
    /// Faithful subset of the public ctor `Structure(VM&, JSGlobalObject*,
    /// JSValue prototype, const TypeInfo&, const ClassInfo*, IndexingType,
    /// unsigned inlineCapacity)` (Structure.cpp:234-280): a fresh ROOT structure.
    fn new_root(
        prototype: PrototypePointer,
        indexing_type: IndexingType,
        ty: u8,
        inline_type_flags: u8,
        inline_capacity: u8,
    ) -> Self {
        // ASSERT(static_cast<PropertyOffset>(inlineCapacity) < firstOutOfLineOffset)
        // (Structure.cpp:272). firstOutOfLineOffset == 64 (property_offset.rs:37).
        debug_assert!(
            (inline_capacity as PropertyOffset) < super::property_offset::FIRST_OUT_OF_LINE_OFFSET
        );
        Self {
            handle: StructureHandle::INVALID,
            blob: TypeInfoBlob::new(indexing_type, ty, inline_type_flags),
            inline_capacity,
            prototype,
            previous: None,
            transition_property_name: None,
            transition_property_attributes: 0,
            transition_kind: TransitionKind::Unknown,
            transition_offset: INVALID_OFFSET, // setTransitionOffset(vm, invalidOffset)
            max_offset: INVALID_OFFSET,        // setMaxOffset(vm, invalidOffset)
            property_hash: 0,
            transition_table: StructureTransitionTable::new(),
            property_table: None,
            pinned_property_table: false, // setIsPinnedPropertyTable(false)
            did_transition: false,        // setDidTransition(false)
        }
    }

    /// Faithful subset of `Structure(VM&, StructureVariant, Structure* previous)`
    /// (Structure.cpp:329-383): a child that inherits the previous structure's
    /// layout-relevant state. The transition-specific fields
    /// (`transition_property_name`/`_attributes`/`_kind`/`transition_offset`,
    /// `property_table`, `max_offset`) are filled by
    /// [`StructureIdTable::add_new_property_transition`], mirroring how C++ sets
    /// them on the freshly created `transition` after the ctor.
    fn new_transition(previous: &Structure, previous_handle: StructureHandle) -> Self {
        Self {
            handle: StructureHandle::INVALID,
            // m_blob = TypeInfoBlob(previous->indexingModeIncludingHistory(), typeInfo)
            // (Structure.cpp:362-363): copy the whole blob (same type/flags).
            blob: previous.blob,
            inline_capacity: previous.inline_capacity, // m_inlineCapacity(previous->m_inlineCapacity)
            prototype: previous.prototype,             // m_prototype(previous->m_prototype.get())
            previous: Some(previous_handle),           // setPreviousID(vm, previous)
            transition_property_name: None,
            transition_property_attributes: 0, // setTransitionPropertyAttributes(0)
            transition_kind: TransitionKind::Unknown, // setTransitionKind(Unknown)
            transition_offset: INVALID_OFFSET, // setTransitionOffset(vm, invalidOffset)
            max_offset: INVALID_OFFSET,        // setMaxOffset(vm, invalidOffset)
            property_hash: previous.property_hash, // m_propertyHash(previous->m_propertyHash)
            transition_table: StructureTransitionTable::new(),
            property_table: None,
            pinned_property_table: false, // setIsPinnedPropertyTable(false)
            did_transition: true,         // setDidTransition(true)
        }
    }

    // --- public accessors (Structure.h) ---

    /// `StructureID id() const` (Structure.h:252): the encoded header word.
    pub fn id(&self) -> StructureId {
        StructureId::encode(self.handle)
    }

    /// The raw registry handle (the decoded form of [`Self::id`]).
    pub fn handle(&self) -> StructureHandle {
        self.handle
    }

    /// `uint32_t typeInfoBlob() const` (Structure.h:254).
    pub fn type_info_blob(&self) -> TypeInfoBlob {
        self.blob
    }

    /// `IndexingType indexingType() const` (Structure.h:404):
    /// `indexingModeIncludingHistory() & AllWritableArrayTypes`.
    pub fn indexing_type(&self) -> IndexingType {
        self.blob.indexing_mode_including_history() & ALL_WRITABLE_ARRAY_TYPES
    }

    /// `IndexingType indexingMode() const` (Structure.h:405):
    /// `indexingModeIncludingHistory() & AllArrayTypes`.
    pub fn indexing_mode(&self) -> IndexingType {
        self.blob.indexing_mode_including_history() & ALL_ARRAY_TYPES
    }

    /// `IndexingType indexingModeIncludingHistory() const` (Structure.h:412).
    pub fn indexing_mode_including_history(&self) -> IndexingType {
        self.blob.indexing_mode_including_history()
    }

    /// `unsigned inlineCapacity() const` (Structure.h:549-550).
    pub fn inline_capacity(&self) -> u8 {
        self.inline_capacity
    }

    /// `PropertyOffset maxOffset() const` (Structure.h:1149-1157).
    pub fn max_offset(&self) -> PropertyOffset {
        self.max_offset
    }

    /// `PropertyOffset transitionOffset() const` (Structure.h:1174-1182).
    pub fn transition_offset(&self) -> PropertyOffset {
        self.transition_offset
    }

    /// `TransitionKind transitionKind() const` (Structure.h, `setTransitionKind`).
    pub fn transition_kind(&self) -> TransitionKind {
        self.transition_kind
    }

    /// `UniquedStringImpl* transitionPropertyName() const` (Structure.h:838).
    pub fn transition_property_name(&self) -> Option<AtomId> {
        self.transition_property_name
    }

    /// `Structure* previousID() const` (Structure.h:1129-1138), as a handle.
    pub fn previous_id(&self) -> Option<StructureHandle> {
        self.previous
    }

    /// `PropertyTable* propertyTableOrNull() const` (Structure.h:1003-1006).
    pub fn property_table_or_null(&self) -> Option<&PropertyTable> {
        self.property_table.as_ref()
    }

    /// `bool isPinnedPropertyTable()` (the `m_bitField` bit). Exposed (with the
    /// setter below) so callers/tests can exercise the pinned clone-on-steal
    /// path without a dictionary transition wired.
    pub fn is_pinned_property_table(&self) -> bool {
        self.pinned_property_table
    }

    /// `void setIsPinnedPropertyTable(bool)`.
    pub fn set_pinned_property_table(&mut self, pinned: bool) {
        self.pinned_property_table = pinned;
    }

    /// `bool didTransition()` (the `m_bitField` bit).
    pub fn did_transition(&self) -> bool {
        self.did_transition
    }

    /// The `m_propertyHash` stand-in for a property uid (Identifier.h:235
    /// `existingSymbolAwareHash`).
    ///
    /// DIVERGENCE: JSC XORs the `StringImpl`'s cached content hash; that hash is
    /// not reachable from the [`AtomId`] handle in this leaf (same situation as
    /// `property_table.rs::rep_hash`), so we mix the uid deterministically with
    /// Knuth's multiplicative constant. `m_propertyHash` only feeds the poly-proto
    /// heuristic (`shouldConvertToPolyProto`, StructureInlines.h:485-529), which
    /// is not modeled here; the value just has to be a deterministic function of
    /// the property set.
    fn property_name_hash(name: AtomId) -> u32 {
        name.table_slot().wrapping_mul(2654435761)
    }

    /// Faithful port of the `ShouldPin::No` body of `template<ShouldPin, Func>
    /// PropertyOffset Structure::add(VM&, PropertyName, unsigned, const Func&)`
    /// (StructureInlines.h:232-287) plus the `setMaxOffset` callback
    /// (Structure.cpp:1301-1308).
    ///
    /// Adds `name` to this structure's OWN (already-materialized) property table
    /// at the next free offset, updates `m_propertyHash` and `m_maxOffset`, and
    /// returns the new offset. Precondition: `property_table` is `Some` — every
    /// caller in this unit sets the table first (the transition path steals one
    /// in before calling `add`), mirroring C++'s `ensurePropertyTable` +
    /// `setPropertyTable` immediately preceding `table->add`. The
    /// `ensurePropertyTable` materialize arm and the several `m_bitField` flag
    /// updates (`setHasNonEnumerableProperties`, `setContainsReadOnlyProperties`,
    /// quick-enumeration, `__proto__`/`then` specials) are deferred — they don't
    /// affect the offset/table result this unit proves.
    fn add(&mut self, name: AtomId, attributes: u32) -> PropertyOffset {
        let inline_capacity = self.inline_capacity as i32;
        let hash = Self::property_name_hash(name);

        let table = self
            .property_table
            .as_mut()
            .expect("Structure::add requires a materialized property table (set by the caller)");

        // PropertyOffset newOffset = table->nextOffset(m_inlineCapacity);
        let new_offset = table.next_offset(inline_capacity);

        // auto [offset, attribute, result] = table->add(vm, PropertyTableEntry(rep, newOffset, attributes));
        let (offset, _attribute, result) =
            table.add(PropertyTableEntry::new(name, new_offset, attributes));
        debug_assert!(result, "add of a new property must insert");
        debug_assert_eq!(offset, new_offset, "table offset must match nextOffset");

        // m_propertyHash = m_propertyHash ^ rep->existingSymbolAwareHash();
        self.property_hash ^= hash;

        // auto newMaxOffset = std::max(newOffset, maxOffset()); ... setMaxOffset(vm, newMaxOffset);
        self.max_offset = new_offset.max(self.max_offset);
        new_offset
    }
}

// ============================= StructureIdTable ===========================

/// The per-VM `StructureID` <-> `Structure` registry (in C++ implicit: a
/// `StructureID` IS the structure's masked heap address; StructureID.h:73-97).
///
/// Per the committed serial decision *Heap OWNS the StructureIDTable; StructureID
/// is a registry handle*, this table owns the `Structure` cells in a `Vec` and
/// hands out 1-based [`gc::StructureId`] handles. Slot 0 is the null/invalid
/// handle (`gc::StructureId::INVALID`), so it is never allocated; handle `n`
/// addresses `structures[n - 1]`. Because the cells live here and reference each
/// other by handle, the `static Structure*` transition/materialization methods of
/// C++ are ported as methods on this table.
#[derive(Clone, Debug, Default)]
pub struct StructureIdTable {
    structures: Vec<Structure>,
}

impl StructureIdTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a structure, assign its 1-based handle, and return it. The handle
    /// is the registry analog of `StructureID::encode(this)` being fixed by the
    /// structure's heap address at allocation.
    fn register(&mut self, mut structure: Structure) -> StructureHandle {
        let handle = StructureHandle::new((self.structures.len() + 1) as u32);
        structure.handle = handle;
        self.structures.push(structure);
        handle
    }

    /// Borrow a structure by handle (the registry analog of `StructureID::
    /// decode`).
    pub fn structure(&self, handle: StructureHandle) -> &Structure {
        &self.structures[(handle.raw() - 1) as usize]
    }

    fn structure_mut(&mut self, handle: StructureHandle) -> &mut Structure {
        &mut self.structures[(handle.raw() - 1) as usize]
    }

    /// Create and register a fresh ROOT structure
    /// (`Structure::create(VM&, JSGlobalObject*, JSValue, const TypeInfo&, ...)`,
    /// StructureInlines.h via Structure.cpp:234).
    pub fn create_root(
        &mut self,
        prototype: PrototypePointer,
        indexing_type: IndexingType,
        ty: u8,
        inline_type_flags: u8,
        inline_capacity: u8,
    ) -> StructureHandle {
        let structure = Structure::new_root(
            prototype,
            indexing_type,
            ty,
            inline_type_flags,
            inline_capacity,
        );
        self.register(structure)
    }

    /// `static Structure* addPropertyTransition(VM&, Structure*, PropertyName,
    /// unsigned attributes, PropertyOffset&)` (Structure.cpp:561-568).
    ///
    /// First tries to reuse an existing transition (the sibling-sharing fast
    /// path) and otherwise creates a new one. Returns `(child handle, offset)`.
    pub fn add_property_transition(
        &mut self,
        base: StructureHandle,
        name: AtomId,
        attributes: u32,
    ) -> (StructureHandle, PropertyOffset) {
        if let Some(existing) = self.add_property_transition_to_existing(base, name, attributes) {
            return existing;
        }
        self.add_new_property_transition(base, name, attributes)
    }

    /// `static Structure* addPropertyTransitionToExistingStructureImpl(Structure*,
    /// UniquedStringImpl* uid, unsigned attributes, PropertyOffset&)`
    /// (StructureInlines.h:549-566): if `base` already has a `PropertyAddition`
    /// transition for `(name, attributes)`, reuse the SAME child structure — this
    /// is the convergence that keeps two objects growing the same shape
    /// monomorphic. The `hasBeenDictionary()` early-out (always false here, no
    /// dictionaries modeled) is omitted.
    fn add_property_transition_to_existing(
        &self,
        base: StructureHandle,
        name: AtomId,
        attributes: u32,
    ) -> Option<(StructureHandle, PropertyOffset)> {
        let rep = PointerKey::from_uid(name.table_slot() as usize);
        let target = self.structure(base).transition_table.get(
            rep,
            attributes,
            TransitionKind::PropertyAddition,
        )?;
        // validateOffset(existingTransition->transitionOffset(), ...); offset = existingTransition->transitionOffset();
        let offset = self.structure(target).transition_offset;
        Some((target, offset))
    }

    /// `static Structure* addNewPropertyTransition(VM&, Structure*, PropertyName,
    /// unsigned, PropertyOffset&, PutPropertySlot::Context, Deferred...)`
    /// (Structure.cpp:570-625), non-dictionary path.
    ///
    /// Creates the child, MOVES the base's property table into it (cloning only
    /// if pinned), adds the property at the next offset, records the transition
    /// edge on the base so future identical adds converge, and returns
    /// `(child handle, offset)`.
    fn add_new_property_transition(
        &mut self,
        base: StructureHandle,
        name: AtomId,
        attributes: u32,
    ) -> (StructureHandle, PropertyOffset) {
        debug_assert!(attributes <= u8::MAX as u32);

        // Structure* transition = Structure::create(vm, structure, deferred);
        let mut transition = {
            let base_structure = self.structure(base);
            Structure::new_transition(base_structure, base)
        };

        // transition->m_blob.setIndexingModeIncludingHistory(structure->indexingModeIncludingHistory() & ~CopyOnWrite);
        let base_indexing = self.structure(base).indexing_mode_including_history();
        transition
            .blob
            .set_indexing_mode_including_history(base_indexing & !COPY_ON_WRITE);

        // transition->m_transitionPropertyName = propertyName.uid();
        transition.transition_property_name = Some(name);
        // transition->setTransitionPropertyAttributes(attributes);
        transition.transition_property_attributes = attributes as TransitionPropertyAttributes;
        // transition->setTransitionKind(TransitionKind::PropertyAddition);
        transition.transition_kind = TransitionKind::PropertyAddition;
        // transition->setPropertyTable(vm, structure->takePropertyTableOrCloneIfPinned(vm));
        transition.property_table = Some(self.take_property_table_or_clone_if_pinned(base));
        // transition->setMaxOffset(vm, structure->maxOffset());
        transition.max_offset = self.structure(base).max_offset;

        // offset = transition->add(vm, propertyName, attributes);
        let offset = transition.add(name, attributes);
        // transition->setTransitionOffset(vm, offset);
        transition.transition_offset = offset;

        let transition_handle = self.register(transition);

        // structure->m_transitionTable.add(vm, structure, transition);
        // The table re-derives the key from the transition structure's fields
        // (createKeyFromStructure, StructureInlines.h:580-588); we hand it the
        // already-resolved record (see TransitionStructure docs).
        let record = TransitionStructure::new(
            transition_handle,
            PointerKey::from_uid(name.table_slot() as usize),
            attributes as TransitionPropertyAttributes,
            TransitionKind::PropertyAddition,
        );
        self.structure_mut(base).transition_table.add(record);

        (transition_handle, offset)
    }

    /// `PropertyTable* Structure::takePropertyTableOrCloneIfPinned(VM&)`
    /// (Structure.cpp:912-925). "This must always return a property table."
    ///
    /// If the base owns a table: clone it when pinned (it stays shared by the
    /// dictionary), otherwise MOVE it out (`Option::take`, the common case,
    /// leaving the base to rematerialize). If the base has no table, rebuild one
    /// by materialization without caching it back.
    pub fn take_property_table_or_clone_if_pinned(
        &mut self,
        base: StructureHandle,
    ) -> PropertyTable {
        let has_table = self.structure(base).property_table.is_some();
        if has_table {
            if self.structure(base).pinned_property_table {
                // return result->copy(vm, result->size() + 1);
                let table = self.structure(base).property_table.as_ref().unwrap();
                let capacity = table.size() + 1;
                return clone_property_table(table, capacity);
            }
            // setPropertyTable(vm, nullptr); return result;
            return self.structure_mut(base).property_table.take().unwrap();
        }
        // bool setPropertyTable = false; return materializePropertyTable(vm, setPropertyTable);
        self.materialize_property_table(base)
    }

    /// `bool Structure::findStructuresAndMapForMaterialization(Vector<Structure*,
    /// 8>&, Structure*&, PropertyTable*&)` (Structure.cpp:432-454): walk the
    /// `previousID` chain from `handle`, collecting the structures with no table,
    /// until one OWNS a table (or the chain ends). Returns the collected handles
    /// (in `this -> ... -> deepest` order) and the owner's table (if any), found
    /// at `owner = deepest->previousID()`.
    fn find_structures_for_materialization(
        &self,
        handle: StructureHandle,
    ) -> (Vec<StructureHandle>, Option<&PropertyTable>) {
        let mut visited = Vec::new();
        let mut current = Some(handle);
        while let Some(h) = current {
            let structure = self.structure(h);
            if let Some(table) = structure.property_table.as_ref() {
                return (visited, Some(table));
            }
            visited.push(h);
            current = structure.previous;
        }
        (visited, None)
    }

    /// `PropertyTable* Structure::materializePropertyTable(VM&, bool
    /// setPropertyTable)` (Structure.cpp:456-533), with `setPropertyTable ==
    /// false` (pure replay — it returns a fresh table and does not cache it back,
    /// which is the arm `takePropertyTableOrCloneIfPinned`/`copyPropertyTableFor
    /// Pinning` use; the caching arm is a memory optimization deferred with the
    /// GC wiring).
    ///
    /// Starts from a copy of the nearest ancestor's table (or a fresh empty table
    /// sized for this structure's capacity) and REPLAYS each collected
    /// transition, oldest first, rebuilding the exact `(key, offset, attributes)`
    /// set this structure should expose.
    pub fn materialize_property_table(&self, handle: StructureHandle) -> PropertyTable {
        let target = self.structure(handle);
        // unsigned capacity = numberOfSlotsForMaxOffset(maxOffset(), m_inlineCapacity);
        let capacity =
            number_of_slots_for_max_offset(target.max_offset, target.inline_capacity as i32) as u32;

        let (visited, found_table) = self.find_structures_for_materialization(handle);
        let mut table = match found_table {
            // table = table->copy(vm, capacity);
            Some(found) => clone_property_table(found, capacity),
            // table = PropertyTable::create(vm, capacity);
            None => PropertyTable::with_capacity(capacity),
        };

        // for (size_t i = structures.size(); i--;) { ... } -- oldest transition first.
        for &h in visited.iter().rev() {
            let structure = self.structure(h);
            // if (!structure->m_transitionPropertyName) continue;
            let Some(name) = structure.transition_property_name else {
                continue;
            };
            match structure.transition_kind {
                TransitionKind::PropertyAddition => {
                    // auto nextOffset = table->nextOffset(structure->inlineCapacity());
                    // ASSERT(nextOffset == structure->transitionOffset());
                    // nextOffset() is SIDE-EFFECTING: it advances the allocator
                    // state by popping the PropertyTable m_deletedOffsets recycle
                    // stack (PropertyTable::nextOffset -> takeDeletedOffset), so it
                    // MUST run in release too -- it is not merely an assertion.
                    // Wrapping it in debug_assert_eq! would elide the pop in the
                    // release/parity build, leaking a freed offset onto a live
                    // property and clobbering the value-storage mirror.
                    let next_offset = table.next_offset(structure.inline_capacity as i32);
                    debug_assert_eq!(
                        next_offset, structure.transition_offset,
                        "replayed nextOffset must equal the recorded transitionOffset"
                    );
                    let entry = PropertyTableEntry::new(
                        name,
                        structure.transition_offset,
                        u32::from(structure.transition_property_attributes),
                    );
                    let (offset, _attr, result) = table.add(entry);
                    debug_assert!(result);
                    debug_assert_eq!(offset, structure.transition_offset);
                }
                // PropertyDeletion / PropertyAttributeChange / SetBrand
                // (Structure.cpp:499-513) are faithful to model once deletion and
                // attribute-change transitions are ported; this unit creates only
                // PropertyAddition edges, so they are unreachable here.
                _ => {}
            }
        }

        table
    }

    /// `PropertyTable* Structure::copyPropertyTableForPinning(VM&)`
    /// (Structure.cpp:1218-1224): clone the existing table, or materialize one if
    /// absent. Used when a structure becomes a (pinned) dictionary.
    pub fn copy_property_table_for_pinning(&self, handle: StructureHandle) -> PropertyTable {
        let structure = self.structure(handle);
        if let Some(table) = structure.property_table.as_ref() {
            let capacity = number_of_slots_for_max_offset(
                structure.max_offset,
                structure.inline_capacity as i32,
            ) as u32;
            return clone_property_table(table, capacity);
        }
        self.materialize_property_table(handle)
    }

    /// The handle the NEXT [`Self::register`] will assign (1-based). Mirrors the
    /// fact that in C++ a fresh `Structure`'s `StructureID` is fixed by its heap
    /// address at allocation; here it is the next `Vec` slot. Used by the
    /// interpreter to snapshot "did a new structure get allocated".
    pub fn peek_next_handle(&self) -> StructureHandle {
        StructureHandle::new((self.structures.len() + 1) as u32)
    }

    /// Mint a fresh, standalone (pinned dictionary) structure whose property table is
    /// a faithful clone of `base`'s materialized table, with `removed` (if present)
    /// taken out and its freed offset pushed onto the recycle stack.
    ///
    /// Models the NON-`PropertyAddition` shape change — property deletion,
    /// data<->accessor conversion, attribute change — that JSC routes through a
    /// dictionary / `removePropertyTransition` / `attributeChangeTransition`
    /// (Structure.cpp). Those transition KINDS are not yet ported (this unit only
    /// creates `PropertyAddition` edges, see `materialize_property_table`), so this is
    /// the conservative fresh-id path: a new standalone structure (NOT recorded on
    /// `base`'s transition table, so it is per-object, never shared) that carries the
    /// exact SURVIVING offsets — the owning object's out-of-line storage stays valid
    /// (never a wrong slot) — and recycles a removed offset exactly as
    /// `PropertyTable::nextOffset`/`takeDeletedOffset` (PropertyTable.h:471/457) would,
    /// so a later add reuses the freed slot instead of colliding.
    pub fn create_dictionary_from(
        &mut self,
        base: StructureHandle,
        removed: Option<AtomId>,
    ) -> StructureHandle {
        let mut table = self.materialize_property_table(base);
        if let Some(uid) = removed {
            let (offset, _attributes) = table.take(uid);
            if offset != INVALID_OFFSET {
                table.add_deleted_offset(offset);
            }
        }
        let max_offset = self.structure(base).max_offset;
        let mut dictionary = {
            let base_structure = self.structure(base);
            Structure::new_transition(base_structure, base)
        };
        // The dictionary OWNS its (pinned) table and exposes the same max offset, so
        // `materialize`/`next_offset` over it never replays `base` and never shrinks
        // the offset high-water mark across the removal.
        dictionary.property_table = Some(table);
        dictionary.pinned_property_table = true;
        dictionary.max_offset = max_offset;
        self.register(dictionary)
    }

    /// `Structure::attributeChange` core (Structure.cpp:1317 ->
    /// `PropertyTable::updateAttributeIfExists`, PropertyTable.h:444): update `uid`'s
    /// attributes IN PLACE in `handle`'s OWNED (pinned dictionary) PropertyTable,
    /// KEEPING its offset. The faithful `attributeChangeTransition` (Structure.cpp:806)
    /// on a per-object dictionary: an offset-stable kind/attribute change (data<->
    /// accessor, accessor getter/setter update, data attribute change). No-op if the
    /// structure carries no owned table or the key is absent.
    pub fn update_attributes(&mut self, handle: StructureHandle, uid: AtomId, attributes: u32) {
        if let Some(table) = self.structure_mut(handle).property_table.as_mut() {
            table.update_attribute_if_exists(uid, attributes);
        }
    }
}

/// `PropertyTable* PropertyTable::copy(VM&, unsigned newCapacity)` /
/// `PropertyTable::clone` (PropertyTable.h, not ported in the leaf).
///
/// Now that `PropertyTable` derives `Clone`, this is a faithful deep copy: it
/// preserves the index layout, the insertion-ordered value array, AND the
/// `m_deletedOffsets` recycle stack — exactly what a true `PropertyTable::copy`
/// preserves and what the dictionary/deletion path below relies on so a
/// subsequently added property recycles a freed offset rather than colliding.
/// `new_capacity` is advisory (the table rehashes on demand); it is kept in the
/// signature to mirror the C++ `copy(VM&, newCapacity)` call shape.
fn clone_property_table(source: &PropertyTable, _new_capacity: u32) -> PropertyTable {
    source.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::property_offset::{offset_for_property_number, FIRST_OUT_OF_LINE_OFFSET};

    // A plain ordinary object's type/flags for the blob; values are illustrative
    // (JSType::FinalObjectType == 33, runtime/JSType.h:78). Only the indexing byte
    // and the offset math are load-bearing in these tests.
    const FINAL_OBJECT_TYPE: u8 = 33;
    const NO_FLAGS: u8 = 0;

    fn atom(slot: u32) -> AtomId {
        AtomId::from_table_slot(slot)
    }

    fn fresh_root(table: &mut StructureIdTable, inline_capacity: u8) -> StructureHandle {
        table.create_root(
            PrototypePointer::null(),
            super::super::indexing_type::NON_ARRAY,
            FINAL_OBJECT_TYPE,
            NO_FLAGS,
            inline_capacity,
        )
    }

    // StructureID.h:39-97: encode/decode round-trip, nuke reserves the low bit,
    // and the null handle encodes to an invalid (zero) id.
    #[test]
    fn structure_id_encode_decode_and_nuke() {
        let handle = StructureHandle::new(7);
        let id = StructureId::encode(handle);

        assert!(id.is_valid());
        assert!(!id.is_nuked());
        assert_eq!(id.decode(), handle);
        // The encoded word is the handle shifted up one bit (low bit reserved).
        assert_eq!(id.bits(), 7 << 1);
        // The JIT shape guard loads exactly this word from [cell + 0].
        assert_eq!(StructureId::CELL_STRUCTURE_ID_OFFSET, 0);

        let nuked = id.nuke();
        assert!(nuked.is_nuked());
        assert_eq!(nuked.bits() & StructureId::NUKED_STRUCTURE_ID_BIT, 1);
        // decontaminate clears the nuke bit; decode still recovers the handle.
        assert_eq!(nuked.decontaminate(), id);
        assert_eq!(nuked.decode(), handle);

        // The null/invalid handle (slot 0) maps to a zero, invalid id.
        let invalid = StructureId::encode(StructureHandle::INVALID);
        assert!(!invalid.is_valid());
        assert_eq!(invalid.bits(), 0);
        assert_eq!(StructureId::default(), invalid);
    }

    // TypeInfoBlob.h:39-70: the 32-bit little-endian byte packing and the
    // indexing-byte mutation, plus Structure's indexingType masking (Structure.h:404).
    #[test]
    fn type_info_blob_packs_and_masks() {
        use super::super::indexing_type::{ARRAY_WITH_INT32, NON_ARRAY};

        let mut blob = TypeInfoBlob::new(NON_ARRAY, FINAL_OBJECT_TYPE, 0x12);
        assert_eq!(blob.indexing_mode_including_history(), NON_ARRAY);
        assert_eq!(blob.ty(), FINAL_OBJECT_TYPE);
        assert_eq!(blob.inline_type_flags(), 0x12);
        // defaultCellState == DefinitelyWhite == 1 (TypeInfoBlob.h:44).
        assert_eq!(blob.default_cell_state(), 1);
        // Word layout: byte0 indexing, byte1 type, byte2 flags, byte3 cellState.
        assert_eq!(blob.blob(), 0x01_12_21_00);

        blob.set_indexing_mode_including_history(ARRAY_WITH_INT32);
        assert_eq!(blob.indexing_mode_including_history(), ARRAY_WITH_INT32);
        // Mutating the indexing byte leaves type/flags/cellState intact.
        assert_eq!(blob.ty(), FINAL_OBJECT_TYPE);
        assert_eq!(blob.inline_type_flags(), 0x12);
        assert_eq!(blob.default_cell_state(), 1);
    }

    // StructureInlines.h:232-287 + property_offset.rs: a property addition lands
    // at the next offset, and a second addition advances to the following offset.
    #[test]
    fn transition_adds_an_offset() {
        // inline_capacity 6: the first six properties are inline offsets 0..6.
        let mut table = StructureIdTable::new();
        let root = fresh_root(&mut table, 6);

        let (s1, off_x) = table.add_property_transition(root, atom(1), 0);
        assert_eq!(off_x, offset_for_property_number(0, 6)); // 0

        let (s2, off_y) = table.add_property_transition(s1, atom(2), 0);
        assert_eq!(off_y, offset_for_property_number(1, 6)); // 1

        // The child structure now owns a table exposing x@0 and y@1.
        let s2_table = table.structure(s2).property_table_or_null().unwrap();
        assert_eq!(s2_table.get(atom(1)), (off_x, 0));
        assert_eq!(s2_table.get(atom(2)), (off_y, 0));
        assert_eq!(table.structure(s2).max_offset(), off_y);
        assert_eq!(table.structure(s2).transition_offset(), off_y);

        // With inline_capacity 0, the first property jumps to the out-of-line band
        // (firstOutOfLineOffset == 64), proving the offset math is the leaf port's.
        let mut table0 = StructureIdTable::new();
        let root0 = fresh_root(&mut table0, 0);
        let (_, off0) = table0.add_property_transition(root0, atom(1), 0);
        assert_eq!(off0, FIRST_OUT_OF_LINE_OFFSET); // 64
    }

    // StructureInlines.h:549-566: two objects adding the same property to the same
    // base converge on the SAME child structure (monomorphic IC sharing).
    #[test]
    fn sibling_transitions_converge() {
        let mut table = StructureIdTable::new();
        let root = fresh_root(&mut table, 6);

        let (a, off_a) = table.add_property_transition(root, atom(1), 0);
        let (b, off_b) = table.add_property_transition(root, atom(1), 0);
        assert_eq!(
            a, b,
            "same (name, attributes) transition must reuse the child"
        );
        assert_eq!(off_a, off_b);

        // A DIFFERENT property (or different attributes) produces a distinct child.
        let (c, _) = table.add_property_transition(root, atom(2), 0);
        assert_ne!(a, c);
        let (d, _) = table.add_property_transition(root, atom(1), 4);
        assert_ne!(a, d);

        // The shared edge is also visible through the base's transition table:
        // a second add resolves via add_property_transition_to_existing.
        let reused = table.add_property_transition_to_existing(root, atom(1), 0);
        assert_eq!(reused, Some((a, off_a)));
    }

    // Structure.cpp:456-533: a structure whose table was moved to a child can
    // rebuild its exact table by replaying the transition chain.
    #[test]
    fn materialize_replays() {
        let mut table = StructureIdTable::new();
        let root = fresh_root(&mut table, 6);

        // root --a--> sa --b--> sb. Building sb steals sa's table, so sa has none.
        let (sa, off_a) = table.add_property_transition(root, atom(10), 0);
        let (sb, off_b) = table.add_property_transition(sa, atom(11), 0);

        assert!(
            table.structure(sa).property_table_or_null().is_none(),
            "sa's table was moved to sb"
        );

        // Materialize sa: replay reconstructs exactly {a@off_a}.
        let sa_table = table.materialize_property_table(sa);
        assert_eq!(sa_table.size(), 1);
        assert_eq!(sa_table.get(atom(10)), (off_a, 0));
        assert_eq!(sa_table.get(atom(11)), (INVALID_OFFSET, 0));

        // sb still owns its table directly: {a, b} with the original offsets.
        let sb_table = table.structure(sb).property_table_or_null().unwrap();
        assert_eq!(sb_table.size(), 2);
        assert_eq!(sb_table.get(atom(10)), (off_a, 0));
        assert_eq!(sb_table.get(atom(11)), (off_b, 0));

        // Materializing the root (no previous, no table) yields an empty table.
        let root_table = table.materialize_property_table(root);
        assert_eq!(root_table.size(), 0);
    }

    // Structure.cpp:912-925: an unpinned steal MOVES the table out; a pinned base
    // CLONES it, leaving the original in place.
    #[test]
    fn steal_moves_the_table_and_pinned_clones() {
        // Unpinned: the table is moved out of the base.
        let mut table = StructureIdTable::new();
        let root = fresh_root(&mut table, 6);
        let (sa, off_a) = table.add_property_transition(root, atom(1), 0);
        assert!(table.structure(sa).property_table_or_null().is_some());

        let stolen = table.take_property_table_or_clone_if_pinned(sa);
        assert_eq!(stolen.get(atom(1)), (off_a, 0));
        assert!(
            table.structure(sa).property_table_or_null().is_none(),
            "an unpinned steal leaves the base with no table"
        );

        // Pinned: the table is cloned and the base keeps its own.
        let mut table2 = StructureIdTable::new();
        let root2 = fresh_root(&mut table2, 6);
        let (sb, off_b) = table2.add_property_transition(root2, atom(1), 0);
        table2.structure_mut(sb).set_pinned_property_table(true);

        let cloned = table2.take_property_table_or_clone_if_pinned(sb);
        assert_eq!(cloned.get(atom(1)), (off_b, 0));
        let kept = table2.structure(sb).property_table_or_null();
        assert!(kept.is_some(), "a pinned base keeps its table");
        assert_eq!(kept.unwrap().get(atom(1)), (off_b, 0));
    }
}
