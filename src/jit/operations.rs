//! `JITOperations` — the runtime-call shims emitted baseline-JIT code calls.
//!
//! Faithful port of `Source/JavaScriptCore/jit/JITOperations.{h,cpp}`: the free
//! `operation*` functions a baseline-JIT slow path emits a C-ABI call to. The
//! op_add slow path mirrors `operationValueAdd`
//! (`EncodedJSValue(JSGlobalObject*, EncodedJSValue, EncodedJSValue)`,
//! JITOperations.h:419 / JITOperations.cpp:4860), which reaches the VM via
//! `globalObject->vm()` and runs `jsAdd`, setting `vm.m_exception` on throw.
//!
//! ## Unsafe boundary (the runtime-call `#![allow(unsafe_code)]` island under
//! `jit/`)
//!
//! `jit/mod.rs` is `#![deny(unsafe_code)]`; this module re-enables it locally
//! (mirroring `jit/unsafe_platform_boundary.rs`). The ONLY `unsafe` here is the
//! per-op pair of reborrows: `&mut *vm` (D1) and `&mut *host` (D5).
//!
//! SAFETY (D1 + D5 — the load-bearing invariant): the `Vm` AND the active
//! `CoreOpcodeDispatchHost` cross the JIT boundary as raw pointers (`*mut Vm` in
//! arg0; `*mut CoreOpcodeDispatchHost` parked on `vm.jit_host`). During JS
//! execution exactly one `&mut Vm` and one `&mut host` are live — the driver's.
//! To call JIT code the driver parks BOTH as raw pointers for the call region
//! (`Vm::set_jit_host` parks the host; the trampoline parks the `*mut Vm`) and
//! does NOT touch either until the call returns; the slow-path shim reborrows
//! EXACTLY ONE `&mut *vm` and EXACTLY ONE `&mut *host` for the operation's
//! duration and drops both before returning to JIT code. Sound under Stacked/Tree
//! Borrows: the driver is BLOCKED inside the call-into-JIT, so its `&mut Vm` and
//! `&mut host` are dormant (never read/written while JIT runs) and neither
//! reborrow aliases a live parent `&mut`. The `Vm` and the host are DISJOINT
//! allocations (the host is not inside the `Vm` — `vm.jit_host` is a raw pointer
//! to a separately-owned object), so the two reborrows do not alias each other
//! either. This is the canonical "pass `&mut` as a raw pointer through a C
//! callback and reborrow once" pattern — the SAME raw-pointer discipline
//! `vm/jsstack.rs` uses for the `Register` arena. Caller obligations:
//!   - `vm` is non-null, aligned, points to a live `Vm` whose `&mut` is parked.
//!   - `vm.jit_host` is non-null (the driver called `set_jit_host`), aligned, and
//!     points to a live `CoreOpcodeDispatchHost` whose `&mut` is parked, disjoint
//!     from the `Vm`.
//!   - No other `&Vm`/`&mut Vm`/`&host`/`&mut host` is dereferenced for the call.
//! Miri-verified on the live reborrows (see the test below — TWO reborrows).
//!
//! D2 (c) — transitional reach mechanism: arg0 is the pre-resolved `*mut Vm` (the
//! `BASELINE_PINNED_VM_REGISTER` value, jit/abi.rs:573), i.e. JSC's
//! `globalObject->vm()` with the one deref ELIDED. The faithful end-state (b)
//! passes `globalObject` in x0 and does `(*global_object).vm()` once the
//! global-object cell carries an `m_vm` back-pointer; this shim's signature then
//! migrates `(*mut Vm, ..)` -> `(*GlobalObject, ..)` mechanically.

#![allow(unsafe_code)]

use crate::interpreter::CoreOpcodeDispatchHost;
use crate::value::{EncodedJsValue, JsValue};
use crate::vm::Vm;

/// `JSValue::encode(JSValue())` — the empty-value bits returned on the throw edge
/// (`ValueEmpty == 0x0`, JSCJSValue.h:483-488). The JIT discards the return
/// register on the exception path and branches on the `m_exception` mirror word
/// instead.
const JS_VALUE_EMPTY_BITS: u64 = 0;

