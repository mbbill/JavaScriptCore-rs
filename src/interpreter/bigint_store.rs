//! `CoreBigIntStore` — the live heap-JSBigInt cell store.
//!
//! Phase E B2: extracted from `interpreter/mod.rs`. gc-r4-completion U3 (bigint-cell GC):
//! the bigint CELL is now a POD `CoreObjectStore::space` arena cell (identity = arena
//! address, R4a — faithful to JSC allocating JSBigInt in the GC'd `vm.heap.cellSpace`,
//! runtime/JSBigInt.h:63-67 / heap/Heap.h:1131), so it is marked + swept + reclaimed like
//! an object cell. The variable digit payload (sign + limbs) is relocated OUT of the cell
//! into this store's `bigint_records` slab (gc-r4 SD-4 off-cell relocation, the SAME shape
//! the string store applies to its StringImpl payload). The former leaking
//! `Vec<Pin<Box<CoreBigIntCell>>>` is GONE; the arena IS the bigint-cell home.
//!
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/JSBigInt.{h,cpp}. A
//! JSBigInt is `DoesNotNeedDestruction` (it inherits JSCell's default, runtime/JSCell.h:105
//! `static constexpr DestructionMode needsDestruction = DoesNotNeedDestruction;` — JSBigInt.h
//! declares NO override) and declares NO visitChildren (none in JSBigInt.{h,cpp}; the base
//! JSCell visit adds no cell edges), so the cell is a pure GC LEAF with no outgoing edges.

use super::object_store::CoreObjectStore;
use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreBigIntStore {
    // gc-r4-completion U3 (SD-4) — the store-owned slab of out-of-line digit payloads, the
    // home of each bigint cell's variable sign+limbs (the string store's `string_records`
    // analog). A slot is reached from a cell's arena address through `indices_by_payload`;
    // `bigint_record_free_list` recycles a DEAD bigint's slot index, filled by
    // `reconcile_dead_bigint`.
    pub(crate) bigint_records: Vec<BigIntRecord>,
    pub(crate) bigint_record_free_list: Vec<usize>,
    // value -> the canonical bigint CELL's ARENA ADDRESS, and it is WEAK: remove-on-sweep
    // BY IDENTITY in `reconcile_dead_bigint`, the VERBATIM mirror of the string store's
    // `by_text` (whose C++ shape is `~StringImpl -> AtomStringImpl::remove`, WTF/wtf/text/
    // StringImpl.cpp:118-129). A MARKED (live) canonical bigint is never reconciled, so it
    // is retained — never strong-rooted (a strong map would defeat the GC).
    //
    // DIVERGENCE NOTE: C++ JSC does NOT dedup JSBigInt cells (every tryCreateWithLength,
    // runtime/JSBigInt.cpp, mints a fresh cell; `===` compares VALUES via JSBigInt::equals,
    // so two equal heap cells are indistinguishable to JS). The port's pre-existing by-value
    // dedup is therefore unobservable to JS; this unit keeps it in the ratified WEAK shape
    // so it can never root a dead cell.
    pub(crate) by_value: HashMap<CoreBigIntValue, usize>,
    // cell ARENA ADDRESS -> `bigint_records` slot index: the bigint-cell RESOLUTION index
    // (the string store's `indices_by_payload` analog), letting `value(value)` resolve with
    // a store-local map lookup and NO arena deref. The reconcile drops a dead cell's entry.
    pub(crate) indices_by_payload: HashMap<usize, usize>,
}

/// One bigint cell's out-of-line digit payload (gc-r4-completion U3 SD-4), held in the
/// store's `bigint_records` slab: the sign+limbs value, the bound heap `CellId` (the
/// `payload<->cell` bridge id) and the cell's own arena address (slot -> addr, for
/// `value_for_index` + the by-identity intern removal). Freed by `reconcile_dead_bigint`
/// when the cell is swept.
#[derive(Clone, Debug, Default)]
pub(crate) struct BigIntRecord {
    /// The owning bigint cell's arena address (= identity).
    pub(crate) addr: usize,
    pub(crate) value: CoreBigIntValue,
    /// Mirrors `StringRecord::cell_id`; bound eagerly at `allocate` (bigints always
    /// allocate with a `&mut Heap` in hand).
    pub(crate) cell_id: CellId,
}

