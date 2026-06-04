//! C++ `ConservativeRoots`-mapped conservative-root descriptors.

use crate::gc::{CellId, ConservativeRootSpan, OpaqueRootRecord};

/// Validated conservative root equivalent to a C++ `HeapCell*` entry.
///
/// `candidate_address` records the raw machine word that matched heap identity.
/// It is not a precise root registry slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConservativeRootCell {
    pub candidate_address: usize,
    pub cell: CellId,
}

/// Conservative roots found in raw stack or register spans.
///
/// Candidate addresses are untrusted until the heap validates them. Validated
/// cells are the Rust descriptor equivalent of C++ `ConservativeRoots::roots()`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConservativeRoots {
    spans: Vec<ConservativeRootSpan>,
    candidate_addresses: Vec<usize>,
    validated_cells: Vec<ConservativeRootCell>,
    opaque_roots: Vec<OpaqueRootRecord>,
}

impl ConservativeRoots {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_span(&mut self, span: ConservativeRootSpan) {
        self.spans.push(span);
    }

    pub fn add_candidate_address(&mut self, address: usize) {
        self.candidate_addresses.push(address);
    }

    pub fn add_validated_cell(&mut self, root: ConservativeRootCell) {
        self.validated_cells.push(root);
    }

    pub fn add_opaque_root(&mut self, root: OpaqueRootRecord) {
        self.opaque_roots.push(root);
    }

    pub fn extend(&mut self, other: Self) {
        self.spans.extend(other.spans);
        self.candidate_addresses.extend(other.candidate_addresses);
        self.validated_cells.extend(other.validated_cells);
        self.opaque_roots.extend(other.opaque_roots);
    }

    pub fn spans(&self) -> &[ConservativeRootSpan] {
        &self.spans
    }

    pub fn candidate_addresses(&self) -> &[usize] {
        &self.candidate_addresses
    }

    pub fn validated_cells(&self) -> &[ConservativeRootCell] {
        &self.validated_cells
    }

    pub fn roots(&self) -> &[ConservativeRootCell] {
        self.validated_cells()
    }

    pub fn size(&self) -> usize {
        self.validated_cells.len()
    }

    pub fn opaque_roots(&self) -> &[OpaqueRootRecord] {
        &self.opaque_roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        AllocationMode, CellId, CellMetadata, CellZapReason, ConservativeRootSource, Heap,
        HeapAllocationRequest, HeapCellInvalidationRequest, RootPlanStep,
    };

    fn allocate_test_cell(heap: &mut Heap) -> CellId {
        heap.allocate_record(HeapAllocationRequest {
            heap: heap.id(),
            subspace: "object",
            metadata: CellMetadata::default(),
            byte_size: 64,
            mode: AllocationMode::Normal,
            may_trigger_collection: false,
        })
        .map(|response| response.cell)
        .expect("test allocation")
    }

    #[test]
    fn conservative_root_candidate_validation_requires_exact_published_live_payload() {
        let mut heap = Heap::new();
        let published = allocate_test_cell(&mut heap);
        let unpublished = allocate_test_cell(&mut heap);
        let invalidated = allocate_test_cell(&mut heap);
        let published_payload = 0x1000;
        let unpublished_payload = 0x2000;
        let invalidated_payload = 0x3000;

        heap.bind_cell_payload(published, published_payload)
            .expect("bind published");
        heap.publish_cell(published).expect("publish cell");
        heap.bind_cell_payload(unpublished, unpublished_payload)
            .expect("bind unpublished");
        heap.bind_cell_payload(invalidated, invalidated_payload)
            .expect("bind invalidated");
        heap.publish_cell(invalidated).expect("publish invalidated");
        heap.invalidate_cell(HeapCellInvalidationRequest {
            cell: invalidated,
            reason: CellZapReason::Destruction,
        })
        .expect("invalidate cell");

        assert_eq!(
            heap.validate_conservative_root_candidate_exact_payload(published_payload),
            Some(ConservativeRootCell {
                candidate_address: published_payload,
                cell: published
            })
        );
        assert_eq!(
            heap.validate_conservative_root_candidate_exact_payload(0),
            None
        );
        assert_eq!(
            heap.validate_conservative_root_candidate_exact_payload(0x4000),
            None
        );
        assert_eq!(
            heap.validate_conservative_root_candidate_exact_payload(unpublished_payload),
            None
        );
        assert_eq!(
            heap.validate_conservative_root_candidate_exact_payload(invalidated_payload),
            None
        );
    }

    #[test]
    fn heap_ingests_validated_conservative_cells_as_distinct_plan_steps() {
        let mut heap = Heap::new();
        let cell = allocate_test_cell(&mut heap);
        let payload = 0x1000;
        heap.bind_cell_payload(cell, payload).expect("bind payload");
        heap.publish_cell(cell).expect("publish cell");

        let mut roots = ConservativeRoots::new();
        roots.add_validated_cell(
            heap.validate_conservative_root_candidate_exact_payload(payload)
                .expect("validated root"),
        );

        assert_eq!(heap.ingest_conservative_roots(roots), Ok(()));
        assert_eq!(
            heap.root_marking_plan().planned_steps(),
            Ok(vec![RootPlanStep::ConservativeCell {
                root: ConservativeRootCell {
                    candidate_address: payload,
                    cell
                },
                source: ConservativeRootSource::MachineStack
            }])
        );
    }
}
