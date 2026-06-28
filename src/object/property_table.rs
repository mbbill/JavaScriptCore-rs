//! Faithful port of C++ JSC `runtime/PropertyTable.h:85-636` (+ `PropertyTable.cpp`).
//!
//! `PropertyTable` is JSC's shared, INSERTION-ORDERED property map: an
//! open-addressing hash index over a flat, append-only array of
//! `(key, PropertyOffset, attributes)` entries. It is the table a `Structure`
//! materializes to answer named-property lookups. The lookup is a double-hash
//! (linear-with-growing-stride / triangular) probe; deletes leave tombstones and
//! are reclaimed by `rehash`; freed `PropertyOffset`s are recycled through a LIFO
//! `m_deletedOffsets` stack.
//!
//! ## C++ -> Rust structural mapping
//!
//! - `PropertyTable` (PropertyTable.h:85) -> [`PropertyTable`].
//! - `PropertyTableEntry` (Structure.h:144-174) -> [`PropertyTableEntry`].
//! - `PropertyTable::KeyType = UniquedStringImpl*` (PropertyTable.h:104) ->
//!   [`AtomId`], the uniqued atom/symbol handle. This follows the landed serial
//!   decision "StringImpl = Rc immutable off-heap; atoms are uniqued handles": the
//!   table keys on identity, and `AtomId` IS that identity. JSC packs two pointer
//!   sentinels into the key field -- `nullptr` (empty/unused slot) and
//!   `PROPERTY_MAP_DELETED_ENTRY_KEY == (UniquedStringImpl*)1` (PropertyTable.h:39,
//!   the tombstone). Rust keeps the key a safe handle and lifts those two
//!   sentinels into the [`EntryKey`] enum instead of overloading a raw pointer.
//!
//! ## Intentional, observation-neutral divergences (commented at each site)
//!
//! - COMPACT vs NON-COMPACT (PropertyTable.h:60-83, 200-244). JSC co-allocates the
//!   index vector and the value array in ONE tagged `malloc` and, while both the
//!   `PropertyOffset` and the index fit in a `uint8_t`, stores them in a "compact"
//!   form (`uint8_t` index + `CompactPropertyTableEntry`) to save memory, falling
//!   back to a `uint32_t` index + `PropertyTableEntry` once they grow. That is a
//!   pure MEMORY-FOOTPRINT optimization: it changes neither lookup results, probe
//!   order, insertion order, nor capacities -- only the storage width and the
//!   moment `rehash` swaps representation. A safe-Rust port cannot express the
//!   tagged-pointer/`bit_cast`/co-allocation tricks without `unsafe`, and this file
//!   lives under `#![deny(unsafe_op_in_unsafe_fn)]`. So we always use the
//!   non-compact form: index vector as `Vec<u32>`, value array as
//!   `Vec<PropertyTableEntry>`. The `canFitInCompact`/`canStayCompact` machinery
//!   then collapses -- `can_insert` keeps only the capacity test and `rehash`
//!   ignores its `can_stay_compact` flag (both noted at the code sites).
//! - HASH FUNCTION (Identifier.h:234-237). JSC hashes a key with
//!   `IdentifierRepHash::hash(key) == key->existingSymbolAwareHash()`, i.e. the
//!   string-content hash precomputed and cached inside the `StringImpl`. At this
//!   layer the key is the `AtomId` handle (a table slot), and that cached content
//!   hash is not reachable, so [`PropertyTable::rep_hash`] derives the probe hash
//!   from the uid via WTF's integer hash (`WTF/HashFunctions.h:35` `intHash`). The
//!   open-addressing table is correct for ANY deterministic key->hash function;
//!   only the probe DISTRIBUTION differs from JSC. When `AtomId` is later wired to
//!   surface the `StringImpl` hash, swap `rep_hash` for that load to match JSC.
//! - REF/DEREF. JSC `ref()`s the key on insert and `deref()`s on remove/destroy to
//!   keep the `StringImpl` alive (PropertyTable.h:381, :411). `AtomId` is a
//!   non-owning handle into the VM atom table, which owns the storage, so those
//!   become no-ops (noted at the sites).
//!
//! ## Out of scope for this leaf unit (external dependencies, deferred)
//!
//! The `JSCell`/GC/VM surface -- `create`/`clone`/`copy`/`destroy`/
//! `visitChildren`/`finishCreation`/`reportExtraMemoryAllocated` -- plus
//! `seal`/`freeze`/`isSealed`/`isFrozen` (need `PropertyName::isPrivateName` and
//! the `PropertyAttribute` flags) and `renumberPropertyOffsets` (needs
//! `JSObject::getDirect`) are NOT ported here; they belong with the Structure/GC
//! wiring. This file ports the standalone open-addressing core and its
//! insertion-ordered iteration, which is the reusable, dependency-free heart of
//! the class.
//!
//! STANDALONE / NOT WIRED. Like `property_offset.rs`, this is the faithful
//! reference, tested against the C++ algorithm but not yet a dependency of any
//! caller; `#![allow(dead_code)]` documents the awaiting-wire state.
#![allow(dead_code)]

