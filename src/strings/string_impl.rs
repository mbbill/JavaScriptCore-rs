//! Faithful port of `WTF::StringImpl` (WTF/wtf/text/StringImpl.h:149-320).
//!
//! This is the storage-backed, immutable, reference-counted string buffer that
//! the rest of JavaScriptCore builds every `JSString`, `AtomString`, and
//! `Identifier` on top of. It supersedes the storage-less `FlatString` metadata
//! skeleton in `string.rs` (which only described where bytes live, never owned
//! them). Stage A delivers the real buffer + accessors; it is intentionally NOT
//! yet wired into the interpreter/runtime.
//!
//! # Ownership mapping / divergence (sourced)
//!
//! C++ JSC keeps `StringImpl` OFF the GC heap. It is an *intrusively, atomically
//! reference-counted* object: `StringImplShape::m_refCount` is a
//! `std::atomic<uint32_t>` packed into the object header, with the low bit
//! reserved for the static-string flag and `s_refCountIncrement == 0x2` so
//! ref/deref never disturbs that flag (StringImpl.h:163, 581-582). It is *not* a
//! `JSCell` and is never traced by the collector.
//!
//! Rust mapping: `Rc<StringImpl>`. `Rc` reproduces the load-bearing semantics
//! the engine relies on — an off-GC-heap, shared, refcounted, deeply immutable
//! buffer whose character storage is freed when the last reference drops.
//!
//! Permanent divergences from the C++ layout, by design for Rust safety:
//!   * `Rc` keeps its strong count in a separate header word and is
//!     single-threaded; JSC's count is an intrusive atomic so a string can be
//!     `isolatedCopy()`-handed across threads. Cross-thread string sharing is
//!     out of scope here, so the non-atomic `Rc` count is faithful to every
//!     single-thread use; a later stage that needs cross-thread strings would
//!     move to `Arc` + isolated copies, matching JSC's atomic discipline.
//!   * JSC's common `create()` path tail-allocates the characters in the SAME
//!     heap block as the header (`BufferInternal`); Rust cannot safely express a
//!     variable-length tail allocation, so the characters live in a separate
//!     owned `Box<[u8]>` / `Box<[u16]>`. The ownership semantics are identical
//!     (this `StringImpl` uniquely owns and frees its character storage), so we
//!     report `BufferOwnership::Internal` to match what `create()` yields; only
//!     the physical single-vs-double allocation differs.

use std::cell::Cell;
use std::rc::Rc;

/// Maximum length in code units. Mirrors `StringImplShape::MaxLength`
/// (StringImpl.h:152): `std::numeric_limits<int32_t>::max()`.
pub const MAX_LENGTH: u32 = i32::MAX as u32;

// Flag layout inside `m_hashAndFlags`. The low `s_flagCount` bits are flags; the
// hash occupies the high bits, shifted left by `s_flagCount` (StringImpl.h:
// 211-227, 351-360). Stage A models the flag bits exactly; the hash bits stay at
// `s_hashZeroValue` (uncomputed) because lazy hashing (RapidHash) is deferred.

/// `StringImpl::s_flagCount` (StringImpl.h:185): 8 low bits reserved for flags.
const FLAG_COUNT: u32 = 8;
/// `StringImpl::s_flagStringKindCount` (StringImpl.h:188).
const FLAG_STRING_KIND_COUNT: u32 = 4;
/// `s_hashFlagStringKindIsAtom = 1u << s_flagStringKindCount` (StringImpl.h:191).
const HASH_FLAG_STRING_KIND_IS_ATOM: u32 = 1 << FLAG_STRING_KIND_COUNT;
/// `s_hashFlagStringKindIsSymbol = 1u << (s_flagStringKindCount + 1)`
/// (StringImpl.h:192).
const HASH_FLAG_STRING_KIND_IS_SYMBOL: u32 = 1 << (FLAG_STRING_KIND_COUNT + 1);
/// `s_hashFlag8BitBuffer = 1u << 2` (StringImpl.h:195).
const HASH_FLAG_8BIT_BUFFER: u32 = 1 << 2;
/// `s_hashMaskBufferOwnership = (1u << 0) | (1u << 1)` (StringImpl.h:196).
const HASH_MASK_BUFFER_OWNERSHIP: u32 = (1 << 0) | (1 << 1);
/// `s_hashZeroValue = 0` (StringImpl.h:190): hash not yet computed.
const HASH_ZERO_VALUE: u32 = 0;

