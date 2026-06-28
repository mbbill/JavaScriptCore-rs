//! `CoreBigIntStore` — the live heap-JSBigInt cell store.
//!
//! Phase E B2: extracted verbatim from `interpreter/mod.rs` by pure code-motion
//! (no body changed; only module placement and `pub(crate)` visibility keywords).
//! Faithful TARGET on the C++ side: Source/JavaScriptCore/runtime/JSBigInt.{h,cpp}.

use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct CoreBigIntStore {
    pub(crate) bigints: Vec<Pin<Box<CoreBigIntCell>>>,
    pub(crate) by_value: HashMap<CoreBigIntValue, usize>,
}

// #[repr(C)] pins the header layout so offset_of!(js_type)==4 is stable. js_type is a
// constant (always HeapBigInt) so its participation in the derived Eq/Hash/PartialEq
// is behavior-neutral (the store keys on CoreBigIntValue, not the cell).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[repr(C)]
pub(crate) struct CoreBigIntCell {
    pub(crate) cell_id: CellId,
    // C++ JSC JSCell::m_type (runtime/JSCell.h:298) == HeapBigIntType
    // (runtime/JSType.h:38) for every heap JSBigInt cell; read via
    // JSCell::isHeapBigInt() (runtime/JSCell.h:128). At offset 4 for kind-consistency.
    pub(crate) js_type: JsType,
    pub(crate) value: CoreBigIntValue,
}

// Fixed, kind-consistent JSCell::m_type offset guard (mirrors CoreObjectCell's).
const _: () = assert!(
    std::mem::offset_of!(CoreBigIntCell, js_type) == 4,
    "CoreBigIntCell::js_type must be at offset 4 (fixed kind-consistent JSCell::m_type analog)"
);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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

impl CoreBigIntStore {
    pub(crate) fn allocate(
        &mut self,
        heap: &mut Heap,
        value: CoreBigIntValue,
    ) -> Result<RuntimeValue, ExecutionError> {
        if let Some(index) = self.by_value.get(&value).copied() {
            return Ok(self.value_for_index(index));
        }
        let (bigint, runtime_value) =
            allocate_primitive_interpreter_cell(heap, CellType::BigInt, |cell_id| {
                CoreBigIntCell {
                    cell_id,
                    js_type: JsType::HeapBigInt,
                    value: value.clone(),
                }
            })?;
        let index = self.bigints.len();
        self.bigints.push(bigint);
        self.by_value.insert(value, index);
        Ok(runtime_value)
    }

    pub(crate) fn is_bigint(&self, value: RuntimeValue) -> bool {
        self.value(value).is_some()
    }

    pub(crate) fn value(&self, value: RuntimeValue) -> Option<CoreBigIntValue> {
        let payload = value.as_cell()?.pointer_payload_bits();
        self.bigints
            .iter()
            .find(|bigint| core::ptr::from_ref(bigint.as_ref().get_ref()) as usize == payload)
            .map(|bigint| {
                let bigint = bigint.as_ref().get_ref();
                // Cross-check the in-cell JSCell::m_type against the store gate: a cell
                // owned by the bigint store MUST report HeapBigIntType
                // (runtime/JSCell.h:128). Debug-only.
                debug_assert!(
                    bigint.js_type == JsType::HeapBigInt,
                    "cell owned by CoreBigIntStore must carry JsType::HeapBigInt"
                );
                bigint.value.clone()
            })
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

    pub(crate) fn value_for_index(&self, index: usize) -> RuntimeValue {
        let bigint = self.bigints[index].as_ref().get_ref();
        let ptr = NonNull::from(bigint);
        // SAFETY: The indexed bigint cell is owned by this store and remains
        // pinned while the dispatch host is alive.
        RuntimeValue::from_cell(unsafe { GcRef::from_non_null(ptr) })
    }
}