use super::property_offset::{offset_for_property_number, PropertyOffset, INVALID_OFFSET};
use crate::strings::AtomId;

// C++ JSC `PropertyTable.h:298` `findImpl` / `:516` `reinsert`: a slot of the
// index vector holding `EmptyEntryIndex` (0) means "no entry here". 1-based entry
// indices reference the value array at `[entryIndex - 1]`.
// (`PropertyTable.h:185` `static constexpr unsigned EmptyEntryIndex = 0;`).
const EMPTY_ENTRY_INDEX: u32 = 0;

// C++ JSC `PropertyTable.h:281` `static constexpr unsigned MinimumTableSize = 16;`
// "compact index is uint8_t and we should keep 16 byte aligned entries".
const MINIMUM_TABLE_SIZE: u32 = 16;

/// The three states JSC packs into a `PropertyTableEntry`'s `UniquedStringImpl*`
/// key field. Faithful to the two pointer sentinels (`PropertyTable.h:39`,
/// Structure.h:144-174); see the module-level KeyType note.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKey {
    /// `nullptr`: an unused value slot, or the trailing iteration/deleted guard
    /// slot. Never matches a real key.
    Empty,
    /// `PROPERTY_MAP_DELETED_ENTRY_KEY == (UniquedStringImpl*)1`: a tombstoned
    /// slot left by `remove`; skipped by iteration and by `rehash`. Never matches
    /// a real key.
    Deleted,
    /// A live uniqued property key.
    Key(AtomId),
}

/// Faithful port of `PropertyTableEntry` (runtime/Structure.h:144-174): the value
/// record stored in the table -- a uniqued key, its [`PropertyOffset`], and its
/// attributes byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyTableEntry {
    // Structure.h:171 `UniquedStringImpl* m_key { nullptr };`
    key: EntryKey,
    // Structure.h:172 `PropertyOffset m_offset { 0 };`
    offset: PropertyOffset,
    // Structure.h:173 `uint8_t m_attributes { 0 };`. Stored as `u32` to match the
    // `unsigned attributes` API surface; the JSC entry narrows to a 7-bit byte
    // (PropertySlot.h:45 "This must be 7 bits"), preserved by the debug_assert.
    attributes: u32,
}

impl PropertyTableEntry {
    /// `PropertyTableEntry(UniquedStringImpl*, PropertyOffset, unsigned)`
    /// (Structure.h:148). The C++ ctor `ASSERT(this->attributes() == attributes)`,
    /// i.e. the value must round-trip through the stored `uint8_t`.
    pub fn new(key: AtomId, offset: PropertyOffset, attributes: u32) -> Self {
        debug_assert!(
            attributes <= u8::MAX as u32,
            "PropertyTableEntry attributes must fit in a byte"
        );
        Self {
            key: EntryKey::Key(key),
            offset,
            attributes,
        }
    }

    /// JSC's default-constructed `PropertyTableEntry` (Structure.h:146): null key,
    /// offset 0, attributes 0. Used to zero-fill the value array (mirroring
    /// `allocateZeroedIndexVector`, PropertyTable.h:548).
    fn empty() -> Self {
        Self {
            key: EntryKey::Empty,
            offset: 0,
            attributes: 0,
        }
    }

    fn raw_key(&self) -> EntryKey {
        self.key
    }

    /// The live key of a non-tombstone entry. Returns `None` for empty/deleted
    /// slots. (`UniquedStringImpl* key()`, Structure.h:163.)
    pub fn key(&self) -> Option<AtomId> {
        match self.key {
            EntryKey::Key(atom) => Some(atom),
            EntryKey::Empty | EntryKey::Deleted => None,
        }
    }

    /// Precondition: the entry holds a live key (the table only ever hashes/probes
    /// real entries through this).
    fn require_key(&self) -> AtomId {
        match self.key {
            EntryKey::Key(atom) => atom,
            EntryKey::Empty | EntryKey::Deleted => {
                unreachable!("PropertyTable hashed an entry without a live key")
            }
        }
    }

    /// `void setKey(UniquedStringImpl*)` with `PROPERTY_MAP_DELETED_ENTRY_KEY`
    /// (PropertyTable.h:405): tombstone this value slot.
    fn set_deleted(&mut self) {
        self.key = EntryKey::Deleted;
    }

    /// `PropertyOffset offset()` (Structure.h:165).
    pub fn offset(&self) -> PropertyOffset {
        self.offset
    }

    /// `void setOffset(PropertyOffset)` (Structure.h:166).
    pub fn set_offset(&mut self, offset: PropertyOffset) {
        self.offset = offset;
    }

