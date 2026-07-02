//! `CoreSymbolStore` — the live JSC Symbol cell store (+ the Symbol registry and
//! well-known symbols).
//!
//! Phase E B3 extracted this from `interpreter/mod.rs`; gc-r4-completion U2 (symbol-cell
//! GC) makes the symbol CELL a POD `CoreObjectStore::space` arena cell (identity = arena
//! address, R4a — faithful to JSC GC'ing Symbol cells in `vm.symbolSpace`,
//! runtime/Symbol.h:46-50), marked + swept + reclaimed like an object cell. The variable
//! payload (description / registry key / heap-binding id) is relocated OUT of the cell
//! into this store's `symbol_records` slab (the off-cell relocation string U1 SD-4 set;
//! C++ Symbol's payload is likewise off-cell: a refcounted `SymbolImpl` reached through
//! `PrivateName m_privateName`, runtime/Symbol.h:90). The former leaking
//! `Vec<Pin<Box<CoreSymbolCell>>>` is GONE; the arena IS the symbol-cell home.
//!
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/Symbol.{h,cpp} +
//! WTF wtf/text/SymbolRegistry.h. ONE Heap, multiple subspaces (HeapUtil.h): the symbol
//! cell shares `CoreObjectStore::space` with object/string cells, distinguished by
//! `js_type` (SymbolType) — the collector type-dispatches by header (U0) and the object
//! deref islands reject leaf cells (U0b `JSCell::isObject()` gate).

use super::object_store::CoreObjectStore;
use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreSymbolStore {
    // gc-r4-completion U2 — the store-owned slab of out-of-line symbol payloads (the
    // `CoreStringStore::string_records` analog; C++ Symbol's off-cell refcounted
    // `SymbolImpl`, runtime/Symbol.h:58 `uid()`). A `symbol_records` SLOT is reached from
    // a cell's arena address through `indices_by_payload`; `symbol_record_free_list`
    // recycles a DEAD symbol's slot index, filled by `reconcile_dead_symbol`.
    pub(crate) symbol_records: Vec<SymbolRecord>,
    pub(crate) symbol_record_free_list: Vec<usize>,
    // cell ARENA ADDRESS -> `symbol_records` slot index: the symbol-cell RESOLUTION index
    // (store-side analog of Symbol's inline `m_privateName` payload pointer, kept
    // store-side like `CoreStringStore::indices_by_payload` so the ~30 `is_symbol`/
    // `description` callers keep `&self`). The reconcile drops a dead cell's entry.
    pub(crate) indices_by_payload: HashMap<usize, usize>,
    // `Symbol.for` registry: key text -> the registered symbol VALUE. STRONG ROOTS
    // (`gather_symbol_roots`) — the faithful projection of JSC's registry semantics:
    // since WebKit d781022ac569 `VM::m_symbolRegistry` (runtime/VM.h:599) holds registered
    // SymbolImpls STRONGLY (wtf/text/SymbolRegistry.h:52,
    // `UncheckedKeyHashSet<RefPtr<StringImpl>>`), so a registered uid is immortal for the
    // VM lifetime; only the 8-byte Symbol CELL is weak (`vm.symbolImplToSymbolMap`,
    // runtime/VM.h:754 WeakGCMap) with `Symbol::create(vm, uid)` re-minting a cell around
    // the immortal uid on demand (runtime/Symbol.cpp:166-174). The port has no SymbolImpl
    // layer (the CELL is the uid), so strong-rooting the registry VALUE is the same
    // lifetime guarantee; the one delta is that JSC could collect and re-mint the cell
    // while the port keeps cell+record alive (a few dozen bytes per registered symbol).
    pub(crate) registry: HashMap<String, RuntimeValue>,
    // Well-known symbols (Symbol.iterator, ...). STRONG ROOTS (`gather_symbol_roots`) —
    // faithful: JSC installs each well-known Symbol cell as a ReadOnly|DontDelete property
    // of the SymbolConstructor (runtime/SymbolConstructor.cpp:64-75), and their uids are
    // VM-owned CommonIdentifiers, so they are strongly reachable for the VM lifetime.
    pub(crate) well_known: HashMap<String, RuntimeValue>,
}

