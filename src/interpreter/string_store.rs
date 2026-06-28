//! `CoreStringStore` — the live JSString/StringImpl-backed string-cell store.
//!
//! Phase E B1: extracted verbatim from `interpreter/mod.rs` by pure code-motion
//! (no body changed; only module placement and `pub(crate)` visibility keywords).
//! This populates the interpreter host's `strings` field; the four runtime-class
//! stores were fused into the dispatch host and are being separated back out to
//! restore the interpreter/runtime boundary the module header declares.
//!
//! Faithful TARGET on the C++ side (the StringImpl-swap cutover lands here):
//! Source/JavaScriptCore/runtime/JSString.{h,cpp} + strings/StringImpl.h.

use super::*;
use super::{allocate_primitive_interpreter_cell, allocate_primitive_interpreter_cell_id};

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreStringStore {
    pub(crate) strings: Vec<Pin<Box<CoreStringCell>>>,
    pub(crate) by_text: HashMap<String, usize>,
    pub(crate) indices_by_payload: HashMap<usize, usize>,
}

// #[repr(C)] pins the header layout so offset_of!(js_type)==4 is stable (only the
// field name is accessed today, so fixing the layout is behavior-neutral).
#[derive(Clone, Debug, Default)]
#[repr(C)]
pub(crate) struct CoreStringCell {
    pub(crate) cell_id: CellId,
    // C++ JSC JSCell::m_type (runtime/JSCell.h:298) == StringType (runtime/JSType.h:37)
    // for every JSString cell; read via JSCell::isString() (runtime/JSCell.h:127).
    // Placed at offset 4 (after the 4-byte cell_id) for kind-consistency with the
    // other cell headers; see the CoreObjectCell::js_type comment for the offset-4
    // (not C++ byte-5) divergence rationale.
    pub(crate) js_type: JsType,
    pub(crate) text: CoreStringCellText,
    pub(crate) atom: Option<Identifier>,
}