    /// `uint8_t attributes()` (Structure.h:167).
    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    /// `void setAttributes(uint8_t)` (Structure.h:168).
    pub fn set_attributes(&mut self, attributes: u32) {
        debug_assert!(
            attributes <= u8::MAX as u32,
            "PropertyTableEntry attributes must fit in a byte"
        );
        self.attributes = attributes;
    }
}

/// Faithful port of `PropertyTable::FindResult` (PropertyTable.h:120-125): the
/// outcome of a probe -- where the entry is (or where an empty slot to fill is),
/// plus the found offset/attributes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindResult {
    /// 1-based value-array index, or [`EMPTY_ENTRY_INDEX`] when the key is absent.
    pub entry_index: u32,
    /// The index-vector slot the probe stopped at.
    pub index: u32,
    /// The found offset, or [`INVALID_OFFSET`] when absent.
    pub offset: PropertyOffset,
    /// The found attributes, or 0 when absent.
    pub attributes: u32,
}

/// Faithful port of `PropertyTable` (runtime/PropertyTable.h:85-636).
///
/// See the module docs for the C++->Rust mapping and the (observation-neutral)
/// divergences. Ownership skeleton: the table OWNS its index vector and value
/// array directly. JSC co-allocates them in one tagged `malloc`; the safe-Rust
/// port splits them into two `Vec`s of the non-compact width (module note).
///
/// `Clone` is derived to support `StructureIdTable` cloning (the interpreter's
/// `CoreObjectStore` snapshot/test path clones the whole structure registry). A
/// derived clone is a faithful deep copy: it preserves the index layout, the
/// insertion-ordered value array, AND the `m_deletedOffsets` recycle stack
/// exactly (which the insertion-order `clone_property_table` rebuild does not),
/// so it is the more faithful analog of `PropertyTable::copy` (PropertyTable.h).
#[derive(Clone)]
pub struct PropertyTable {
    /// `unsigned m_indexSize` (PropertyTable.h:273): index-vector length, a power
    /// of two. `tableCapacity() == m_indexSize >> 1`.
    index_size: u32,
    /// `unsigned m_indexMask` (PropertyTable.h:274) `== m_indexSize - 1`.
    index_mask: u32,
    /// `uintptr_t m_indexVector` (PropertyTable.h:275): the open-addressing index.
    /// Each slot is a 1-based entry index into [`Self::table`], `0 ==`
    /// [`EMPTY_ENTRY_INDEX`], `deletedEntryIndex()` for a tombstoned slot. Always
    /// the non-compact (`uint32_t`) form here (module note).
    index: Vec<u32>,
    /// The value array `tableFromIndexVector(...)` (PropertyTable.h:213). JSC
    /// co-allocates it right after the index vector with `tableCapacity() + 1`
    /// slots -- the `+1` is the trailing iteration/deleted guard (PropertyTable.h:
    /// 158-167). Entries are appended in INSERTION ORDER and addressed 1-based via
    /// the index vector.
    table: Vec<PropertyTableEntry>,
    /// `unsigned m_keyCount` (PropertyTable.h:276): number of live entries.
    key_count: u32,
    /// `unsigned m_deletedCount` (PropertyTable.h:277): number of tombstoned value
    /// slots not yet reclaimed by `rehash`.
    deleted_count: u32,
    /// `std::unique_ptr<Vector<PropertyOffset>> m_deletedOffsets`
    /// (PropertyTable.h:278): LIFO stack of `PropertyOffset`s freed by callers, so
    /// `next_offset` recycles holes instead of growing storage.
    deleted_offsets: Option<Vec<PropertyOffset>>,
}

impl PropertyTable {
    /// `PropertyTable(VM&, unsigned initialCapacity)` (PropertyTable.cpp:60). Sizes
    /// the index for `initialCapacity` and zero-fills both arrays.
    pub fn with_capacity(initial_capacity: u32) -> Self {
        let index_size = Self::size_for_capacity(initial_capacity);
        debug_assert!(index_size.is_power_of_two());
        let table_capacity = (index_size >> 1) as usize;
        Self {
            index_size,
            index_mask: index_size - 1,
            // allocateZeroedIndexVector (PropertyTable.h:548): all EmptyEntryIndex.
            index: vec![EMPTY_ENTRY_INDEX; index_size as usize],
            // tableCapacity() + 1 slots, all default/empty (the +1 is the guard).
            table: vec![PropertyTableEntry::empty(); table_capacity + 1],
            key_count: 0,
            deleted_count: 0,
            deleted_offsets: None,
        }
    }

    /// `static unsigned sizeForCapacity(unsigned capacity)` (PropertyTable.h:589):
    /// rounds up to a power of two. `roundUpToPowerOfTwo` is `std::bit_ceil`
    /// (MathExtras.h:488), mirrored by `next_power_of_two`.
    fn size_for_capacity(capacity: u32) -> u32 {
        if capacity < MINIMUM_TABLE_SIZE / 2 {
            return MINIMUM_TABLE_SIZE;
        }
        (capacity + 1).next_power_of_two() * 2
    }

