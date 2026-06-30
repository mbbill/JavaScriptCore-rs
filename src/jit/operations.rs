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

/// `operationValueDiv(JSGlobalObject*, EncodedJSValue, EncodedJSValue)`
/// (JITOperations.cpp). The baseline op_div slow path: a non-number operand (the
/// double fast path's `branchIfNotNumber` guard) far-calls this shim, which runs
/// the faithful `jsNumber(left / right)` evaluator (`arithmetic_binary_result`
/// with `DivNumber`) and returns the boxed result, or stamps `m_exception` and
/// returns `JSValue::empty()` bits on throw. Shares [`dispatch_value_binary_operation`].
pub extern "C" fn operation_value_div(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_binary_operation(vm, op1, op2, Vm::operation_value_div)
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

/// `false`/`true` as the `size_t` 0/1 a baseline relational/branch operation
/// returns (JSC `operationCompareLess` etc. return `size_t`; the emitted slow
/// path consumes the result with `branchTest32(Zero/NonZero, returnValueGPR)`).
const FALSE_RESULT: u64 = 0;
const TRUE_RESULT: u64 = 1;

/// `operationCompareLess(JSGlobalObject*, EncodedJSValue, EncodedJSValue)`
/// (JITOperations.cpp; `jsLess<true>`). The baseline FUSED int32 compare-and-branch
/// slow path (JSC `emitSlow_op_jless`/`op_jnless`, JITArithmetic.cpp:127/359) emits
/// a C-ABI call to this shim with the two boxed operands; it runs the faithful
/// relational evaluator and returns the comparison's boolean as 0/1, or — on throw
/// (e.g. an object operand's `valueOf`, or a `Symbol` operand) — stamps the JIT
/// `m_exception` mirror and returns 0 (the caller branches on the exception word).
pub extern "C" fn operation_compare_less(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_compare_operation(vm, op1, op2, Vm::operation_compare_less)
}

/// `operationCompareLessEq` -> `jsLessEq<true>`. LessEqualInt32 member.
pub extern "C" fn operation_compare_lesseq(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_compare_operation(vm, op1, op2, Vm::operation_compare_lesseq)
}

/// `operationCompareGreater` -> `jsLess<false>` (operands swapped). GreaterThanInt32.
pub extern "C" fn operation_compare_greater(vm: *mut Vm, op1: u64, op2: u64) -> u64 {
    dispatch_value_compare_operation(vm, op1, op2, Vm::operation_compare_greater)
}

/// The SHARED body of every baseline relational slow-path shim, mirroring
/// [`dispatch_value_binary_operation`] but yielding a boolean as 0/1 (JSC's
/// `operationCompare*` return a `size_t`). `eval` selects the faithful relational
/// evaluator (`Vm::operation_compare_{less,lesseq,greater}`, each a thin wrapper
/// over `CoreOpcodeDispatchHost::numeric_compare`).
fn dispatch_value_compare_operation(
    vm: *mut Vm,
    op1: u64,
    op2: u64,
    eval: fn(
        &mut Vm,
        &mut CoreOpcodeDispatchHost,
        JsValue,
        JsValue,
    ) -> Result<bool, EncodedJsValue>,
) -> u64 {
    // D1 + D5 reborrows — see the module SAFETY note. Exactly one `&mut *vm` and
    // one `&mut *host`, both dropped before returning to JIT code.
    let vm = unsafe { &mut *vm };
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    let op1 = JsValue::from_encoded(EncodedJsValue(op1));
    let op2 = JsValue::from_encoded(EncodedJsValue(op2));

    match eval(vm, host, op1, op2) {
        Ok(true) => TRUE_RESULT,
        Ok(false) => FALSE_RESULT,
        Err(encoded_exception) => {
            vm.set_jit_pending_exception(encoded_exception);
            FALSE_RESULT
        }
    }
}

/// `operationConvertJSValueToBoolean(JSGlobalObject*, EncodedJSValue)` analog
/// for the baseline `op_jtrue` slow path (JSC `emitSlow_op_jtrue`,
/// JITOpcodes.cpp): returns the value's truthiness as 0/1. The boolean conversion
/// is INFALLIBLE in this engine (`CoreOpcodeDispatchHost::value_is_truthy` is total
/// over every `JSValue` — strings/symbols/numbers/objects), so there is no throw
/// edge and `m_exception` is never touched.
pub extern "C" fn operation_jtrue(vm: *mut Vm, value: u64) -> u64 {
    dispatch_value_truthy_operation(vm, value, false)
}

/// `op_jfalse` slow path: the truthiness, inverted (jump-if-FALSE jumps when the
/// value is NOT truthy). Returns `!truthy` as 0/1 so the lowering tests the SAME
/// `NonZero -> take the branch` direction the fast path uses.
pub extern "C" fn operation_jfalse(vm: *mut Vm, value: u64) -> u64 {
    dispatch_value_truthy_operation(vm, value, true)
}

/// The SHARED body of the `op_jtrue`/`op_jfalse` slow-path shims. `invert` selects
/// jfalse (`!truthy`) vs jtrue (`truthy`). Only the host reborrow is needed (the
/// truthy evaluator reads no `Vm` state); the `&mut *vm` reborrow is taken solely
/// to reach the parked host pointer (D5) and is dropped immediately.
fn dispatch_value_truthy_operation(vm: *mut Vm, value: u64, invert: bool) -> u64 {
    let vm = unsafe { &mut *vm };
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    let value = JsValue::from_encoded(EncodedJsValue(value));
    let truthy = host.value_is_truthy(value);
    let taken = truthy ^ invert;
    if taken {
        TRUE_RESULT
    } else {
        FALSE_RESULT
    }
}

/// `operationGetByVal(JSGlobalObject*, EncodedJSValue base, EncodedJSValue prop)`
/// (JITOperations.cpp). The baseline `op_get_by_val` slow-call: the Stage-1 emitter
/// (`arm64_baseline/function_emitter.rs`) loads the boxed base/prop into the C-ABI
/// arg slots and far-calls this shim. It reborrows `&mut *vm` (D1) and `&mut *host`
/// (D5) — the only `unsafe`, the SAME reborrow island the arith family composes
/// (see the module SAFETY note) — decodes the operands, runs the faithful get
/// (`Vm::operation_get_by_val`: the typed-array element shortcut + the general
/// funnel), and returns the boxed result, or — on throw — stamps the JIT
/// `m_exception` mirror (D3) and returns `JSValue::empty()` bits.
pub extern "C" fn operation_get_by_val(vm: *mut Vm, base: u64, prop: u64) -> u64 {
    // D1 + D5 reborrows — see the module SAFETY note. Exactly one `&mut *vm` and one
    // `&mut *host`, both dropped before returning to JIT code.
    let vm = unsafe { &mut *vm };
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    let base = JsValue::from_encoded(EncodedJsValue(base));
    let prop = JsValue::from_encoded(EncodedJsValue(prop));

    match vm.operation_get_by_val(host, base, prop) {
        Ok(result) => result.encoded().0,
        Err(encoded_exception) => {
            vm.set_jit_pending_exception(encoded_exception);
            JS_VALUE_EMPTY_BITS
        }
    }
}

/// `operationPutByVal(JSGlobalObject*, EncodedJSValue base, EncodedJSValue prop,
/// EncodedJSValue value)` (JITOperations.cpp). The baseline `op_put_by_val`
/// slow-call: the emitter loads boxed base/prop/value into the C-ABI arg slots and
/// far-calls this shim. Same D1+D5 reborrow island as the get shim; runs the
/// faithful put (`Vm::operation_put_by_val`: the typed-array element shortcut + the
/// general funnel). put_by_val produces no observable value, so on success it
/// returns `undefined` bits (the lowering discards the result register); on throw
/// it stamps `m_exception` (D3) and returns `JSValue::empty()` bits.
pub extern "C" fn operation_put_by_val(vm: *mut Vm, base: u64, prop: u64, value: u64) -> u64 {
    let vm = unsafe { &mut *vm };
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    let base = JsValue::from_encoded(EncodedJsValue(base));
    let prop = JsValue::from_encoded(EncodedJsValue(prop));
    let value = JsValue::from_encoded(EncodedJsValue(value));

    match vm.operation_put_by_val(host, base, prop, value) {
        Ok(result) => result.encoded().0,
        Err(encoded_exception) => {
            vm.set_jit_pending_exception(encoded_exception);
            JS_VALUE_EMPTY_BITS
        }
    }
}

/// The maximum number of explicit call arguments (EXCLUDING `this`) the baseline
/// `op_call` lowering passes to [`operation_call`] in registers. The AAPCS64
/// integer/pointer argument registers are `x0..x7` (8 total); this shim consumes
/// `x0`=vm, `x1`=callee, `x2`=argc, leaving `x3..x7` for up to 5 boxed arguments.
/// A call site with more arguments is DECLINED by the emitter's S4 allowlist gate
/// (`EmitFunctionError::UnsupportedCallArity`) and stays in the interpreter, so the
/// shim is only ever reached with `argc <= MAX_REGISTER_CALL_ARGS`.
///
/// DIVERGENCE (B5-first-cut): JSC builds the callee frame's argument area on the
/// stack and passes a `CallFrame*`, supporting any arity. This first cut passes the
/// already-boxed arguments by value in registers and bounds the arity; the general
/// stack-built outgoing-argument area arrives with the native direct-link (B5-full).
pub(crate) const MAX_REGISTER_CALL_ARGS: usize = 5;

/// `operationVirtualCall`-family analog (JITOperations.cpp / the unlinked
/// `virtualThunk` path): the baseline `op_call` slow-call shim. The emitted lowering
/// (`arm64_baseline/function_emitter.rs::emit_op_call`) reads the boxed callee +
/// each boxed argument cfr-relative from the JIT caller's frame (in the op_call
/// operand order), loads `x1`=callee, `x2`=argc, `x3..x7`=arg0..arg4, sets `x0`=the
/// pinned `*mut Vm`, and far-calls this shim. It reborrows `&mut *vm` (D1) and
/// `&mut *host` (D5) — the SAME reborrow island the arith/get/put families compose
/// (see the module SAFETY note) — and runs the UNLINKED VIRTUAL CALL
/// (`Vm::operation_call` -> `execute_function_value`), returning the boxed result, or
/// — on throw (the callee threw, or a non-callable callee's TypeError) — stamping the
/// JIT `m_exception` mirror (D3) and returning `JSValue::empty()` bits (the caller's
/// post-call exception probe branches on the mirror word).
///
/// `extern "C"` is load-bearing (the C-ABI the far-call expects). `argc` is the
/// op_call argument count EXCLUDING `this`; `this` is `undefined` (op_call's
/// implicit receiver) and is supplied by `Vm::operation_call`, not passed here.
#[allow(clippy::too_many_arguments)]
pub extern "C" fn operation_call(
    vm: *mut Vm,
    callee: u64,
    argc: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> u64 {
    // D1 + D5 reborrows — see the module SAFETY note. Exactly one `&mut *vm` and one
    // `&mut *host`, both dropped before returning to JIT code.
    let vm = unsafe { &mut *vm };
    let host_ptr = vm.jit_host_ptr();
    debug_assert!(
        !host_ptr.is_null(),
        "the driver must park the dispatch host (Vm::set_jit_host) before the \
         JIT-call region; a null host means the slow path ran outside a parked region"
    );
    let host: &mut CoreOpcodeDispatchHost = unsafe { &mut *host_ptr };

    let callee = JsValue::from_encoded(EncodedJsValue(callee));
    // The emitter never emits a far-call to this shim with argc > MAX_REGISTER_CALL_ARGS
    // (the S4 arity gate declines such a CodeBlock). Clamp defensively so a malformed
    // argc can never index past the register slots.
    let argc = (argc as usize).min(MAX_REGISTER_CALL_ARGS);
    let raw_args = [arg0, arg1, arg2, arg3, arg4];
    let mut arguments = [JsValue::undefined(); MAX_REGISTER_CALL_ARGS];
    for (slot, &bits) in arguments.iter_mut().zip(raw_args.iter()).take(argc) {
        *slot = JsValue::from_encoded(EncodedJsValue(bits));
    }

    match vm.operation_call(host, callee, &arguments[..argc]) {
        Ok(result) => result.encoded().0,
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

    // The compare shim (`dispatch_value_compare_operation`) and the truthy shim
    // (`dispatch_value_truthy_operation`) take the SAME D1+D5 reborrow island as the
    // add shim but were previously UNTESTED by Miri. Drive both reborrows directly
    // (no native code) over PRIMITIVE operands — the int32 relational/truthy fast
    // paths the baseline lowering far-calls — so this is a Miri target:
    //   MIRIFLAGS="-Zmiri-permissive-provenance -Zmiri-tree-borrows" \
    //     cargo +nightly miri test --lib \
    //     jit::operations::tests::compare_and_truthy_shims_reborrow_real_vm_and_host
    #[test]
    fn compare_and_truthy_shims_reborrow_real_vm_and_host() {
        let mut host = CoreOpcodeDispatchHost::new();
        let mut vm = Vm::new(VmConfig::interpreter_only());
        // Driver parks the host for the JIT-call region (D5), then the `*mut Vm` (D1).
        // No CodeBlock is parked: the primitive relational path never reaches
        // `relational_to_primitive` (which reads `state.code_block`), so the bridge's
        // placeholder CodeBlock is used — exactly the int32 fast path.
        vm.set_jit_host(&mut host);
        let vm_ptr: *mut Vm = &mut vm;

        let two = JsValue::from_i32(2).encoded().0;
        let three = JsValue::from_i32(3).encoded().0;

        // --- compare shim: each call reborrows `&mut *vm` + `&mut *host`. ---
        assert_eq!(operation_compare_less(vm_ptr, two, three), TRUE_RESULT); // 2 < 3
        assert_eq!(operation_compare_less(vm_ptr, three, two), FALSE_RESULT); // 3 < 2
        assert_eq!(operation_compare_lesseq(vm_ptr, three, three), TRUE_RESULT); // 3 <= 3
        assert_eq!(operation_compare_greater(vm_ptr, three, two), TRUE_RESULT); // 3 > 2

        // --- truthy shim: jtrue returns truthiness, jfalse its inversion. ---
        let zero = JsValue::from_i32(0).encoded().0;
        let one = JsValue::from_i32(1).encoded().0;
        assert_eq!(operation_jtrue(vm_ptr, one), TRUE_RESULT); // 1 is truthy
        assert_eq!(operation_jtrue(vm_ptr, zero), FALSE_RESULT); // 0 is falsy
        assert_eq!(operation_jfalse(vm_ptr, zero), TRUE_RESULT); // jfalse branches on falsy
        assert_eq!(operation_jfalse(vm_ptr, one), FALSE_RESULT);

        // The parked region is dormant until here; reading vm/host back is sound.
        vm.clear_jit_host();
        // No primitive compare/truthy path throws, so the mirror stays clear.
        assert_eq!(
            vm.jit_pending_exception().0,
            0,
            "primitive compare/truthy must not throw"
        );
    }
}
