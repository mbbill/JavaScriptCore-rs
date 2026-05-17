use crate::runtime::exception::JsResult;
use crate::runtime::property::RuntimePropertyKey;
use crate::runtime::state::{ObjectId, RuntimeValue, StringId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonObject {
    pub object: Option<ObjectId>,
    pub has_raw_json_methods: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonParseRequest {
    pub source: StringId,
    pub reviver: RuntimeValue,
    pub source_text_access_enabled: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonStringifyRequest {
    pub value: RuntimeValue,
    pub replacer: RuntimeValue,
    pub space: RuntimeValue,
    pub stack: JsonStringifyStack,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonStringifyStack {
    pub object_stack_depth: usize,
    pub holder_stack_depth: usize,
    pub gap: Option<StringId>,
    pub using_array_replacer: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonPropertyVisit {
    pub holder: ObjectId,
    pub key: RuntimePropertyKey,
    pub value: RuntimeValue,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawJsonObject {
    pub object: Option<ObjectId>,
    pub raw_text: Option<StringId>,
}

pub trait JsonOperations {
    fn parse_json(&mut self, request: JsonParseRequest) -> JsResult<RuntimeValue>;
    fn stringify_json(&mut self, request: JsonStringifyRequest) -> JsResult<Option<StringId>>;
    fn call_to_json(&mut self, visit: JsonPropertyVisit) -> JsResult<RuntimeValue>;
    fn create_raw_json(&mut self, source: StringId) -> JsResult<RawJsonObject>;
}