/// The POD arena BIGINT CELL — the JSBigInt JSCell header.
///
/// DIVERGENCE (permanent, Rust-substrate): C++ JSBigInt stores its digits INLINE, trailing
/// the cell — `allocationSize(length) = offsetOfData() + length * sizeof(Digit)`
/// (runtime/JSBigInt.h:70-72), `dataStorage() = this + offsetOfData()` (JSBigInt.h:629),
/// `m_length` on the cell (JSBigInt.h:635), sign as the per-cell header bit
/// (JSBigInt.h:109-111). The port's arena admits fixed-size POD blobs only
/// (`admit_leaf_cell_blob`; no variable trailing arrays), so the variable digit payload
/// relocates to the store's `bigint_records` slab — the SAME SD-4 off-cell relocation the
/// string store applies to JSString's StringImpl payload. The cell is therefore a pure
/// header with NO edges (JSBigInt has no visitChildren; see the module doc).
///
/// `#[repr(C)]` pins the header layout so `js_type` sits at the kind-consistent offset 4
/// (the fixed `JSCell::m_type` offset every arena cell kind carries — see
/// `arena_cell_kind_at`).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct CoreBigIntCell {
    // C++ JSC JSCell::m_structureID (runtime/JSCell.h, offset 0). JSBigInt uses the VM's
    // shared bigint Structure (JSBigInt::createStructure); the port does not model one, so
    // this is INVALID — the cell is a pure header whose payload lives in `bigint_records`.
    pub(crate) structure_id: StructureId,
    // C++ JSC JSCell::m_type (runtime/JSCell.h:298) == HeapBigIntType
    // (runtime/JSType.h:38) for every heap JSBigInt cell; read via
    // JSCell::isHeapBigInt() (runtime/JSCell.h:128). At the fixed common offset 4 read by
    // the collector's type-dispatch + the U0b isObject gate.
    pub(crate) js_type: JsType,
}