/// One symbol cell's out-of-line payload (gc-r4-completion U2), held in the store's
/// `symbol_records` slab. C++ Symbol reaches its description through the refcounted
/// `SymbolImpl` uid (`m_privateName`, runtime/Symbol.h:90) — an off-cell payload just like
/// this record. Freed by `reconcile_dead_symbol` when the cell is swept (the
/// `Symbol::destroy` -> `~PrivateName` deref analog, runtime/Symbol.cpp:107-110).
#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolRecord {
    /// The owning symbol cell's arena address (= identity); `value_for_index` rebuilds
    /// the `RuntimeValue` from it.
    pub(crate) addr: usize,
    /// `Symbol([description])`'s description (SymbolImpl text; `None` = `Symbol()`).
    pub(crate) description: Option<String>,
    /// `Some(key)` iff this symbol was minted by `Symbol.for(key)` (a
    /// `RegisteredSymbolImpl` in C++, wtf/text/SymbolRegistry.h). Registered symbols are
    /// strongly rooted via `registry`, so a dead cell must carry `None` here.
    pub(crate) registry_key: Option<String>,
    /// The lazily-bound heap `CellId` (the `payload<->cell` bridge id; default ==
    /// unbound). Mirrors `StringRecord::cell_id`.
    pub(crate) cell_id: CellId,
}

/// The POD arena SYMBOL CELL — the Symbol JSCell header. The variable payload lives
/// off-cell in `CoreSymbolStore::symbol_records`, so the cell is a pure 8-byte header
/// with NO outgoing edges (see `trace_leaf_cell`'s Symbol arm for the visitChildren
/// representation delta).
///
/// `#[repr(C)]` pins the header layout so `js_type` sits at the kind-consistent offset 4
/// (the fixed `JSCell::m_type` offset every arena cell kind carries — see
/// `arena_cell_kind_at`).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct CoreSymbolCell {
    // C++ JSC JSCell::m_structureID (runtime/JSCell.h, offset 0). JSC Symbol uses
    // `vm.symbolStructure`; the port does not model a symbol Structure, so this is
    // INVALID — the cell is a pure header whose payload lives in `symbol_records`.
    pub(crate) structure_id: StructureId,
    // C++ JSC JSCell::m_type == SymbolType (runtime/JSCell.h:298 / runtime/JSType.h:40)
    // for every Symbol cell; isSymbol() == (type == SymbolType) (runtime/JSCell.h:129).
    // Read at the fixed common offset 4 by the collector's type-dispatch + U0b's
    // isObject gate.
    pub(crate) js_type: JsType,
}

