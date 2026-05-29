//! Fast non-cryptographic hasher for VM-internal integer-keyed maps.
//!
//! Rust-internal perf primitive with no C++ JSC counterpart.
//!
//! `std`'s default `HashMap`/`HashSet` use `SipHasher13`, a DoS-resistant
//! cryptographic hash chosen to defend against hash-flooding from
//! attacker-controlled keys. Some VM-internal hash containers are keyed solely
//! by integers minted from sequential/base-offset internal counters (e.g.
//! `RootId`, which is a `#[repr(transparent)] u64` produced from heap slots,
//! cell numbers, and fixed bases). Those keys are never attacker-controlled, so
//! SipHash's DoS resistance buys nothing and its per-probe cost dominates hot
//! paths that rebuild/probe such maps on every bytecode.
//!
//! This is an FxHash-style multiply-rotate mix (the same family Firefox and
//! `rustc` use for their internal integer maps). It is intentionally simple and
//! fast, NOT collision-resistant against adversaries — only use it for
//! VM-internal integer-keyed maps that need no DoS resistance. Swapping it in
//! is semantically inert: `HashMap`/`HashSet` membership, `get`/`insert`/
//! `contains` results, and `len` are independent of the `BuildHasher`; only
//! internal bucket placement and iteration order change.

use std::hash::{BuildHasher, Hasher};

/// FxHash mixing constant (golden-ratio-derived odd multiplier).
const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// FxHash-style integer hasher. Cheap multiply-rotate mix; no DoS resistance.
#[derive(Clone, Copy, Default)]
pub struct FxIntHasher {
    hash: u64,
}

impl FxIntHasher {
    #[inline]
    fn add(&mut self, value: u64) {
        // FxHash core mix: rotate the accumulator, fold in the value, multiply
        // by the odd seed constant to diffuse bits across the whole word.
        self.hash = (self.hash.rotate_left(5) ^ value).wrapping_mul(FX_SEED);
    }
}

impl Hasher for FxIntHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    // Integer keys (notably `RootId(u64)`) hash through `write_u64`, so override
    // it directly to take the fast single-word path. The generic `write` byte
    // path below remains correct for any other key type but is not on the hot
    // path here.
    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Generic byte path for completeness: fold 8-byte chunks, then the tail.
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        let tail = chunks.remainder();
        if !tail.is_empty() {
            let mut buf = [0u8; 8];
            buf[..tail.len()].copy_from_slice(tail);
            self.add(u64::from_ne_bytes(buf));
        }
    }
}

/// `BuildHasher` for [`FxIntHasher`]. `Default` so `HashMap`/`HashSet` keyed
/// with it stay `Default`-constructible (required by structs that derive
/// `Default`).
#[derive(Clone, Copy, Default)]
pub struct FxIntBuildHasher;

impl BuildHasher for FxIntBuildHasher {
    type Hasher = FxIntHasher;

    #[inline]
    fn build_hasher(&self) -> FxIntHasher {
        FxIntHasher::default()
    }
}