// Fixed, kind-consistent JSCell header offsets (mirrors CoreStringCell's).
const _: () = assert!(
    std::mem::offset_of!(CoreBigIntCell, structure_id) == 0,
    "CoreBigIntCell::structure_id must be at offset 0 (JSCell m_structureID)"
);
const _: () = assert!(
    std::mem::offset_of!(CoreBigIntCell, js_type) == 4,
    "CoreBigIntCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);
// POD: the MarkedBlock sweep runs NO destructor — faithful to JSBigInt's
// DoesNotNeedDestruction (runtime/JSCell.h:105, no override in JSBigInt.h). A Drop field
// would leak (and break the blob copy in `admit_leaf_cell_blob`). The variable digit
// payload lives in the slab, not here.
const _: () = assert!(
    !std::mem::needs_drop::<CoreBigIntCell>(),
    "CoreBigIntCell must be POD (no Drop) for the R4 MarkedBlock sweep + the blob copy"
);

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CoreBigIntValue {
    pub(crate) sign: i8,
    pub(crate) limbs: Vec<u32>,
}

impl CoreBigIntValue {
    const LIMB_BITS: usize = u32::BITS as usize;

    pub(crate) fn zero() -> Self {
        Self {
            sign: 0,
            limbs: Vec::new(),
        }
    }

    pub(crate) fn one() -> Self {
        Self {
            sign: 1,
            limbs: vec![1],
        }
    }

    pub(crate) fn from_i32(value: i32) -> Self {
        Self::from_i128(i128::from(value))
    }

    pub(crate) fn from_i128(value: i128) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let sign = if value < 0 { -1 } else { 1 };
        let mut magnitude = value.unsigned_abs();
        let mut limbs = Vec::new();
        while magnitude != 0 {
            limbs.push(magnitude as u32);
            magnitude >>= Self::LIMB_BITS;
        }
        Self::normalized(sign, limbs)
    }

    pub(crate) fn from_f64_integer(value: f64) -> Option<Self> {
        if !value.is_finite() || value.fract() != 0.0 {
            return None;
        }
        parse_bigint_text(&format!("{value:.0}"), false).ok()
    }

    pub(crate) fn from_digits(
        radix: u32,
        digits: &str,
        negative: bool,
    ) -> Result<Self, ExecutionError> {
        let mut value = Self::zero();
        let mut saw_digit = false;
        for digit in digits.chars() {
            if digit == '_' {
                continue;
            }
            let digit = digit.to_digit(radix).ok_or(ExecutionError::ExpectedInt32)?;
            saw_digit = true;
            value = value.mul_small(radix).add_small(digit);
        }
        if !saw_digit {
            return Err(ExecutionError::ExpectedInt32);
        }
        if negative {
            Ok(value.neg())
        } else {
            Ok(value)
        }
    }

    pub(crate) fn normalized(sign: i8, mut limbs: Vec<u32>) -> Self {
        while limbs.last().copied() == Some(0) {
            limbs.pop();
        }
        if limbs.is_empty() {
            Self::zero()
        } else {
            Self {
                sign: if sign < 0 { -1 } else { 1 },
                limbs,
            }
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.sign == 0
    }

    pub(crate) fn is_negative(&self) -> bool {
        self.sign < 0
    }

    pub(crate) fn abs(&self) -> Self {
        if self.is_zero() {
            Self::zero()
        } else {
            Self {
                sign: 1,
                limbs: self.limbs.clone(),
            }
        }
    }

    pub(crate) fn neg(&self) -> Self {
        if self.is_zero() {
            Self::zero()
        } else {
            Self {
                sign: -self.sign,
                limbs: self.limbs.clone(),
            }
        }
    }

    pub(crate) fn add(&self, other: &Self) -> Self {
        match (self.sign, other.sign) {
            (0, _) => other.clone(),
            (_, 0) => self.clone(),
            (left, right) if left == right => {
                Self::normalized(left, Self::add_abs_limbs(&self.limbs, &other.limbs))
            }
            _ => match self.cmp_abs(other) {
                Ordering::Greater => {
                    Self::normalized(self.sign, Self::sub_abs_limbs(&self.limbs, &other.limbs))
                }
                Ordering::Less => {
                    Self::normalized(other.sign, Self::sub_abs_limbs(&other.limbs, &self.limbs))
                }
                Ordering::Equal => Self::zero(),
            },
        }
    }

    pub(crate) fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub(crate) fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let mut result = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (left_index, left) in self.limbs.iter().copied().enumerate() {
            let mut carry = 0u64;
            for (right_index, right) in other.limbs.iter().copied().enumerate() {
                let index = left_index + right_index;
                let value = u64::from(result[index]) + u64::from(left) * u64::from(right) + carry;
                result[index] = value as u32;
                carry = value >> Self::LIMB_BITS;
            }
            let mut index = left_index + other.limbs.len();
            while carry != 0 {
                if index == result.len() {
                    result.push(0);
                }
                let value = u64::from(result[index]) + carry;
                result[index] = value as u32;
                carry = value >> Self::LIMB_BITS;
                index += 1;
            }
        }
        Self::normalized(self.sign * other.sign, result)
    }

    pub(crate) fn div_rem(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() {
            return None;
        }
        let (quotient, remainder) = Self::div_rem_abs(&self.abs(), &other.abs());
        let quotient = Self::normalized(self.sign * other.sign, quotient.limbs);
        let remainder = Self::normalized(self.sign, remainder.limbs);
        Some((quotient, remainder))
    }

    pub(crate) fn div(&self, other: &Self) -> Option<Self> {
        self.div_rem(other).map(|(quotient, _)| quotient)
    }

    pub(crate) fn rem(&self, other: &Self) -> Option<Self> {
        self.div_rem(other).map(|(_, remainder)| remainder)
    }

    pub(crate) fn pow_u32(&self, mut exponent: u32) -> Self {
        let mut base = self.clone();
        let mut result = Self::one();
        while exponent != 0 {
            if exponent & 1 == 1 {
                result = result.mul(&base);
            }
            exponent >>= 1;
            if exponent != 0 {
                base = base.mul(&base);
            }
        }
        result
    }

    pub(crate) fn bit_not(&self) -> Self {
        let len = self.limbs.len().max(1) + 1;
        let words = self
            .to_twos_complement_words(len)
            .into_iter()
            .map(|word| !word)
            .collect();
        Self::from_twos_complement_words(words)
    }

    pub(crate) fn bitwise_with(&self, other: &Self, op: fn(u32, u32) -> u32) -> Self {
        let len = self.limbs.len().max(other.limbs.len()).max(1) + 1;
        let left = self.to_twos_complement_words(len);
        let right = other.to_twos_complement_words(len);
        let words = left
            .into_iter()
            .zip(right)
            .map(|(left, right)| op(left, right))
            .collect();
        Self::from_twos_complement_words(words)
    }

    pub(crate) fn shift_left_by_bigint(&self, shift: &Self) -> Option<Self> {
        if shift.is_negative() {
            let count = shift.neg().to_usize()?;
            Some(self.shift_right_bits(count))
        } else {
            Some(self.shift_left_bits(shift.to_usize()?))
        }
    }

    pub(crate) fn shift_right_by_bigint(&self, shift: &Self) -> Option<Self> {
        if shift.is_negative() {
            let count = shift.neg().to_usize()?;
            Some(self.shift_left_bits(count))
        } else {
            Some(self.shift_right_bits(shift.to_usize()?))
        }
    }

    pub(crate) fn shift_left_bits(&self, bits: usize) -> Self {
        if self.is_zero() || bits == 0 {
            return self.clone();
        }
        let limb_shift = bits / Self::LIMB_BITS;
        let bit_shift = bits % Self::LIMB_BITS;
        let mut limbs = vec![0; limb_shift];
        let mut carry = 0u64;
        for limb in self.limbs.iter().copied() {
            let value = (u64::from(limb) << bit_shift) | carry;
            limbs.push(value as u32);
            carry = value >> Self::LIMB_BITS;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
        Self::normalized(self.sign, limbs)
    }

    pub(crate) fn shift_right_bits(&self, bits: usize) -> Self {
        if self.is_zero() || bits == 0 {
            return self.clone();
        }
        if self.is_negative() {
            let round = Self::one().shift_left_bits(bits).sub(&Self::one());
            return self.abs().add(&round).shift_right_abs_bits(bits).neg();
        }
        self.shift_right_abs_bits(bits)
    }

    pub(crate) fn shift_right_abs_bits(&self, bits: usize) -> Self {
        if self.is_zero() {
            return Self::zero();
        }
        let limb_shift = bits / Self::LIMB_BITS;
        if limb_shift >= self.limbs.len() {
            return Self::zero();
        }
        let bit_shift = bits % Self::LIMB_BITS;
        let mut limbs = Vec::with_capacity(self.limbs.len() - limb_shift);
        if bit_shift == 0 {
            limbs.extend_from_slice(&self.limbs[limb_shift..]);
        } else {
            let mut carry = 0u32;
            for limb in self.limbs[limb_shift..].iter().rev().copied() {
                let value = (limb >> bit_shift) | carry;
                carry = limb << (Self::LIMB_BITS - bit_shift);
                limbs.push(value);
            }
            limbs.reverse();
        }
        Self::normalized(1, limbs)
    }

    pub(crate) fn cmp_abs(&self, other: &Self) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => self.limbs.iter().rev().cmp(other.limbs.iter().rev()),
            ordering => ordering,
        }
    }

    pub(crate) fn cmp(&self, other: &Self) -> Ordering {
        match self.sign.cmp(&other.sign) {
            Ordering::Equal if self.sign < 0 => other.cmp_abs(self),
            Ordering::Equal => self.cmp_abs(other),
            ordering => ordering,
        }
    }

    pub(crate) fn to_u32(&self) -> Option<u32> {
        if self.sign < 0 || self.limbs.len() > 1 {
            return None;
        }
        Some(self.limbs.first().copied().unwrap_or(0))
    }

    pub(crate) fn to_usize(&self) -> Option<usize> {
        if self.sign < 0 {
            return None;
        }
        let mut value = 0usize;
        for limb in self.limbs.iter().rev().copied() {
            value = value.checked_shl(Self::LIMB_BITS as u32)?;
            value = value.checked_add(limb as usize)?;
        }
        Some(value)
    }

    pub(crate) fn to_f64(&self) -> f64 {
        let mut value = 0.0;
        for limb in self.limbs.iter().rev().copied() {
            value = value * 4_294_967_296.0 + f64::from(limb);
        }
        if self.sign < 0 {
            -value
        } else {
            value
        }
    }

    pub(crate) fn to_decimal_string(&self) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let mut value = self.abs();
        let mut digits = Vec::new();
        while !value.is_zero() {
            let (quotient, remainder) = value.div_rem_small(10);
            digits.push(char::from(b'0' + u8::try_from(remainder).unwrap_or(0)));
            value = quotient;
        }
        if self.is_negative() {
            digits.push('-');
        }
        digits.iter().rev().collect()
    }

    pub(crate) fn bit_len(&self) -> usize {
        let Some(top) = self.limbs.last().copied() else {
            return 0;
        };
        (self.limbs.len() - 1) * Self::LIMB_BITS + (Self::LIMB_BITS - top.leading_zeros() as usize)
    }

    pub(crate) fn bit(&self, bit: usize) -> bool {
        let limb = bit / Self::LIMB_BITS;
        let offset = bit % Self::LIMB_BITS;
        self.limbs
            .get(limb)
            .copied()
            .is_some_and(|word| (word & (1u32 << offset)) != 0)
    }

    pub(crate) fn set_bit(&mut self, bit: usize) {
        let limb = bit / Self::LIMB_BITS;
        let offset = bit % Self::LIMB_BITS;
        if self.limbs.len() <= limb {
            self.limbs.resize(limb + 1, 0);
        }
        self.limbs[limb] |= 1u32 << offset;
        if self.sign == 0 {
            self.sign = 1;
        }
    }

    pub(crate) fn div_rem_abs(numerator: &Self, denominator: &Self) -> (Self, Self) {
        if numerator.cmp_abs(denominator) == Ordering::Less {
            return (Self::zero(), numerator.clone());
        }
        let mut quotient = Self::zero();
        let mut remainder = Self::zero();
        for bit in (0..numerator.bit_len()).rev() {
            remainder = remainder.shift_left_bits(1);
            if numerator.bit(bit) {
                remainder = remainder.add_small(1);
            }
            if remainder.cmp_abs(denominator) != Ordering::Less {
                remainder = remainder.sub_abs(denominator);
                quotient.set_bit(bit);
            }
        }
        (quotient, remainder)
    }

    pub(crate) fn sub_abs(&self, other: &Self) -> Self {
        Self::normalized(1, Self::sub_abs_limbs(&self.limbs, &other.limbs))
    }

    pub(crate) fn add_abs_limbs(left: &[u32], right: &[u32]) -> Vec<u32> {
        let max_len = left.len().max(right.len());
        let mut limbs = Vec::with_capacity(max_len + 1);
        let mut carry = 0u64;
        for index in 0..max_len {
            let value = u64::from(left.get(index).copied().unwrap_or(0))
                + u64::from(right.get(index).copied().unwrap_or(0))
                + carry;
            limbs.push(value as u32);
            carry = value >> Self::LIMB_BITS;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
        limbs
    }

    pub(crate) fn sub_abs_limbs(left: &[u32], right: &[u32]) -> Vec<u32> {
        let mut limbs = Vec::with_capacity(left.len());
        let mut borrow = 0i64;
        for (index, left) in left.iter().copied().enumerate() {
            let value =
                i64::from(left) - i64::from(right.get(index).copied().unwrap_or(0)) - borrow;
            if value < 0 {
                limbs.push((value + (1i64 << Self::LIMB_BITS)) as u32);
                borrow = 1;
            } else {
                limbs.push(value as u32);
                borrow = 0;
            }
        }
        limbs
    }

    pub(crate) fn mul_small(&self, factor: u32) -> Self {
        if self.is_zero() || factor == 0 {
            return Self::zero();
        }
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        let mut carry = 0u64;
        for limb in self.limbs.iter().copied() {
            let value = u64::from(limb) * u64::from(factor) + carry;
            limbs.push(value as u32);
            carry = value >> Self::LIMB_BITS;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
        Self::normalized(self.sign, limbs)
    }

    pub(crate) fn add_small(&self, addend: u32) -> Self {
        if addend == 0 {
            return self.clone();
        }
        self.add(&Self::normalized(1, vec![addend]))
    }

    pub(crate) fn div_rem_small(&self, divisor: u32) -> (Self, u32) {
        let mut limbs = Vec::with_capacity(self.limbs.len());
        let mut remainder = 0u64;
        for limb in self.limbs.iter().rev().copied() {
            let value = (remainder << Self::LIMB_BITS) | u64::from(limb);
            limbs.push((value / u64::from(divisor)) as u32);
            remainder = value % u64::from(divisor);
        }
        limbs.reverse();
        (Self::normalized(1, limbs), remainder as u32)
    }

    pub(crate) fn to_twos_complement_words(&self, len: usize) -> Vec<u32> {
        let mut words = self.limbs.clone();
        words.resize(len, 0);
        if self.sign >= 0 {
            return words;
        }
        for word in &mut words {
            *word = !*word;
        }
        Self::add_one_to_words(&mut words);
        words
    }

    pub(crate) fn from_twos_complement_words(mut words: Vec<u32>) -> Self {
        let negative = words
            .last()
            .copied()
            .is_some_and(|word| (word & 0x8000_0000) != 0);
        if !negative {
            return Self::normalized(1, words);
        }
        for word in &mut words {
            *word = !*word;
        }
        Self::add_one_to_words(&mut words);
        Self::normalized(-1, words)
    }

    pub(crate) fn add_one_to_words(words: &mut Vec<u32>) {
        let mut carry = 1u64;
        for word in words {
            let value = u64::from(*word) + carry;
            *word = value as u32;
            carry = value >> Self::LIMB_BITS;
            if carry == 0 {
                break;
            }
        }
    }
}

/// Build + admit a POD `CoreBigIntCell` into the SHARED arena (`CoreObjectStore::space`) via
/// the leaf-cell admission chokepoint, returning its arena address (= identity). Mirrors
/// `admit_string_cell`; a bigint cell carries no fiber/edge field (no visitChildren).
fn admit_bigint_cell(objects: &mut CoreObjectStore) -> usize {
    let cell = CoreBigIntCell {
        structure_id: StructureId::INVALID,
        js_type: JsType::HeapBigInt,
    };
    let len = core::mem::size_of::<CoreBigIntCell>();
    let src = core::ptr::from_ref(&cell).cast::<u8>();
    // SAFETY: `CoreBigIntCell` is POD (`needs_drop == false` asserted above) and `js_type`
    // sits at the const-asserted common offset; the interpreter store is single-threaded.
    // `admit_leaf_cell_blob` copies the bytes into a fresh arena slot + registers it live,
    // returning the arena address.
    unsafe { objects.admit_leaf_cell_blob(src, len) }
}

/// Rebuild the bigint `RuntimeValue` (identity) from a bigint cell's arena address — the
/// leaf analog of `string_value_for_addr`.
fn bigint_value_for_addr(addr: usize) -> RuntimeValue {
    let ptr = core::ptr::with_exposed_provenance_mut::<CoreBigIntCell>(addr);
    let ptr = NonNull::new(ptr).expect("bigint cell arena address is non-null");
    // SAFETY: `addr` is a live arena bigint cell this store published; `from_cell` reads
    // only the pointer's integer bits (it never dereferences here); no GC moves a cell
    // pre-R4b.
    RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
}

impl CoreBigIntStore {
    /// Allocate a slab record, REUSING a freed slot if one exists (mirrors the string
    /// store's `push_record`). Returns the slot index.
    fn push_record(&mut self, record: BigIntRecord) -> usize {
        if let Some(slot) = self.bigint_record_free_list.pop() {
            self.bigint_records[slot] = record; // drops the empty placeholder
            slot
        } else {
            let slot = self.bigint_records.len();
            self.bigint_records.push(record);
            slot
        }
    }

    pub(crate) fn allocate(
        &mut self,
        objects: &mut CoreObjectStore,
        heap: &mut Heap,
        value: CoreBigIntValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(&addr) = self.by_value.get(&value) {
            let slot = self.indices_by_payload[&addr];
            return self.bind_index_to_heap(heap, slot);
        }
        let addr = admit_bigint_cell(objects);
        // gc-r4 leak-fix C1: report the limb slab's bytes into the GC-trigger counter (see
        // `MarkedSpace::report_extra_memory_allocated`'s DIVERGENCE note — C++ allocates a
        // JSBigInt's digits AS PART of the cell's own variable-sized `tryAllocateCell`
        // (runtime/JSBigInt.cpp:109 / `allocationSize`, runtime/JSBigInt.h:70-72), so they are
        // counted for free by the block/large allocator's `didAllocate`, not through
        // `reportExtraMemoryAllocated`; this off-arena limb slab is the pre-R4 substitute).
        objects
            .space
            .report_extra_memory_allocated(value.limbs.len() * std::mem::size_of::<u32>());
        let slot = self.push_record(BigIntRecord {
            addr,
            value: value.clone(),
            cell_id: CellId::default(),
        });
        self.indices_by_payload.insert(addr, slot);
        self.by_value.insert(value, addr);
        self.bind_index_to_heap(heap, slot)
    }

    /// Bind (or rebind) a bigint cell to the heap `payload<->cell` bridge, mirroring
    /// `CoreStringStore::bind_index_to_heap`: bind the heap `CellId` to the cell's ARENA
    /// ADDRESS and stamp it into the slab record. Returns the bigint value.
    pub(crate) fn bind_index_to_heap(
        &mut self,
        heap: &mut Heap,
        slot: usize,
    ) -> Result<RuntimeValue, ExecutionError> {
        let addr = self.bigint_records[slot].addr;
        let cell_id = if let Some(cell_id) = heap.cell_for_payload(addr) {
            heap.publish_cell(cell_id)?;
            cell_id
        } else {
            let cell_id = allocate_primitive_interpreter_cell_id(
                heap,
                CellType::BigInt,
                std::mem::size_of::<CoreBigIntCell>().max(1),
            )?;
            heap.bind_cell_payload(cell_id, addr)?;
            heap.publish_cell(cell_id)?;
            cell_id
        };
        self.bigint_records[slot].cell_id = cell_id;
        Ok(bigint_value_for_addr(addr))
    }

    /// gc-r4-completion U3 — the LEAF reconcile for ONE dead (unmarked) bigint cell, driven
    /// by the host from `CoreObjectStore::take_reclaimed_leaf_addrs` after a collection
    /// (verbatim mirror of `CoreStringStore::reconcile_dead_string`). Frees the cell's
    /// `bigint_records` slot and WEAK-removes its `by_value` intern entry BY IDENTITY: the
    /// entry is evicted ONLY if it still names THIS dead address (a MARKED canonical bigint
    /// is never reconciled — only unmarked cells reach here). A no-op if `addr` is not one
    /// of this store's cells.
    ///
    /// KNOWN RESIDUAL (bounded, shared with strings — see b73d806): the heap's stale
    /// `payload<->cell`-id entry for `addr` is NOT cleaned here (no `&mut Heap`); a recycled
    /// arena address reuses it via `bind_index_to_heap`'s `cell_for_payload` hit.
    pub(crate) fn reconcile_dead_bigint(&mut self, addr: usize) {
        let Some(slot) = self.indices_by_payload.remove(&addr) else {
            return;
        };
        // Free the slab slot (recycle the index); keep the record long enough to key the
        // by-identity intern removal.
        let record = std::mem::take(&mut self.bigint_records[slot]);
        if self.by_value.get(&record.value).copied() == Some(addr) {
            self.by_value.remove(&record.value);
        }
        self.bigint_record_free_list.push(slot);
    }

    pub(crate) fn is_bigint(&self, value: RuntimeValue) -> bool {
        self.index_for_value(value).is_some()
    }

    pub(crate) fn value(&self, value: RuntimeValue) -> Option<CoreBigIntValue> {
        let slot = self.index_for_value(value)?;
        Some(self.bigint_records[slot].value.clone())
    }

    pub(crate) fn to_string(&self, value: RuntimeValue) -> Option<String> {
        self.value(value).map(|value| value.to_decimal_string())
    }

    pub(crate) fn strict_equals(&self, left: RuntimeValue, right: RuntimeValue) -> Option<bool> {
        match (self.value(left), self.value(right)) {
            (Some(left), Some(right)) => Some(left == right),
            (Some(_), None) | (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    }

    pub(crate) fn index_for_value(&self, value: RuntimeValue) -> Option<usize> {
        let addr = value.as_cell()?.pointer_payload_bits();
        self.indices_by_payload.get(&addr).copied()
    }
}