// Fixed, kind-consistent JSCell header offsets (mirrors CoreStringCell's).
const _: () = assert!(
    std::mem::offset_of!(CoreSymbolCell, structure_id) == 0,
    "CoreSymbolCell::structure_id must be at offset 0 (JSCell m_structureID)"
);
const _: () = assert!(
    std::mem::offset_of!(CoreSymbolCell, js_type) == 4,
    "CoreSymbolCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);
// POD: the MarkedBlock sweep runs NO destructor; a Drop field would leak (and break the
// blob copy in `admit_leaf_cell_blob`). The description/registry-key text lives in the
// slab, not here.
const _: () = assert!(
    !std::mem::needs_drop::<CoreSymbolCell>(),
    "CoreSymbolCell must be POD (no Drop) for the R4 MarkedBlock sweep + the blob copy"
);
const _: () = assert!(
    std::mem::size_of::<CoreSymbolCell>() == 8,
    "CoreSymbolCell is a pure 8-byte JSCell header (no inline payload)"
);

/// Build + admit a POD `CoreSymbolCell` into the SHARED arena (`CoreObjectStore::space`)
/// via the leaf-cell admission chokepoint, returning its arena address (= identity).
fn admit_symbol_cell(objects: &mut CoreObjectStore) -> usize {
    let cell = CoreSymbolCell {
        structure_id: StructureId::INVALID,
        js_type: JsType::Symbol,
    };
    let len = core::mem::size_of::<CoreSymbolCell>();
    let src = core::ptr::from_ref(&cell).cast::<u8>();
    // SAFETY: `CoreSymbolCell` is POD (`needs_drop == false` asserted above) and `js_type`
    // sits at the const-asserted common offset; the interpreter store is single-threaded.
    // `admit_leaf_cell_blob` copies the bytes into a fresh arena slot + registers it live,
    // returning the arena address.
    unsafe { objects.admit_leaf_cell_blob(src, len) }
}

/// Rebuild the symbol `RuntimeValue` (identity) from a symbol cell's arena address — the
/// leaf analog of `CoreObjectStore::allocate_cell`'s `from_cell` tail.
fn symbol_value_for_addr(addr: usize) -> RuntimeValue {
    let ptr = core::ptr::with_exposed_provenance_mut::<CoreSymbolCell>(addr);
    let ptr = NonNull::new(ptr).expect("symbol cell arena address is non-null");
    // SAFETY: `addr` is a live arena symbol cell this store published; `from_cell` reads
    // only the pointer's integer bits (it never dereferences here); no GC moves a cell
    // pre-R4b.
    RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
}

impl CoreSymbolStore {
    /// Allocate a slab record, REUSING a freed slot if one exists. Returns the slot index.
    fn push_record(&mut self, record: SymbolRecord) -> usize {
        if let Some(slot) = self.symbol_record_free_list.pop() {
            self.symbol_records[slot] = record; // drops the empty placeholder
            slot
        } else {
            let slot = self.symbol_records.len();
            self.symbol_records.push(record);
            slot
        }
    }

    /// Admit a fresh symbol cell + slab record; returns the slot index. Every `Symbol()`
    /// mints a FRESH cell (descriptions are never interned — `Symbol('a') !== Symbol('a')`,
    /// runtime/Symbol.cpp:139-152 `createWithDescription` always allocates).
    fn admit_symbol(
        &mut self,
        objects: &mut CoreObjectStore,
        description: Option<String>,
        registry_key: Option<String>,
    ) -> usize {
        let addr = admit_symbol_cell(objects);
        let slot = self.push_record(SymbolRecord {
            addr,
            description,
            registry_key,
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        slot
    }

    pub(crate) fn allocate_untracked(
        &mut self,
        objects: &mut CoreObjectStore,
        description: Option<String>,
    ) -> RuntimeValue {
        let slot = self.admit_symbol(objects, description, None);
        self.value_for_index(slot)
    }

    pub(crate) fn well_known_untracked(
        &mut self,
        objects: &mut CoreObjectStore,
        name: &str,
    ) -> RuntimeValue {
        if let Some(symbol) = self.well_known.get(name).copied() {
            return symbol;
        }
        let symbol = self.allocate_untracked(objects, Some(name.to_owned()));
        self.well_known.insert(name.to_owned(), symbol);
        symbol
    }

    pub(crate) fn allocate(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        description: Option<String>,
    ) -> Result<RuntimeValue, ExecutionError> {
        let slot = self.admit_symbol(objects, description, None);
        self.bind_index_to_heap(heap, slot)
    }

    /// `Symbol.for(key)` (runtime/SymbolConstructor.cpp `symbolConstructorFor` ->
    /// `vm.symbolRegistry().symbolForKey` + `Symbol::create(vm, uid)`): return the
    /// registered symbol for `key`, minting + registering it on first use. See the
    /// `registry` field comment for the strong-rooting projection.
    pub(crate) fn for_key(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        key: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(symbol) = self.registry.get(key).copied() {
            // A registered symbol is strongly rooted (never reconciled), so its
            // resolution entry is always present; rebind the heap bridge like the
            // string interning hit does.
            let addr = symbol
                .as_cell()
                .expect("registry holds symbol cells")
                .pointer_payload_bits();
            let slot = self.indices_by_payload[&addr];
            return self.bind_index_to_heap(heap, slot);
        }
        let slot = self.admit_symbol(objects, Some(key.to_owned()), Some(key.to_owned()));
        let symbol = self.bind_index_to_heap(heap, slot)?;
        self.registry.insert(key.to_owned(), symbol);
        Ok(symbol)
    }

    pub(crate) fn well_known(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        name: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(symbol) = self.well_known.get(name).copied() {
            return Ok(symbol);
        }
        let slot = self.admit_symbol(objects, Some(name.to_owned()), None);
        let symbol = self.bind_index_to_heap(heap, slot)?;
        self.well_known.insert(name.to_owned(), symbol);
        Ok(symbol)
    }

    /// Lazily bind (or rebind) a symbol cell to the heap `payload<->cell` bridge,
    /// mirroring `CoreStringStore::bind_index_to_heap`: bind the heap `CellId` to the
    /// cell's ARENA ADDRESS and stamp it into the slab record. Returns the symbol value.
    pub(crate) fn bind_index_to_heap(
        &mut self,
        heap: &mut Heap,
        slot: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let addr = self.symbol_records[slot].addr;
        let cell_id = if let Some(cell_id) = heap.cell_for_payload(addr) {
            heap.publish_cell(cell_id)?;
            cell_id
        } else {
            let cell_id = allocate_primitive_interpreter_cell_id(
                heap,
                CellType::Symbol,
                std::mem::size_of::<CoreSymbolCell>().max(1),
            )?;
            heap.bind_cell_payload(cell_id, addr)?;
            heap.publish_cell(cell_id)?;
            cell_id
        };
        self.symbol_records[slot].cell_id = cell_id;
        Ok(symbol_value_for_addr(addr))
    }

    /// gc-r4-completion U2 — the LEAF reconcile for ONE dead (unmarked) symbol cell,
    /// driven by the host from `CoreObjectStore::take_reclaimed_leaf_addrs` after a
    /// collection (next to `reconcile_dead_string`; a no-op if `addr` is not one of this
    /// store's cells). Frees the cell's `symbol_records` slot — the `Symbol::destroy` ->
    /// `~PrivateName` payload release analog (runtime/Symbol.cpp:107-110; Symbol is
    /// `NeedsDestruction`, runtime/Symbol.h:44) — and drops the resolution entry.
    ///
    /// Registered (`Symbol.for`) and well-known symbols are STRONG roots
    /// (`gather_symbol_roots`), so they can never reach here; the debug asserts guard
    /// BOTH invariants (a registered/well-known dead symbol would leave a dangling
    /// `registry`/`well_known` value — a rooting-wiring bug).
    ///
    /// Residual (shared with strings, b73d806): symbols bind a `gc::Heap` payload<->cell
    /// id EAGERLY on the heap-bound paths, and this reconcile cleans the slab +
    /// resolution map but not that id table (no `&mut Heap` here) — a dead heap-bound
    /// symbol leaves a stale id entry. Same class as objects/strings; faithful fix later
    /// (lazy binding, or cleaning the id table in the safepoint drain).
    pub(crate) fn reconcile_dead_symbol(&mut self, addr: usize) {
        let Some(slot) = self.indices_by_payload.remove(&addr) else {
            return;
        };
        debug_assert!(
            self.symbol_records[slot].registry_key.is_none(),
            "a registered (Symbol.for) symbol is strongly rooted and must never be swept"
        );
        // Same invariant for the other strong root class: a well-known symbol reaching the
        // dead-reconcile means its root was dropped from the collection's host_roots — a
        // rooting-wiring bug, caught here in debug (O(well_known) scan; ~a dozen entries).
        debug_assert!(
            !self.well_known.values().any(|value| {
                value.as_cell().map(|cell| cell.pointer_payload_bits()) == Some(addr)
            }),
            "a well-known symbol is strongly rooted and must never be swept"
        );
        // Free the slab slot (drop the description/key text + recycle the index).
        let _ = std::mem::take(&mut self.symbol_records[slot]);
        self.symbol_record_free_list.push(slot);
    }

    /// gc-r4-completion U2 — this store's own STRONG GC roots: every well-known symbol +
    /// every `Symbol.for`-registered symbol. The host folds these into the collection's
    /// `host_roots` (the `JSGlobalLexicalEnvironment`-style host-owned channel). See the
    /// `registry` / `well_known` field comments for why both are strong (the faithful
    /// projection of VM::m_symbolRegistry's strong uids + the SymbolConstructor's
    /// ReadOnly well-known properties).
    pub(crate) fn gather_symbol_roots(&self) -> Vec<RuntimeValue> {
        self.well_known
            .values()
            .chain(self.registry.values())
            .copied()
            .collect()
    }

    pub(crate) fn is_symbol(&self, value: RuntimeValue) -> bool {
        self.find(value).is_some()
    }

    pub(crate) fn description(&self, value: RuntimeValue) -> Option<Option<String>> {
        self.find(value).map(|record| record.description.clone())
    }

    pub(crate) fn key_for(&self, value: RuntimeValue) -> Option<String> {
        self.find(value)
            .and_then(|record| record.registry_key.clone())
    }

    pub(crate) fn symbol_to_string(&self, value: RuntimeValue) -> Option<String> {
        let record = self.find(value)?;
        Some(match &record.description {
            Some(description) => format!("Symbol({description})"),
            None => "Symbol()".to_owned(),
        })
    }

    pub(crate) fn index_for_value(&self, value: RuntimeValue) -> Option<usize> {
        let addr = value.as_cell()?.pointer_payload_bits();
        self.indices_by_payload.get(&addr).copied()
    }

    pub(crate) fn value_for_index(&self, slot: usize) -> RuntimeValue {
        let addr = self.symbol_records[slot].addr;
        symbol_value_for_addr(addr)
    }

    /// Resolve a symbol value to its slab record via the store-local resolution map (NO
    /// arena deref) — the store's `isSymbol()` gate + payload access in one.
    pub(crate) fn find(&self, value: RuntimeValue) -> Option<&SymbolRecord> {
        let slot = self.index_for_value(value)?;
        self.symbol_records.get(slot)
    }
}