/// `StringImpl::BufferOwnership` (StringImpl.h:208).
///
/// Records who owns the character storage. Encoded in the low two bits of
/// `m_hashAndFlags`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferOwnership {
    /// `BufferInternal = 0`: storage owned by this `StringImpl`. JSC tail-
    /// allocates it; the Rust port owns it via a separate `Box` (see module
    /// divergence note).
    Internal = 0,
    /// `BufferOwned = 1`: a separately allocated buffer adopted by the impl.
    Owned = 1,
    /// `BufferSubstring = 2`: shares another `StringImpl`'s buffer.
    Substring = 2,
    /// `BufferExternal = 3`: buffer owned by an embedder.
    External = 3,
}

/// `StringImpl::StringKind` (StringImpl.h:198-202): the two kind flag bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringKind {
    /// `StringNormal = 0`: non-symbol, non-atom.
    Normal,
    /// `StringAtom = s_hashFlagStringKindIsAtom`.
    Atom,
    /// `StringSymbol = s_hashFlagStringKindIsSymbol`.
    Symbol,
}

/// The character storage union, faithful to the `m_data8`/`m_data16` union in
/// `StringImplShape` (StringImpl.h:158-166). The `s_hashFlag8BitBuffer` flag
/// selects the active arm, so the discriminated `enum` is the safe Rust
/// counterpart to the unsafe C++ union keyed by `is8Bit()`.
///
/// `Latin1` units are Latin-1 (each byte zero-extends to a code unit 0x00-0xFF).
/// `Utf16` units are raw UTF-16 code units, including unpaired surrogates
/// (0xD800-0xDFFF), stored EXACTLY — there is no lossy UTF-8 round-trip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StringData {
    Latin1(Box<[u8]>),
    Utf16(Box<[u16]>),
}

/// `WTF::StringImpl` — immutable, refcounted, off-GC-heap string buffer.
///
/// Field order mirrors `StringImplShape` (StringImpl.h:160-171): length, the
/// data union, then the packed hash+flags word. `m_refCount` is represented by
/// the enclosing `Rc` instead of an in-object field (see module divergence
/// note).
#[derive(Debug)]
pub struct StringImpl {
    /// `m_length` (StringImpl.h:161). Stored explicitly so `length()` is O(1)
    /// independent of the data arm, exactly as JSC keeps a separate length word
    /// beside the data pointer. Always equals the active data slice length.
    length: u32,
    /// `mutable unsigned m_hashAndFlags` (StringImpl.h:171). `Cell` models the
    /// `mutable` qualifier: JSC fills the hash bits lazily on a logically-const
    /// string via `setHash` (StringImpl.h:1156). Stage A never computes the
    /// hash, so the high bits remain `s_hashZeroValue`; the low 8 flag bits are
    /// set once at construction and never mutated here.
    hash_and_flags: Cell<u32>,
    /// `m_data8`/`m_data16` union (StringImpl.h:158-166).
    data: StringData,
}

impl StringImpl {
    /// Builds the packed flag word (hash bits zero) for a freshly created,
    /// non-symbol, non-atom string with the given width and ownership. Mirrors
    /// the `hashAndFlags` argument the C++ constructors pass to
    /// `StringImplShape` (e.g. StringImpl.h:956, 965).
    const fn initial_flags(is_8bit: bool, ownership: BufferOwnership) -> u32 {
        let width_flag = if is_8bit { HASH_FLAG_8BIT_BUFFER } else { 0 };
        // StringNormal == 0, so it contributes nothing.
        HASH_ZERO_VALUE | width_flag | (ownership as u32)
    }