    /// `unsigned tableCapacity() const { return m_indexSize >> 1; }`
    /// (PropertyTable.h:577).
    fn table_capacity(&self) -> u32 {
        self.index_size >> 1
    }

    /// `unsigned deletedEntryIndex() const { return tableCapacity() + 1; }`
    /// (PropertyTable.h:579): the 1-based index of the guard slot, also the value
    /// stored in the index vector to mark a tombstoned probe slot.
    fn deleted_entry_index(&self) -> u32 {
        self.table_capacity() + 1
    }

    /// `unsigned usedCount() const { return m_keyCount + m_deletedCount; }`
    /// (PropertyTable.h:584): next free value slot is `usedCount()` (0-based).
    fn used_count(&self) -> u32 {
        self.key_count + self.deleted_count
    }

    /// `unsigned size() const` (PropertyTable.h:336): number of live entries.
    pub fn size(&self) -> u32 {
        self.key_count
    }

    /// `bool isEmpty() const` (PropertyTable.h:341).
    pub fn is_empty(&self) -> bool {
        self.key_count == 0
    }

    /// `unsigned propertyStorageSize() const` (PropertyTable.h:438): live entries
    /// plus the offsets parked in the deleted-offset stack.
    pub fn property_storage_size(&self) -> u32 {
        self.size() + self.deleted_offsets.as_ref().map_or(0, |v| v.len() as u32)
    }

    /// `bool canInsert(const ValueType&)` (PropertyTable.h:603). The compact-fit
    /// arm is dropped (non-compact only, module note), leaving the capacity test.
    fn can_insert(&self) -> bool {
        self.used_count() < self.table_capacity()
    }

    /// `WTF::intHash(uint64_t)` (WTF/HashFunctions.h:35): rapidhash "mum" mixer.
    fn int_hash_u64(key: u64) -> u32 {
        const SECRET1: u64 = 0x2d358dccaa6c78a5;
        const SECRET2: u64 = 0x8bb84b93962eacc9;
        let product = (key ^ SECRET1) as u128 * (key ^ SECRET2) as u128;
        let folded = (product as u64) ^ ((product >> 64) as u64);
        folded as u32
    }

    /// `IdentifierRepHash::hash(key)` (Identifier.h:235). DIVERGENCE: JSC returns
    /// the `StringImpl`'s cached `existingSymbolAwareHash()`; here the key is the
    /// `AtomId` handle, so we derive the probe hash from the uid via WTF `intHash`.
    /// Distribution-only difference (module note).
    fn rep_hash(key: AtomId) -> u32 {
        Self::int_hash_u64(key.table_slot() as u64)
    }

    /// `FindResult find(const KeyType&)` / `findImpl` (PropertyTable.h:298-340).
    /// Double-hash probe: stride grows by 1 each step (triangular numbers), masked
    /// to the index size. Stops at the first empty slot (absent) or a key match.
    /// Tombstoned probe slots hold `deletedEntryIndex()`, which points at the guard
    /// (an `Empty` key) and so is skipped just like JSC's deleted sentinel.
    pub fn find(&self, key: AtomId) -> FindResult {
        let index_mask = self.index_mask;
        let hash = Self::rep_hash(key);
        let mut probe_count: u32 = 0;
        let mut index = hash & index_mask;
        loop {
            let entry_index = self.index[index as usize];
            if entry_index == EMPTY_ENTRY_INDEX {
                return FindResult {
                    entry_index,
                    index,
                    offset: INVALID_OFFSET,
                    attributes: 0,
                };
            }
            let entry = &self.table[(entry_index - 1) as usize];
            if entry.raw_key() == EntryKey::Key(key) {
                return FindResult {
                    entry_index,
                    index,
                    offset: entry.offset,
                    attributes: entry.attributes,
                };
            }
            probe_count += 1;
            index = (index + probe_count) & index_mask;
        }
    }

    /// `std::tuple<PropertyOffset, unsigned> get(const KeyType&)`
    /// (PropertyTable.h:344). Returns `(offset, attributes)`, or
    /// `(INVALID_OFFSET, 0)` when absent.
    pub fn get(&self, key: AtomId) -> (PropertyOffset, u32) {
        if self.key_count == 0 {
            return (INVALID_OFFSET, 0);
        }
        let result = self.find(key);
        (result.offset, result.attributes)
    }

    /// `std::tuple<PropertyOffset, unsigned, bool> add(VM&, const ValueType&)`
    /// (PropertyTable.h:357). If the key already exists, returns its
    /// `(offset, attributes, false)`; otherwise inserts and returns
    /// `(offset, attributes, true)`.
    pub fn add(&mut self, entry: PropertyTableEntry) -> (PropertyOffset, u32, bool) {
        let result = self.find(entry.require_key());
        if result.offset != INVALID_OFFSET {
            return (result.offset, result.attributes, false);
        }
        self.add_after_find(entry, result)
    }

