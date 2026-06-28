//! `CoreSymbolStore` — the live JSC Symbol cell store (+ the Symbol registry and
//! well-known symbols).
//!
//! Phase E B3: extracted verbatim from `interpreter/mod.rs` by pure code-motion
//! (no body changed; only module placement and `pub(crate)` visibility keywords).
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/Symbol.{h,cpp} +
//! JSSymbol + SymbolRegistry.

use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreSymbolStore {
    pub(crate) symbols: Vec<Pin<Box<CoreSymbolCell>>>,
    pub(crate) registry: HashMap<String, RuntimeValue>,
    pub(crate) well_known: HashMap<String, RuntimeValue>,
}

// #[repr(C)] pins the header layout so offset_of!(js_type)==4 is stable (only the
// field name is accessed today, so fixing the layout is behavior-neutral).
#[derive(Clone, Debug, Default)]
#[repr(C)]
pub(crate) struct CoreSymbolCell {
    pub(crate) cell_id: CellId,
    // C++ JSC JSCell::m_type (runtime/JSCell.h:298) == SymbolType (runtime/JSType.h:40)
    // for every JSC Symbol cell; read via JSCell::isSymbol() (runtime/JSCell.h:129).
    // At offset 4 for kind-consistency.
    pub(crate) js_type: JsType,
    pub(crate) description: Option<String>,
    pub(crate) registry_key: Option<String>,
}

// Fixed, kind-consistent JSCell::m_type offset guard (mirrors CoreObjectCell's).
const _: () = assert!(
    std::mem::offset_of!(CoreSymbolCell, js_type) == 4,
    "CoreSymbolCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);

impl CoreSymbolStore {
    pub(crate) fn allocate_untracked(&mut self, description: Option<String>) -> RuntimeValue {
        let mut symbol = Box::pin(CoreSymbolCell {
            cell_id: CellId::default(),
            js_type: JsType::Symbol,
            description,
            registry_key: None,
        });
        let ptr = NonNull::from(symbol.as_mut().get_mut());
        self.symbols.push(symbol);
        // SAFETY: The host owns the boxed cell for the lifetime of the dispatch
        // run and never moves the allocation after the value is published.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }

    pub(crate) fn well_known_untracked(&mut self, name: &str) -> RuntimeValue {
        if let Some(symbol) = self.well_known.get(name).copied() {
            return symbol;
        }
        let symbol = self.allocate_untracked(Some(name.to_owned()));
        self.well_known.insert(name.to_owned(), symbol);
        symbol
    }

    pub(crate) fn allocate(
        &mut self,
        heap: &mut Heap,
        description: Option<String>,
    ) -> Result<RuntimeValue, ExecutionError> {
        self.allocate_cell(heap, |cell_id| CoreSymbolCell {
            cell_id,
            js_type: JsType::Symbol,
            description,
            registry_key: None,
        })
    }

    pub(crate) fn for_key(
        &mut self,
        heap: &mut Heap,
        key: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(symbol) = self.registry.get(key).copied() {
            return Ok(symbol);
        }
        let symbol = self.allocate_cell(heap, |cell_id| CoreSymbolCell {
            cell_id,
            js_type: JsType::Symbol,
            description: Some(key.to_owned()),
            registry_key: Some(key.to_owned()),
        })?;
        self.registry.insert(key.to_owned(), symbol);
        Ok(symbol)
    }

    pub(crate) fn well_known(
        &mut self,
        heap: &mut Heap,
        name: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(symbol) = self.well_known.get(name).copied() {
            return Ok(symbol);
        }
        let symbol = self.allocate_cell(heap, |cell_id| CoreSymbolCell {
            cell_id,
            js_type: JsType::Symbol,
            description: Some(name.to_owned()),
            registry_key: None,
        })?;
        self.well_known.insert(name.to_owned(), symbol);
        Ok(symbol)
    }

    pub(crate) fn bind_index_to_heap(
        &mut self,
        heap: &mut Heap,
        index: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let symbol = self.symbols[index].as_ref().get_ref();
        let payload = core::ptr::from_ref(symbol) as usize;
        let cell_id = if let Some(cell_id) = heap.cell_for_payload(payload) {
            heap.publish_cell(cell_id)?;
            cell_id
        } else {
            let cell_id = allocate_primitive_interpreter_cell_id(
                heap,
                CellType::Symbol,
                std::mem::size_of::<CoreSymbolCell>().max(1),
            )?;
            heap.bind_cell_payload(cell_id, payload)?;
            heap.publish_cell(cell_id)?;
            cell_id
        };
        self.symbols[index].as_mut().get_mut().cell_id = cell_id;
        Ok(self.value_for_index(index))
    }

    pub(crate) fn is_symbol(&self, value: RuntimeValue) -> bool {
        self.find(value).is_some()
    }

    pub(crate) fn description(&self, value: RuntimeValue) -> Option<Option<String>> {
        self.find(value).map(|symbol| symbol.description.clone())
    }

    pub(crate) fn key_for(&self, value: RuntimeValue) -> Option<String> {
        self.find(value)
            .and_then(|symbol| symbol.registry_key.clone())
    }

    pub(crate) fn symbol_to_string(&self, value: RuntimeValue) -> Option<String> {
        let symbol = self.find(value)?;
        Some(match &symbol.description {
            Some(description) => format!("Symbol({description})"),
            None => "Symbol()".to_owned(),
        })
    }

    pub(crate) fn index_for_value(&self, value: RuntimeValue) -> Option<usize> {
        let payload = value.as_cell()?.pointer_payload_bits();
        self.symbols
            .iter()
            .position(|symbol| core::ptr::from_ref(symbol.as_ref().get_ref()) as usize == payload)
    }

    pub(crate) fn value_for_index(&self, index: usize) -> RuntimeValue {
        let symbol = self.symbols[index].as_ref().get_ref();
        let _ = symbol.cell_id;
        let ptr = NonNull::from(symbol);
        // SAFETY: The indexed symbol cell is owned by this store and remains
        // pinned while the dispatch host is alive.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }

    pub(crate) fn allocate_cell(
        &mut self,
        heap: &mut Heap,
        make_cell: impl FnOnce(CellId) -> CoreSymbolCell,
    ) -> Result<RuntimeValue, ExecutionError> {
        let (symbol, value) =
            allocate_primitive_interpreter_cell(heap, CellType::Symbol, make_cell)?;
        self.symbols.push(symbol);
        Ok(value)
    }

    pub(crate) fn find(&self, value: RuntimeValue) -> Option<&CoreSymbolCell> {
        let payload = value.as_cell()?.pointer_payload_bits();
        self.symbols
            .iter()
            .find(|symbol| {
                let symbol = symbol.as_ref().get_ref();
                let _ = symbol.cell_id;
                core::ptr::from_ref(symbol) as usize == payload
            })
            .map(|symbol| {
                let symbol = symbol.as_ref().get_ref();
                // Cross-check the in-cell JSCell::m_type against the store gate: a cell
                // owned by the symbol store MUST report SymbolType
                // (runtime/JSCell.h:129). Debug-only.
                debug_assert!(
                    symbol.js_type == JsType::Symbol,
                    "cell owned by CoreSymbolStore must carry JsType::Symbol"
                );
                symbol
            })
    }
}
