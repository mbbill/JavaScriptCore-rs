use crate::runtime::exception::JsResult;
use crate::runtime::state::{ObjectId, RuntimeValue, StringId, StructureId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct RegExpProgramId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegExpObject {
    /// RegExpObject couples a compiled regexp cell with observable lastIndex.
    ///
    /// `lastIndex` writes can throw when the property is non-writable, and
    /// legacy RegExp constructor side effects are gated by `legacy_features`.
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub program: Option<RegExpProgramId>,
    pub source: Option<StringId>,
    pub flags: RegExpFlags,
    pub last_index: RuntimeValue,
    pub last_index_writable: bool,
    pub legacy_features: RegExpLegacyFeatures,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct RegExpFlags {
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub unicode_sets: bool,
    pub sticky: bool,
    pub has_indices: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RegExpLegacyFeatures {
    Disabled,
    #[default]
    Enabled,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegExpMatchRequest {
    pub regexp: ObjectId,
    pub input: StringId,
    pub start_index: u64,
    pub mode: RegExpMatchMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RegExpMatchMode {
    #[default]
    Exec,
    Test,
    Match,
    MatchAll,
    Search,
    Replace,
    Split,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegExpMatchResult {
    pub matched: bool,
    pub start: u64,
    pub end: u64,
    pub captures: Vec<RegExpCapture>,
    pub groups: Option<ObjectId>,
    pub indices: Option<ObjectId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegExpCapture {
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub name: Option<StringId>,
}

pub trait RegExpOperations {
    fn compile_regexp(&mut self, source: StringId, flags: RegExpFlags)
        -> JsResult<RegExpProgramId>;
    fn execute_regexp(&mut self, request: RegExpMatchRequest) -> JsResult<RegExpMatchResult>;
    fn set_last_index(
        &mut self,
        regexp: ObjectId,
        value: RuntimeValue,
        should_throw: bool,
    ) -> JsResult<bool>;
    fn create_regexp_string_iterator(
        &mut self,
        regexp: ObjectId,
        input: StringId,
        global: bool,
        unicode: bool,
    ) -> JsResult<ObjectId>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegExpLastIndexPlan {
    Keep,
    ResetToZero,
    Write(RuntimeValue),
    RejectReadOnly,
}

impl RegExpObject {
    pub fn plan_last_index_after_match(&self, result: &RegExpMatchResult) -> RegExpLastIndexPlan {
        if !(self.flags.global || self.flags.sticky) {
            return RegExpLastIndexPlan::Keep;
        }
        if !self.last_index_writable {
            return RegExpLastIndexPlan::RejectReadOnly;
        }
        if result.matched {
            if result.end <= i32::MAX as u64 {
                RegExpLastIndexPlan::Write(RuntimeValue::from_i32(result.end as i32))
            } else {
                RegExpLastIndexPlan::Keep
            }
        } else if self.flags.sticky {
            RegExpLastIndexPlan::ResetToZero
        } else {
            RegExpLastIndexPlan::Keep
        }
    }
}