    /// `addAfterFind` (PropertyTable.h:373). Grows (rehashes) if the value array is
    /// full, then claims the empty index slot the find stopped at and appends the
    /// entry in insertion order.
    fn add_after_find(
        &mut self,
        entry: PropertyTableEntry,
        mut result: FindResult,
    ) -> (PropertyOffset, u32, bool) {
        // JSC: `entry.key()->ref();` -- no-op for the non-owning `AtomId` handle
        // (module note).
        if !self.can_insert() {
            // JSC passes `canFitInCompact(entry)` as `canStayCompact`; dropped with
            // compact mode (module note), so we pass `true` unconditionally.
            self.rehash(self.key_count + 1, true);
            result = self.find(entry.require_key());
            debug_assert_eq!(result.offset, INVALID_OFFSET);
            debug_assert_eq!(result.entry_index, EMPTY_ENTRY_INDEX);
        }

        let index = result.index;
        let entry_index = self.used_count() + 1;
        self.index[index as usize] = entry_index;
        self.table[(entry_index - 1) as usize] = entry;
        self.key_count += 1;

        (entry.offset, entry.attributes, true)
    }

    /// `std::tuple<PropertyOffset, unsigned> take(VM&, const KeyType&)`
    /// (PropertyTable.h:430). Removes the key if present, returning its prior
    /// `(offset, attributes)`.
    pub fn take(&mut self, key: AtomId) -> (PropertyOffset, u32) {
        let result = self.find(key);
        if result.offset != INVALID_OFFSET {
            self.remove(result.entry_index, result.index);
        }
        (result.offset, result.attributes)
    }

    /// `void remove(VM&, KeyType, unsigned entryIndex, unsigned index)`
    /// (PropertyTable.h:393). Replaces the index slot with the deleted sentinel and
    /// tombstones the value slot; rehashes to reclaim once a quarter of the index
    /// is tombstoned.
    fn remove(&mut self, entry_index: u32, index: u32) {
        self.index[index as usize] = self.deleted_entry_index();
        self.table[(entry_index - 1) as usize].set_deleted();
        // JSC: `key->deref();` -- no-op for the non-owning `AtomId` handle.

        debug_assert!(self.key_count >= 1);
        self.key_count -= 1;
        self.deleted_count += 1;

        if self.deleted_count * 4 >= self.index_size {
            self.rehash(self.key_count, true);
        }
    }

    /// `PropertyOffset updateAttributeIfExists(const KeyType&, unsigned)`
    /// (PropertyTable.h:444). Updates attributes in place if the key exists,
    /// returning its offset (else [`INVALID_OFFSET`]).
    pub fn update_attribute_if_exists(&mut self, key: AtomId, attributes: u32) -> PropertyOffset {
        let result = self.find(key);
        if result.offset == INVALID_OFFSET {
            return INVALID_OFFSET;
        }
        self.table[(result.entry_index - 1) as usize].set_attributes(attributes);
        result.offset
    }

    /// `reinsert` (PropertyTable.h:516). Inserts an entry KNOWN to be absent into a
    /// freshly sized table: probes only for emptiness (no key compare, no
    /// tombstones), then appends in order.
    fn reinsert(&mut self, entry: PropertyTableEntry) {
        debug_assert!(self.can_insert());
        let index_mask = self.index_mask;
        let hash = Self::rep_hash(entry.require_key());
        let mut probe_count: u32 = 0;
        let mut index = hash & index_mask;
        loop {
            let entry_index = self.index[index as usize];
            if entry_index == EMPTY_ENTRY_INDEX {
                break;
            }
            debug_assert_ne!(
                self.table[(entry_index - 1) as usize].raw_key(),
                entry.raw_key(),
                "reinsert must not see a duplicate key"
            );
            probe_count += 1;
            index = (index + probe_count) & index_mask;
        }

        let entry_index = self.used_count() + 1;
        self.index[index as usize] = entry_index;
        self.table[(entry_index - 1) as usize] = entry;
        self.key_count += 1;
    }

    /// `void rehash(VM&, unsigned newCapacity, bool canStayCompact)`
    /// (PropertyTable.h:534). Reallocates the index + value arrays sized for
    /// `new_capacity` and re-inserts every live entry IN ORDER, dropping
    /// tombstones. `can_stay_compact` is ignored here (non-compact only, module
    /// note).
    fn rehash(&mut self, new_capacity: u32, _can_stay_compact: bool) {
        let old_table = std::mem::take(&mut self.table);
        // Must read usedCount() with the OLD counts, before resetting them.
        let old_used_count = self.used_count() as usize;

        self.index_size = Self::size_for_capacity(new_capacity);
        self.index_mask = self.index_size - 1;
        self.key_count = 0;
        self.deleted_count = 0;

        let table_capacity = (self.index_size >> 1) as usize;
        self.index = vec![EMPTY_ENTRY_INDEX; self.index_size as usize];
        self.table = vec![PropertyTableEntry::empty(); table_capacity + 1];

        for entry in old_table.into_iter().take(old_used_count) {
            if entry.raw_key() == EntryKey::Deleted {
                continue;
            }
            self.reinsert(entry);
        }
    }