/// `operationValueAdd(JSGlobalObject*, EncodedJSValue, EncodedJSValue)`
/// (JITOperations.cpp:4860). The baseline-JIT op_add slow path emits a C-ABI call
/// to this shim with the two boxed operands; it runs the faithful op_add
/// evaluator and returns the boxed result, or — on throw — stamps the JIT
/// `m_exception` mirror and returns `JSValue::empty()` bits.
///
/// Shape (jit-runtime-bridge.md D3/D4/D5):
/// 1. reborrow `&mut *vm` (D1) and `&mut *vm.jit_host` (D5) — the only `unsafe`;
/// 2. decode the two `EncodedJSValue` operands (`JSValue::decode`, repr.rs:561);
/// 3. call the SAFE split-borrow wrapper `Vm::operation_value_add` (D4) with the
///    REAL host, which runs `arithmetic_binary_result` VERBATIM (UNTOUCHED) on the
///    live stores + the `Vm`'s real heap;
/// 4. `Ok` -> `JSValue::encode(result)` (repr.rs:565);
/// 5. `Err(encoded_exception)` -> write the fixed `m_exception` mirror word (D3)
///    and return `JSValue::empty()` bits.
///
/// `extern "C"` is load-bearing — it is the C-ABI the baseline-JIT far-call
/// expects. The op_add lowering materializes this fn's address
/// (`operation_value_add as usize`) into a scratch and `blr`s it
/// (`MacroAssemblerArm64::far_call`); no exported symbol is needed, so this is
/// deliberately not `#[no_mangle]`.
pub extern "C" fn operation_value_add(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_add)
}

/// The int32 arith FAMILY slow-path shims (`operationValue{Sub,Mul,BitAnd,BitOr,
/// BitXor,LShift,RShift}`, JITOperations.cpp:4978/5225/...). Each mirrors
/// `operationValueAdd` EXACTLY — the only difference is which value-arithmetic
/// evaluator it bridges to — so they share [`dispatch_value_binary_operation`]
/// and add ZERO new `unsafe`: the reborrow island is composed once. The boxed
/// operands arrive already faithfully tagged from each op's int32 fast path; on
/// the slow path the operands are still live in `argumentGPR1`/`argumentGPR2`
/// (the op-arg slots), so the lowering far-calls these with no operand moves.
pub extern "C" fn operation_value_sub(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_sub)
}

pub extern "C" fn operation_value_mul(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_mul)
}

pub extern "C" fn operation_value_bitand(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_bitand)
}

pub extern "C" fn operation_value_bitor(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_bitor)
}

pub extern "C" fn operation_value_bitxor(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_bitxor)
}

pub extern "C" fn operation_value_lshift(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_lshift)
}

pub extern "C" fn operation_value_rshift(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_rshift)
}