    /// Creates an 8-bit (Latin-1) string by copying `characters`. Mirrors
    /// `StringImpl::create(std::span<const Latin1Character>)` →
    /// `createInternal` (StringImpl.cpp:247-264): copy the bytes into owned
    /// internal storage. Empty input yields the shared 8-bit empty string, just
    /// as `createInternal` returns `*empty()` (StringImpl.cpp:249).
    pub fn from_latin1(characters: &[u8]) -> Rc<StringImpl> {
        assert!(characters.len() as u64 <= MAX_LENGTH as u64);
        Rc::new(StringImpl {
            length: characters.len() as u32,
            hash_and_flags: Cell::new(Self::initial_flags(true, BufferOwnership::Internal)),
            data: StringData::Latin1(characters.to_vec().into_boxed_slice()),
        })
    }

    /// Creates a 16-bit string by copying `characters`. Mirrors
    /// `StringImpl::create(std::span<const char16_t>)` → `createInternal`
    /// (StringImpl.cpp:257-265).
    ///
    /// SURROGATE-EXACT: every UTF-16 code unit — including unpaired surrogates
    /// (0xD800-0xDFFF) — is stored verbatim. There is no UTF-8 normalization,
    /// unlike the old `String::from_utf16_lossy` path that replaced lone
    /// surrogates with U+FFFD.
    ///
    /// Empty input yields the shared 8-bit empty string, matching
    /// `createInternal` returning `*empty()` (which is 8-bit) for empty spans
    /// (StringImpl.cpp:249, StringImpl.h:421).
    pub fn from_utf16(characters: &[u16]) -> Rc<StringImpl> {
        assert!(characters.len() as u64 <= MAX_LENGTH as u64);
        if characters.is_empty() {
            return Self::from_latin1(&[]);
        }
        Rc::new(StringImpl {
            length: characters.len() as u32,
            hash_and_flags: Cell::new(Self::initial_flags(false, BufferOwnership::Internal)),
            data: StringData::Utf16(characters.to_vec().into_boxed_slice()),
        })
    }