    // --- m_deletedOffsets reuse stack (PropertyTable.h:447-475) ---

    /// `void clearDeletedOffsets()` (PropertyTable.h:447).
    pub fn clear_deleted_offsets(&mut self) {
        self.deleted_offsets = None;
    }

    /// `bool hasDeletedOffset()` (PropertyTable.h:452).
    pub fn has_deleted_offset(&self) -> bool {
        self.deleted_offsets.as_ref().is_some_and(|v| !v.is_empty())
    }

    /// `PropertyOffset takeDeletedOffset()` (PropertyTable.h:457): LIFO pop.
    /// Precondition: [`Self::has_deleted_offset`].
    pub fn take_deleted_offset(&mut self) -> PropertyOffset {
        self.deleted_offsets
            .as_mut()
            .expect("takeDeletedOffset with no deleted offsets")
            .pop()
            .expect("takeDeletedOffset with empty deleted offsets")
    }

    /// `void addDeletedOffset(PropertyOffset)` (PropertyTable.h:462).
    pub fn add_deleted_offset(&mut self, offset: PropertyOffset) {
        let offsets = self.deleted_offsets.get_or_insert_with(Vec::new);
        debug_assert!(
            !offsets.contains(&offset),
            "addDeletedOffset must not double-add an offset"
        );
        offsets.push(offset);
    }

    /// `PropertyOffset nextOffset(PropertyOffset inlineCapacity)`
    /// (PropertyTable.h:471): recycle a freed offset if any, else hand out the next
    /// fresh offset for property number `size()`.
    pub fn next_offset(&mut self, inline_capacity: i32) -> PropertyOffset {
        if self.has_deleted_offset() {
            return self.take_deleted_offset();
        }
        offset_for_property_number(self.size() as i32, inline_capacity)
    }