/// The SHARED body of every int32 arith FAMILY slow-path shim (op_add's original
/// `operation_value_add` body, now factored so the family cannot drift). `eval`
/// selects the faithful evaluator (`Vm::operation_value_{add,sub,mul,bitand,...}`,
/// each a thin wrapper over the same `arithmetic_binary_result` core).
///
/// Shape (jit-runtime-bridge.md D3/D4/D5):
/// 1. reborrow `&mut *vm` (D1) and `&mut *vm.jit_host` (D5) — the only `unsafe`;
/// 2. decode the two `EncodedJSValue` operands (`JSValue::decode`); the int32 fast
///    paths already boxed faithfully as `NumberTag | uint32(value)`;
/// 3. call `eval` (the SAFE split-borrow wrapper) with the REAL host;
/// 4. `Ok` -> `JSValue::encode(result)`;
/// 5. `Err(encoded_exception)` -> stamp the fixed `m_exception` mirror word (D3)
///    and return `JSValue::empty()` bits (the caller ignores the return register on
///    the throw edge and branches on the exception word instead).
fn dispatch_value_binary_operation(
    vm: *mut Vm,
    op1: u64,
    op2: u64,
    eval: fn(
        &mut Vm,
        &mut CoreOpcodeDispatchHost,
        JsValue,
        JsValue,
    ) -> Result<JsValue, EncodedJsValue>,
) -> u64 {
    // D1 reborrow — see the module SAFETY note. Exactly one `&mut *vm`, dropped
    // before this function returns to JIT code.
    let vm = unsafe { &mut *vm };

    // D5 reborrow: the driver parked the active host on `vm.jit_host` before
    // entering JIT code (`Vm::set_jit_host`). Exactly one `&mut *host` per op,
    // disjoint from `&mut *vm` (the host is a separate allocation), dropped before
    // return. `host` is the REAL store-bearing host, NOT a transient empty one.
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    // JSValue::decode(encodedOp{1,2}).
    let op1 = JsValue::from_encoded(EncodedJsValue(op1));
    let op2 = JsValue::from_encoded(EncodedJsValue(op2));

    match eval(vm, host, op1, op2) {
        // JSValue::encode(result).
        Ok(result) => result.encoded().0,
        // The throw edge: stamp `vm.m_exception` (the JIT mirror word, D3) and
        // hand back the empty value.
        Err(encoded_exception) => {
            vm.set_jit_pending_exception(encoded_exception);
            JS_VALUE_EMPTY_BITS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::VmConfig;

    // Direct exercise of the JIT runtime-call bridge WITHOUT emitting code yet:
    // construct a `Vm` AND a REAL `CoreOpcodeDispatchHost`, park `&mut vm` as a
    // `*mut Vm` and the host on `vm.jit_host`, and call the shim the way emitted
    // slow-path code would. This drives BOTH reborrows (vm + host) and is the Miri
    // target:
    //   MIRIFLAGS="-Zmiri-permissive-provenance -Zmiri-tree-borrows" \
    //     cargo +nightly miri test --lib \
    //     jit::operations::tests::operation_value_add_bridges_real_host_add_concat_throw
    #[test]
    fn operation_value_add_bridges_real_host_add_concat_throw() {
        // A REAL host with live stores. Pre-register two strings so the str+str
        // path PROVES the evaluator reached THIS host's string store (a transient
        // empty host would not find them and would Err instead of concatenating).
        let mut host = CoreOpcodeDispatchHost::new();
        let ab = host.allocate_untracked_string_for_test("ab");
        let cd = host.allocate_untracked_string_for_test("cd");

        let mut vm = Vm::new(VmConfig::interpreter_only());
        // Driver parks the host for the JIT-call region (D5), then the `*mut Vm`.
        vm.set_jit_host(&mut host);
        let vm_ptr: *mut Vm = &mut vm;

        // --- Ok number path: 2 + 3 -> boxed int32 5 (real host, number store
        // untouched but reached). ------------------------------------------------
        let two = JsValue::from_i32(2).encoded().0;
        let three = JsValue::from_i32(3).encoded().0;
        let sum_bits = operation_value_add(vm_ptr, two, three);
        assert_eq!(
            JsValue::from_encoded(EncodedJsValue(sum_bits)),
            JsValue::from_i32(5)
        );
        assert_ne!(sum_bits, JS_VALUE_EMPTY_BITS);

        // --- REAL-STORE proof: "ab" + "cd" -> "abcd" created in the live string
        // store. With the real host, `strings.text(ab).is_some()` routes to
        // `concat_primitives`, which allocates "abcd" in THIS host's store. The
        // result reads back as "abcd" from the same host instance. (A transient
        // host would see `text(ab) == None` and Err — see the throw-path note.) ---
        let concat_bits = operation_value_add(vm_ptr, ab.encoded().0, cd.encoded().0);
        let concat = JsValue::from_encoded(EncodedJsValue(concat_bits));
        assert_ne!(concat_bits, JS_VALUE_EMPTY_BITS, "str+str did not throw");

        // --- Throw path: an `Unknown`-kind operand (0x8) is primitive but ToNumber
        // rejects it, so `arithmetic_binary_result` returns
        // `Err(Fail(ExpectedInt32))`. The shim must return EMPTY bits AND set the
        // m_exception mirror. (0x8: not number/immediate/cell -> ValueKind::Unknown,
        // repr.rs kind(); never VALUE_EMPTY which is 0x0.) -----------------------
        let unknown_operand = 0x8u64;
        let seven = JsValue::from_i32(7).encoded().0;
        let thrown_bits = operation_value_add(vm_ptr, unknown_operand, seven);
        assert_eq!(thrown_bits, JS_VALUE_EMPTY_BITS);
        assert_eq!(
            JsValue::from_encoded(EncodedJsValue(thrown_bits)),
            JsValue::default(),
        );

        // End the parked region; the `&mut Vm`/`&mut host` are dormant until here,
        // so reading them back directly is sound (and `vm_ptr` is not used again).
        vm.clear_jit_host();
        assert_ne!(
            vm.jit_pending_exception().0,
            0,
            "m_exception mirror set on throw"
        );
        // The concatenated result lives in the REAL host's store — definitive
        // proof the evaluator ran on the parked host, not a transient one.
        assert_eq!(host.string_text_for_test(concat), Some("abcd"));
    }
}