// Fixed, kind-consistent JSCell::m_type offset guard (mirrors CoreObjectCell's).
const _: () = assert!(
    std::mem::offset_of!(CoreStringCell, js_type) == 4,
    "CoreStringCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);

#[derive(Clone, Debug, Default)]
pub(crate) enum CoreStringCellText {
    #[default]
    Empty,
    Flat(String),
    Substring {
        base: usize,
        start_byte: usize,
        end_byte: usize,
    },
}

const SHARED_SUBSTRING_MIN_CODE_UNITS: usize = 32;

impl CoreStringStore {
    pub(crate) fn allocate_untracked(&mut self, text: &str) -> RuntimeValue {
        if let Some(index) = self.by_text.get(text).copied() {
            return self.value_for_index(index);
        }
        let mut string = Box::pin(CoreStringCell {
            cell_id: CellId::default(),
            js_type: JsType::String,
            text: CoreStringCellText::Flat(text.to_owned()),
            atom: None,
        });
        let ptr = NonNull::from(string.as_mut().get_mut());
        let payload = ptr.as_ptr() as usize;
        let index = self.strings.len();
        self.strings.push(string);
        self.by_text.insert(text.to_owned(), index);
        self.indices_by_payload.insert(payload, index);
        // SAFETY: The host owns the boxed cell for the lifetime of the dispatch
        // run and never moves the allocation after the value is published.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }

    pub(crate) fn allocate_with_heap(
        &mut self,
        heap: &mut Heap,
        text: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(index) = self.by_text.get(text).copied() {
            return self.bind_index_to_heap(heap, index);
        }
        let (string, value) =
            allocate_primitive_interpreter_cell(heap, CellType::String, |cell_id| {
                CoreStringCell {
                    cell_id,
                    js_type: JsType::String,
                    text: CoreStringCellText::Flat(text.to_owned()),
                    atom: None,
                }
            })?;
        let index = self.strings.len();
        let payload = core::ptr::from_ref(string.as_ref().get_ref()) as usize;
        self.strings.push(string);
        self.by_text.insert(text.to_owned(), index);
        self.indices_by_payload.insert(payload, index);
        Ok(value)
    }

    pub(crate) fn allocate_substring_with_heap(
        &mut self,
        heap: &mut Heap,
        base_value: RuntimeValue,
        start: usize,
        end: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let Some(base) = self.index_for_value(base_value) else {
            return self.allocate_with_heap(heap, "");
        };
        let substring = {
            let Some(text) = self.text_for_index(base) else {
                return self.allocate_with_heap(heap, "");
            };
            let length = string_code_unit_len(text);
            let start = start.min(length);
            let end = end.min(length);
            if start >= end {
                return self.allocate_with_heap(heap, "");
            }
            if start == 0 && end == length {
                return self.bind_index_to_heap(heap, base);
            }
            let substring_len = end.saturating_sub(start);
            if substring_len < SHARED_SUBSTRING_MIN_CODE_UNITS {
                return self.allocate_with_heap(heap, &string_slice_code_units(text, start, end));
            }
            let Some(start_byte) = string_byte_index_for_code_unit(text, start) else {
                return self.allocate_with_heap(heap, &string_slice_code_units(text, start, end));
            };
            let Some(end_byte) = string_byte_index_for_code_unit(text, end) else {
                return self.allocate_with_heap(heap, &string_slice_code_units(text, start, end));
            };
            if !text.is_ascii() {
                return self.allocate_with_heap(heap, &string_slice_code_units(text, start, end));
            }
            let (base, start_byte, end_byte) = match &self.strings[base].as_ref().get_ref().text {
                CoreStringCellText::Substring {
                    base,
                    start_byte: base_start,
                    ..
                } => (
                    *base,
                    base_start.saturating_add(start_byte),
                    base_start.saturating_add(end_byte),
                ),
                _ => (base, start_byte, end_byte),
            };
            CoreStringCellText::Substring {
                base,
                start_byte,
                end_byte,
            }
        };
        let (string, value) =
            allocate_primitive_interpreter_cell(heap, CellType::String, |cell_id| {
                CoreStringCell {
                    cell_id,
                    js_type: JsType::String,
                    text: substring,
                    atom: None,
                }
            })?;
        let index = self.strings.len();
        let payload = core::ptr::from_ref(string.as_ref().get_ref()) as usize;
        self.strings.push(string);
        self.indices_by_payload.insert(payload, index);
        Ok(value)
    }

    pub(crate) fn allocate_atom_with_heap(
        &mut self,
        heap: &mut Heap,
        identifier: Identifier,
        text: &str,
    ) -> Result<RuntimeValue, ExecutionError> {
        let value = self.allocate_with_heap(heap, text)?;
        if let Some(index) = self.index_for_value(value) {
            let string = self.strings[index].as_mut().get_mut();
            if string.atom.is_none() {
                string.atom = Some(identifier);
            }
        }
        Ok(value)
    }

    pub(crate) fn bind_index_to_heap(
        &mut self,
        heap: &mut Heap,
        index: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let string = self.strings[index].as_ref().get_ref();
        let payload = core::ptr::from_ref(string) as usize;
        let cell_id = if let Some(cell_id) = heap.cell_for_payload(payload) {
            heap.publish_cell(cell_id)?;
            cell_id
        } else {
            let cell_id = allocate_primitive_interpreter_cell_id(
                heap,
                CellType::String,
                std::mem::size_of::<CoreStringCell>().max(1),
            )?;
            heap.bind_cell_payload(cell_id, payload)?;
            heap.publish_cell(cell_id)?;
            cell_id
        };
        self.strings[index].as_mut().get_mut().cell_id = cell_id;
        Ok(self.value_for_index(index))
    }

    pub(crate) fn strict_equals(&self, left: RuntimeValue, right: RuntimeValue) -> Option<bool> {
        match (self.text(left), self.text(right)) {
            (Some(left), Some(right)) => Some(left == right),
            (Some(_), None) | (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    }

    pub(crate) fn primitive_to_string(&self, value: RuntimeValue) -> Option<String> {
        if let Some(text) = self.text(value) {
            return Some(text.to_owned());
        }
        match value.kind() {
            ValueKind::Undefined => Some("undefined".to_owned()),
            ValueKind::Null => Some("null".to_owned()),
            ValueKind::Boolean => Some(if value.as_bool().unwrap_or(false) {
                "true".to_owned()
            } else {
                "false".to_owned()
            }),
            ValueKind::Int32 | ValueKind::Double => value.as_number().map(number_to_string),
            ValueKind::Cell | ValueKind::Unknown => None,
        }
    }

    pub(crate) fn text(&self, value: RuntimeValue) -> Option<&str> {
        let index = self.index_for_value(value)?;
        self.text_for_index(index)
    }

    pub(crate) fn text_for_index(&self, index: usize) -> Option<&str> {
        match &self.strings.get(index)?.as_ref().get_ref().text {
            CoreStringCellText::Empty => Some(""),
            CoreStringCellText::Flat(text) => Some(text.as_str()),
            CoreStringCellText::Substring {
                base,
                start_byte,
                end_byte,
            } => self.text_for_index(*base)?.get(*start_byte..*end_byte),
        }
    }

    pub(crate) fn atom_identifier(&self, value: RuntimeValue) -> Option<Identifier> {
        let index = self.index_for_value(value)?;
        self.strings[index].as_ref().get_ref().atom
    }

    pub(crate) fn index_for_value(&self, value: RuntimeValue) -> Option<usize> {
        let payload = value.as_cell()?.pointer_payload_bits();
        self.indices_by_payload.get(&payload).copied()
    }

    pub(crate) fn value_for_index(&self, index: usize) -> RuntimeValue {
        let string = self.strings[index].as_ref().get_ref();
        // Cross-check the in-cell JSCell::m_type against the store gate: a cell owned
        // by the string store MUST report StringType (runtime/JSCell.h:127). Debug-only.
        debug_assert!(
            string.js_type == JsType::String,
            "cell owned by CoreStringStore must carry JsType::String"
        );
        let _ = string.cell_id;
        let ptr = NonNull::from(string);
        // SAFETY: The indexed string cell is owned by this store and remains
        // pinned while the dispatch host is alive.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }
}
