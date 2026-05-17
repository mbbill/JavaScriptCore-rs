/// First constant register in JSC's wide32 virtual-register namespace.
pub const FIRST_CONSTANT_REGISTER_INDEX: i32 = 0x4000_0000;

/// JSC's invalid virtual register sentinel.
pub const INVALID_VIRTUAL_REGISTER: i32 = 0x3fff_ffff;

/// Register index for the `this` argument in the call-frame namespace.
///
/// The exact value is owned by `CallFrameSlot::thisArgument` in C++. The Rust
/// skeleton names the dependency without baking a final ABI value into bytecode
/// generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ThisArgumentOffset(pub i32);

/// JSC virtual-register encoding contract.
///
/// Locals, call-frame header slots, arguments, and constants share one signed
/// namespace. Negative values are locals. Non-negative values below
/// `FIRST_CONSTANT_REGISTER_INDEX` are call-frame header/argument slots. Values
/// at or above `FIRST_CONSTANT_REGISTER_INDEX` address the constant pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VirtualRegister(i32);

impl VirtualRegister {
    pub const INVALID: Self = Self(INVALID_VIRTUAL_REGISTER);
    pub const FIRST_CONSTANT: Self = Self(FIRST_CONSTANT_REGISTER_INDEX);

    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i32 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != INVALID_VIRTUAL_REGISTER
    }

    pub const fn is_local(self) -> bool {
        self.0 < 0
    }

    pub const fn is_argument_or_header(self) -> bool {
        self.0 >= 0 && self.0 < FIRST_CONSTANT_REGISTER_INDEX
    }

    pub const fn is_constant(self) -> bool {
        self.0 >= FIRST_CONSTANT_REGISTER_INDEX
    }

    pub const fn local(index: u32) -> Self {
        Self(-((index as i32) + 1))
    }

    pub const fn argument_or_header(raw_slot: u32) -> Self {
        Self(raw_slot as i32)
    }

    pub const fn argument_including_this(
        argument_index: u32,
        this_offset: ThisArgumentOffset,
    ) -> Self {
        Self(this_offset.0 + argument_index as i32)
    }

    pub const fn constant(index: u32) -> Self {
        Self(FIRST_CONSTANT_REGISTER_INDEX + index as i32)
    }

    pub const fn to_local_index(self) -> Option<u32> {
        if self.is_local() {
            Some((-1 - self.0) as u32)
        } else {
            None
        }
    }

    pub const fn to_constant_index(self) -> Option<u32> {
        if self.is_constant() {
            Some((self.0 - FIRST_CONSTANT_REGISTER_INDEX) as u32)
        } else {
            None
        }
    }

    pub fn classify(self, this_offset: ThisArgumentOffset) -> RegisterClass {
        if !self.is_valid() {
            RegisterClass::Invalid
        } else if let Some(index) = self.to_local_index() {
            RegisterClass::Local(index)
        } else if let Some(index) = self.to_constant_index() {
            RegisterClass::Constant(index)
        } else if self.0 < this_offset.0 {
            RegisterClass::CallFrameHeader(self.0 as u32)
        } else {
            RegisterClass::ArgumentIncludingThis((self.0 - this_offset.0) as u32)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RegisterClass {
    Invalid,
    Local(u32),
    CallFrameHeader(u32),
    ArgumentIncludingThis(u32),
    Constant(u32),
}

/// Register operand width selected by the bytecode encoder.
///
/// JSC has narrow and wide constant-register thresholds in the LLInt. This
/// type keeps those choices explicit without implementing fitting rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RegisterOperandWidth {
    Narrow8,
    Wide16,
    Wide32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RegisterOperandEncoding {
    pub register: VirtualRegister,
    pub width: RegisterOperandWidth,
}

/// Registers with special meaning to a code block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SpecialRegisters {
    pub this_register: VirtualRegister,
    pub scope_register: VirtualRegister,
    pub arguments_register: Option<VirtualRegister>,
    pub new_target_register: Option<VirtualRegister>,
    pub generator_register: Option<VirtualRegister>,
    pub promise_register: Option<VirtualRegister>,
}

impl Default for SpecialRegisters {
    fn default() -> Self {
        Self {
            this_register: VirtualRegister::INVALID,
            scope_register: VirtualRegister::INVALID,
            arguments_register: None,
            new_target_register: None,
            generator_register: None,
            promise_register: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct RegisterFrameShape {
    pub num_parameters_including_this: u32,
    pub num_vars: u32,
    pub num_callee_locals: u32,
    pub num_temporaries: u32,
    pub special: SpecialRegisters,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TemporaryRegister {
    pub register: VirtualRegister,
    pub lifetime: TemporaryLifetime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TemporaryLifetime {
    Expression,
    Statement,
    GeneratorInternal,
    CheckpointScratch,
}
