//! S2 UB DISCRIMINATOR -- EXPECTED to report UB under miri (SB and TB).
//!
//! Reproduces the prior Route B S2 skeleton: a `Pin<Box<Cell>>` co-owns the cell
//! while the cell's machine address is ALSO carried in the value and dereferenced.
//! The Box is a Unique (noalias) owner over the cell bytes (exactly the live
//! `Vec<Pin<Box<CoreObjectCell>>>` store at interpreter/mod.rs:5256, whose
//! find_mut at :12084 mutates THROUGH the Box). Minting a `&mut` through the Box
//! retags the whole allocation and pops/Disables the carried raw pointer's tag.
//!
//! Running this PROVES the discriminator is meaningful: miri MUST flag this, or a
//! clean run of the arena (which eliminates the Box) would prove nothing. This is
//! the bug the S4 arena fixes by making the exposed pointer the SOLE access path.

use std::pin::Pin;

#[repr(C)]
struct Cell {
    structure_id: u32,
    js_type: u32,
    field: u64,
}

fn main() {
    // The S2 skeleton: the cell is owned by a Pin<Box> (the CoreObjectStore).
    let mut boxed: Pin<Box<Cell>> = Box::pin(Cell { structure_id: 1, js_type: 2, field: 42 });

    // Carried machine address, exposed once (so this is NOT a mere provenance-loss
    // bug -- it is the genuine aliasing bug that survives expose_provenance).
    let addr = ((&*boxed as *const Cell) as *const u8).expose_provenance();
    let raw: *const Cell = std::ptr::with_exposed_provenance::<Cell>(addr);

    // Deref #1 through the carried pointer: fine on its own.
    let _r1 = unsafe { (*raw).field };

    // The OWNING Pin<Box> path mutates the SAME cell (property write / find_mut).
    // This &mut retags the whole allocation -> pops/Disables `raw`'s tag.
    let owner: &mut Cell = unsafe { Pin::get_unchecked_mut(boxed.as_mut()) };
    owner.field = 7;

    // Deref #2 through the now-stale carried pointer: UB under BOTH Stacked Borrows
    // and Tree Borrows. miri reports it HERE.
    let r2 = unsafe { (*raw).field };
    println!("s2_ub: should be unreachable cleanly under miri, got {}", r2);
}