    /// `template<typename Functor> void forEachProperty(const Functor&) const`
    /// (PropertyTable.h:609). Visits live entries in INSERTION ORDER, skipping
    /// tombstones. (JSC threads an `IterationStatus` for early exit; omitted here.)
    pub fn for_each_property<F: FnMut(&PropertyTableEntry)>(&self, mut functor: F) {
        let used = self.used_count() as usize;
        for entry in self.table.iter().take(used) {
            if entry.raw_key() == EntryKey::Deleted {
                continue;
            }
            functor(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(slot: u32) -> AtomId {
        AtomId::from_table_slot(slot)
    }

    // The C++ `sizeForCapacity` formula (PropertyTable.h:589), and the size table
    // in the header comment (PropertyTable.h:75-82: index-size 16 holds capacity 8).
    #[test]
    fn size_for_capacity_matches_header() {
        // capacity < MinimumTableSize/2 (8) -> MinimumTableSize (16).
        assert_eq!(PropertyTable::size_for_capacity(0), 16);
        assert_eq!(PropertyTable::size_for_capacity(7), 16);
        // capacity >= 8 -> roundUpToPowerOfTwo(capacity + 1) * 2.
        assert_eq!(PropertyTable::size_for_capacity(8), 32);
        assert_eq!(PropertyTable::size_for_capacity(15), 32);
        assert_eq!(PropertyTable::size_for_capacity(16), 64);
        assert_eq!(PropertyTable::size_for_capacity(31), 64);
        assert_eq!(PropertyTable::size_for_capacity(32), 128);
        // Default small table: index 16, tableCapacity 8 (header row "16 / 8").
        let t = PropertyTable::with_capacity(0);
        assert_eq!(t.index_size, 16);
        assert_eq!(t.table_capacity(), 8);
        assert_eq!(t.index_mask, 15);
        // Value array carries the trailing guard slot: tableCapacity() + 1.
        assert_eq!(t.table.len(), 9);
    }

    // Basic add/get and the "already present" path (PropertyTable.h:357).
    #[test]
    fn add_then_get_and_duplicate() {
        let mut t = PropertyTable::with_capacity(0);
        assert!(t.is_empty());

        let (off, attrs, inserted) = t.add(PropertyTableEntry::new(atom(1), 10, 0));
        assert!(inserted);
        assert_eq!((off, attrs), (10, 0));
        assert_eq!(t.size(), 1);
        assert!(!t.is_empty());

        let (off, attrs, inserted) = t.add(PropertyTableEntry::new(atom(2), 11, 4));
        assert!(inserted);
        assert_eq!((off, attrs), (11, 4));

        // get returns (offset, attributes).
        assert_eq!(t.get(atom(1)), (10, 0));
        assert_eq!(t.get(atom(2)), (11, 4));
        // Absent key.
        assert_eq!(t.get(atom(999)), (INVALID_OFFSET, 0));

        // Re-adding an existing key does not insert; returns the existing record
        // (the new offset/attributes are ignored, matching JSC).
        let (off, attrs, inserted) = t.add(PropertyTableEntry::new(atom(1), 777, 2));
        assert!(!inserted);
        assert_eq!((off, attrs), (10, 0));
        assert_eq!(t.size(), 2);
        assert_eq!(t.get(atom(1)), (10, 0));
    }

    // INSERTION ORDER is preserved by forEachProperty (PropertyTable.h:609).
    #[test]
    fn for_each_property_preserves_insertion_order() {
        let mut t = PropertyTable::with_capacity(0);
        let slots = [5u32, 2, 9, 1, 7, 3];
        for (i, &s) in slots.iter().enumerate() {
            t.add(PropertyTableEntry::new(atom(s), i as PropertyOffset, 0));
        }
        let seen: Vec<u32> = {
            let mut v = Vec::new();
            t.for_each_property(|e| v.push(e.key().unwrap().table_slot()));
            v
        };
        assert_eq!(seen, slots.to_vec());
    }

    // take/remove: tombstone, then re-find absent; order of survivors preserved;
    // a deleted key can be re-added (PropertyTable.h:393, :430).
    #[test]
    fn take_removes_and_preserves_order_of_survivors() {
        let mut t = PropertyTable::with_capacity(0);
        for s in 1..=5u32 {
            t.add(PropertyTableEntry::new(
                atom(s),
                s as PropertyOffset * 10,
                0,
            ));
        }
        assert_eq!(t.size(), 5);

        // Remove a middle key.
        let (off, attrs) = t.take(atom(3));
        assert_eq!((off, attrs), (30, 0));
        assert_eq!(t.size(), 4);
        assert_eq!(t.deleted_count, 1);
        assert_eq!(t.get(atom(3)), (INVALID_OFFSET, 0));
        // Taking an absent key is a no-op returning invalid.
        assert_eq!(t.take(atom(3)), (INVALID_OFFSET, 0));
        assert_eq!(t.size(), 4);

        // Survivors keep their insertion order (the tombstone is skipped).
        let seen: Vec<u32> = {
            let mut v = Vec::new();
            t.for_each_property(|e| v.push(e.key().unwrap().table_slot()));
            v
        };
        assert_eq!(seen, vec![1, 2, 4, 5]);

        // Re-add the removed key: appended after the survivors (new insertion).
        let (off, _, inserted) = t.add(PropertyTableEntry::new(atom(3), 333, 0));
        assert!(inserted);
        assert_eq!(off, 333);
        let seen: Vec<u32> = {
            let mut v = Vec::new();
            t.for_each_property(|e| v.push(e.key().unwrap().table_slot()));
            v
        };
        assert_eq!(seen, vec![1, 2, 4, 5, 3]);
    }

    // Growth via rehash (PropertyTable.h:534): exceed tableCapacity and confirm the
    // index grows, every key stays findable, and insertion order survives.
    #[test]
    fn grow_rehashes_and_keeps_all_keys_in_order() {
        let mut t = PropertyTable::with_capacity(0);
        assert_eq!(t.index_size, 16); // capacity 8
        let n = 100u32;
        for s in 1..=n {
            let (_, _, inserted) = t.add(PropertyTableEntry::new(atom(s), s as PropertyOffset, 1));
            assert!(inserted);
        }
        assert_eq!(t.size(), n);
        // Index must have grown well past the initial 16 to hold 100 entries.
        assert!(t.index_size >= 256, "index_size = {}", t.index_size);

        // Every key is still findable with its original payload (exercises the
        // double-hash probe across many collisions).
        for s in 1..=n {
            assert_eq!(t.get(atom(s)), (s as PropertyOffset, 1));
        }
        // Insertion order preserved across all the rehashes.
        let seen: Vec<u32> = {
            let mut v = Vec::new();
            t.for_each_property(|e| v.push(e.key().unwrap().table_slot()));
            v
        };
        assert_eq!(seen, (1..=n).collect::<Vec<_>>());
    }

    // Deleting a quarter of the index triggers a reclaiming rehash
    // (PropertyTable.h:393 `m_deletedCount * 4 >= m_indexSize`).
    #[test]
    fn delete_triggers_reclaiming_rehash() {
        let mut t = PropertyTable::with_capacity(0);
        // Fill to a 64-wide index (tableCapacity 32) so the deletion threshold is
        // clear: deleted_count * 4 >= 64 => 16 deletions reclaim.
        for s in 1..=20u32 {
            t.add(PropertyTableEntry::new(atom(s), s as PropertyOffset, 0));
        }
        assert_eq!(t.index_size, 64);

        // Delete 16 keys one at a time; the 16th deletion hits the threshold and
        // rehashes, resetting deleted_count to 0 while preserving the survivors.
        for s in 1..=16u32 {
            t.take(atom(s));
        }
        assert_eq!(t.size(), 4);
        assert_eq!(
            t.deleted_count, 0,
            "reclaiming rehash should zero tombstones"
        );

        // Survivors intact and in order.
        let seen: Vec<u32> = {
            let mut v = Vec::new();
            t.for_each_property(|e| v.push(e.key().unwrap().table_slot()));
            v
        };
        assert_eq!(seen, vec![17, 18, 19, 20]);
        for s in 17..=20u32 {
            assert_eq!(t.get(atom(s)), (s as PropertyOffset, 0));
        }
        for s in 1..=16u32 {
            assert_eq!(t.get(atom(s)), (INVALID_OFFSET, 0));
        }
    }

    // updateAttributeIfExists (PropertyTable.h:444).
    #[test]
    fn update_attribute_if_exists() {
        let mut t = PropertyTable::with_capacity(0);
        t.add(PropertyTableEntry::new(atom(1), 7, 0));
        // Existing key: updates in place and returns the offset.
        let off = t.update_attribute_if_exists(atom(1), 6);
        assert_eq!(off, 7);
        assert_eq!(t.get(atom(1)), (7, 6));
        // Absent key: returns invalidOffset and changes nothing.
        assert_eq!(t.update_attribute_if_exists(atom(2), 4), INVALID_OFFSET);
    }

    // m_deletedOffsets reuse stack + nextOffset (PropertyTable.h:447-475).
    #[test]
    fn deleted_offsets_stack_and_next_offset() {
        let mut t = PropertyTable::with_capacity(0);
        assert!(!t.has_deleted_offset());

        // With no deleted offsets, nextOffset hands out offsetForPropertyNumber(
        // size(), inlineCapacity). size()==0, inlineCapacity 4 -> 0.
        assert_eq!(t.next_offset(4), offset_for_property_number(0, 4));

        // Park two freed offsets; LIFO order on the way out.
        t.add_deleted_offset(40);
        t.add_deleted_offset(41);
        assert!(t.has_deleted_offset());
        // property_storage_size = live (0) + parked (2).
        assert_eq!(t.property_storage_size(), 2);

        assert_eq!(t.next_offset(4), 41);
        assert_eq!(t.next_offset(4), 40);
        assert!(!t.has_deleted_offset());

        // Back to fresh offsets once the stack drains.
        assert_eq!(t.next_offset(4), offset_for_property_number(0, 4));

        // clearDeletedOffsets drops the stack.
        t.add_deleted_offset(99);
        t.clear_deleted_offsets();
        assert!(!t.has_deleted_offset());
    }

    // propertyStorageSize counts live entries plus parked deleted offsets
    // (PropertyTable.h:438).
    #[test]
    fn property_storage_size_counts_live_plus_parked() {
        let mut t = PropertyTable::with_capacity(0);
        t.add(PropertyTableEntry::new(atom(1), 0, 0));
        t.add(PropertyTableEntry::new(atom(2), 1, 0));
        t.add(PropertyTableEntry::new(atom(3), 2, 0));
        assert_eq!(t.property_storage_size(), 3);
        t.add_deleted_offset(50);
        t.add_deleted_offset(51);
        assert_eq!(t.property_storage_size(), 5);
    }

    // Stress: interleave adds and removes, then verify the table agrees with a
    // reference model on membership/offset for every key. Exercises probing past
    // tombstones, growth, and reclaiming rehashes together.
    #[test]
    fn stress_add_remove_matches_reference_model() {
        use std::collections::BTreeMap;
        let mut t = PropertyTable::with_capacity(0);
        let mut model: BTreeMap<u32, PropertyOffset> = BTreeMap::new();

        // Insert 1..=60.
        for s in 1..=60u32 {
            let off = s as PropertyOffset;
            t.add(PropertyTableEntry::new(atom(s), off, 0));
            model.insert(s, off);
        }
        // Remove every 3rd key.
        for s in (3..=60u32).step_by(3) {
            t.take(atom(s));
            model.remove(&s);
        }
        // Re-add every 5th key with a new offset (those still present are no-ops).
        for s in (5..=60u32).step_by(5) {
            let off = 1000 + s as PropertyOffset;
            let (_, _, inserted) = t.add(PropertyTableEntry::new(atom(s), off, 0));
            if inserted {
                model.insert(s, off);
            }
        }

        assert_eq!(t.size(), model.len() as u32);
        for s in 1..=60u32 {
            match model.get(&s) {
                Some(&off) => assert_eq!(t.get(atom(s)), (off, 0), "key {s}"),
                None => assert_eq!(t.get(atom(s)), (INVALID_OFFSET, 0), "key {s}"),
            }
        }
        // Live set matches the model (order is insertion order; set compared here).
        let mut seen: Vec<u32> = Vec::new();
        t.for_each_property(|e| seen.push(e.key().unwrap().table_slot()));
        let mut seen_sorted = seen.clone();
        seen_sorted.sort_unstable();
        assert_eq!(seen_sorted, model.keys().copied().collect::<Vec<_>>());
    }
}