    /// Creates an 8-bit string from 16-bit input when every code unit fits in
    /// Latin-1, otherwise keeps it 16-bit. Mirrors
    /// `StringImpl::create8BitIfPossible(std::span<const char16_t>)`
    /// (StringImpl.cpp, `charactersAreAllLatin1` guard).
    ///
    /// SURROGATE-EXACT: a lone surrogate is > 0xFF, so the all-Latin-1 check
    /// fails and the string stays 16-bit with the surrogate preserved. Narrowing
    /// only happens when it is provably lossless.
    pub fn from_utf16_8bit_if_possible(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::from_latin1(&[]);
        }
        if characters.iter().all(|&u| u <= 0xFF) {
            let bytes: Vec<u8> = characters.iter().map(|&u| u as u8).collect();
            return Self::from_latin1(&bytes);
        }
        Self::from_utf16(characters)
    }

    /// `length()` (StringImpl.h:316): O(1), returns `m_length`.
    pub fn length(&self) -> u32 {
        self.length
    }

    /// `isEmpty()` (StringImpl.h:317): `!m_length`.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// `is8Bit()` (StringImpl.h:318): `m_hashAndFlags & s_hashFlag8BitBuffer`.
    pub fn is_8bit(&self) -> bool {
        self.hash_and_flags.get() & HASH_FLAG_8BIT_BUFFER != 0
    }

    /// `isAtom()` (StringImpl.h:330). Always false in Stage A (atomization is a
    /// later stage), kept for layout fidelity.
    pub fn is_atom(&self) -> bool {
        self.hash_and_flags.get() & HASH_FLAG_STRING_KIND_IS_ATOM != 0
    }

    /// `isSymbol()` (StringImpl.h:329). Always false in Stage A.
    pub fn is_symbol(&self) -> bool {
        self.hash_and_flags.get() & HASH_FLAG_STRING_KIND_IS_SYMBOL != 0
    }

    /// `bufferOwnership()` (StringImpl.h:534): the low two flag bits.
    pub fn buffer_ownership(&self) -> BufferOwnership {
        match self.hash_and_flags.get() & HASH_MASK_BUFFER_OWNERSHIP {
            0 => BufferOwnership::Internal,
            1 => BufferOwnership::Owned,
            2 => BufferOwnership::Substring,
            _ => BufferOwnership::External,
        }
    }

    /// `span8()` (StringImpl.h:319). Faithful to JSC's `ASSERT(is8Bit())`: the
    /// 8-bit arm is required, so a wrong-width access panics rather than
    /// reinterpreting bytes.
    pub fn span8(&self) -> &[u8] {
        match &self.data {
            StringData::Latin1(bytes) => bytes,
            StringData::Utf16(_) => panic!("span8() on a 16-bit StringImpl"),
        }
    }

    /// `span16()` (StringImpl.h:320). JSC asserts `!is8Bit() || isEmpty()`; an
    /// empty 8-bit string legitimately yields an empty 16-bit span.
    pub fn span16(&self) -> &[u16] {
        match &self.data {
            StringData::Utf16(units) => units,
            StringData::Latin1(bytes) if bytes.is_empty() => &[],
            StringData::Latin1(_) => panic!("span16() on a non-empty 8-bit StringImpl"),
        }
    }

    /// `at(unsigned)` / `operator[]` (StringImpl.h:453-454, 1203):
    /// `is8Bit() ? span8()[i] : span16()[i]`.
    ///
    /// Returns a `char16_t` code unit. For 8-bit storage the Latin-1 byte is
    /// zero-extended (0x00-0xFF); for 16-bit storage the raw code unit —
    /// including an unpaired surrogate — is returned EXACTLY. O(1); out-of-range
    /// `i` panics, mirroring JSC's debug `ASSERT` bound.
    pub fn at(&self, i: u32) -> u16 {
        let i = i as usize;
        match &self.data {
            StringData::Latin1(bytes) => u16::from(bytes[i]),
            StringData::Utf16(units) => units[i],
        }
    }

    /// `rawHash()` (StringImpl.h:359): the stored hash bits, before masking
    /// flags. Zero until the (deferred) lazy hash is computed.
    pub fn raw_hash(&self) -> u32 {
        self.hash_and_flags.get() >> FLAG_COUNT
    }

    /// `hasHash()` (StringImpl.h:362): `!!rawHash()`.
    pub fn has_hash(&self) -> bool {
        self.raw_hash() != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_basics_match_jsc_accessors() {
        let s = StringImpl::from_latin1(b"hello");
        assert!(s.is_8bit());
        assert!(!s.is_empty());
        assert_eq!(s.length(), 5);
        assert_eq!(s.buffer_ownership(), BufferOwnership::Internal);
        assert_eq!(s.span8(), b"hello");
        assert_eq!(s.at(0), u16::from(b'h'));
        assert_eq!(s.at(4), u16::from(b'o'));
        // Latin-1 byte 0xFF zero-extends to code unit 0x00FF, not a sign-extend.
        let high = StringImpl::from_latin1(&[0xFF]);
        assert_eq!(high.at(0), 0x00FF);
    }

    #[test]
    fn utf16_basics_are_width_correct() {
        // Contains U+00E9 (é) which still fits Latin-1, plus U+4E2D (中) which
        // does not — from_utf16 always keeps the requested 16-bit width.
        let units = [0x0048u16, 0x00E9, 0x4E2D];
        let s = StringImpl::from_utf16(&units);
        assert!(!s.is_8bit());
        assert_eq!(s.length(), 3);
        assert_eq!(s.span16(), &units);
        assert_eq!(s.at(0), 0x0048);
        assert_eq!(s.at(1), 0x00E9);
        assert_eq!(s.at(2), 0x4E2D);
    }

    /// The load-bearing regression test: a lone high surrogate (U+D800) is NOT a
    /// valid scalar value, so the old `String::from_utf16_lossy` path corrupted
    /// it to U+FFFD. A faithful `StringImpl` stores UTF-16 code units verbatim.
    #[test]
    fn lone_surrogate_round_trips_exactly() {
        let s = StringImpl::from_utf16(&[0xD800]);
        assert!(!s.is_8bit());
        assert_eq!(s.length(), 1);
        assert_eq!(s.at(0), 0xD800, "surrogate must survive verbatim");
        assert_eq!(s.span16(), &[0xD800u16]);

        // Demonstrate the divergence the faithful port removes: the lossy UTF-8
        // round-trip replaces the lone surrogate with U+FFFD.
        let lossy = String::from_utf16_lossy(&[0xD800]);
        assert_eq!(lossy.chars().next(), Some('\u{FFFD}'));
        assert_ne!(u32::from(s.at(0)), 0xFFFD);
    }

    #[test]
    fn surrogate_pair_units_preserved_individually() {
        // U+1F600 😀 as a surrogate pair: both halves stored as distinct units.
        let units = [0xD83Du16, 0xDE00];
        let s = StringImpl::from_utf16(&units);
        assert!(!s.is_8bit());
        assert_eq!(s.length(), 2);
        assert_eq!(s.at(0), 0xD83D);
        assert_eq!(s.at(1), 0xDE00);
    }

    #[test]
    fn create_8bit_if_possible_keeps_surrogate_16bit() {
        // All-Latin-1 input narrows to an 8-bit buffer with exact bytes.
        let narrowed = StringImpl::from_utf16_8bit_if_possible(&[0x0041, 0x00FF]);
        assert!(narrowed.is_8bit());
        assert_eq!(narrowed.span8(), &[0x41u8, 0xFF]);

        // A lone surrogate is > 0xFF, so the string must stay 16-bit and exact.
        let kept = StringImpl::from_utf16_8bit_if_possible(&[0x0041, 0xD800]);
        assert!(!kept.is_8bit());
        assert_eq!(kept.at(1), 0xD800);
    }

    #[test]
    fn empty_inputs_yield_8bit_empty_string() {
        let from8 = StringImpl::from_latin1(&[]);
        assert!(from8.is_8bit());
        assert!(from8.is_empty());
        assert_eq!(from8.length(), 0);

        // Empty 16-bit input collapses to the 8-bit empty string, like JSC's
        // createInternal returning *empty().
        let from16 = StringImpl::from_utf16(&[]);
        assert!(from16.is_8bit());
        assert!(from16.is_empty());
        // span16() on an empty 8-bit string is allowed and is empty.
        assert_eq!(from16.span16(), &[] as &[u16]);
    }

    #[test]
    fn fresh_string_has_no_hash_and_normal_kind() {
        let s = StringImpl::from_latin1(b"x");
        // Lazy hashing is deferred to a later stage; the hash slot is zero.
        assert!(!s.has_hash());
        assert_eq!(s.raw_hash(), 0);
        // A plain created string is neither atom nor symbol.
        assert!(!s.is_atom());
        assert!(!s.is_symbol());
    }

    #[test]
    fn refcount_sharing_is_immutable_and_shared() {
        let s = StringImpl::from_latin1(b"shared");
        let alias = Rc::clone(&s);
        assert_eq!(Rc::strong_count(&s), 2);
        // Both handles observe the same immutable buffer.
        assert_eq!(s.span8(), alias.span8());
        drop(alias);
        assert_eq!(Rc::strong_count(&s), 1);
    }
}
