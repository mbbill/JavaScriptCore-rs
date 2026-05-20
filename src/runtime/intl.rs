use crate::runtime::exception::JsResult;
use crate::runtime::state::{HostHookId, ObjectId, RuntimeValue, StringId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntlObject {
    pub object: Option<ObjectId>,
    pub default_locale: Option<StringId>,
    pub hooks: IntlHostHooks,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntlHostHooks {
    /// ICU and platform data are reached through host hooks, not hard-coded in
    /// runtime object semantics.
    pub default_locale: Option<HostHookId>,
    pub available_locales: Option<HostHookId>,
    pub canonicalize_locale: Option<HostHookId>,
    pub resolve_time_zone: Option<HostHookId>,
    pub format_to_parts: Option<HostHookId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocaleResolutionRequest {
    pub requested_locales: Vec<StringId>,
    pub matcher: LocaleMatcher,
    pub relevant_extension_keys: Vec<RelevantExtensionKey>,
    pub options: RuntimeValue,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LocaleMatcher {
    Lookup,
    #[default]
    BestFit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RelevantExtensionKey {
    Calendar,
    Collation,
    HourCycle,
    CaseFirst,
    Numeric,
    NumberingSystem,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedLocale {
    pub locale: Option<StringId>,
    pub data_locale: Option<StringId>,
    pub extensions: Vec<IntlExtension>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct IntlExtension {
    pub key: RelevantExtensionKey,
    pub value: StringId,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum IntlServiceKind {
    #[default]
    Collator,
    DateTimeFormat,
    DisplayNames,
    DurationFormat,
    ListFormat,
    Locale,
    NumberFormat,
    PluralRules,
    RelativeTimeFormat,
    Segmenter,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntlServiceObject {
    pub object: Option<ObjectId>,
    pub service: IntlServiceKind,
    pub locale: Option<StringId>,
    pub options: RuntimeValue,
    pub initialized: bool,
}

pub trait IntlOperations {
    fn canonicalize_locale_list(&mut self, locales: RuntimeValue) -> JsResult<Vec<StringId>>;
    fn resolve_locale(&mut self, request: LocaleResolutionRequest) -> JsResult<ResolvedLocale>;
    fn initialize_intl_service(
        &mut self,
        service: IntlServiceKind,
        locales: RuntimeValue,
        options: RuntimeValue,
    ) -> JsResult<IntlServiceObject>;
    fn supported_locales_of(
        &mut self,
        service: IntlServiceKind,
        locales: RuntimeValue,
        options: RuntimeValue,
    ) -> JsResult<ObjectId>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IntlInitializationPlan {
    Initialize,
    AlreadyInitialized,
    RequiresHostLocale,
}

impl IntlServiceObject {
    pub fn initialization_plan(&self, intl: &IntlObject) -> IntlInitializationPlan {
        if self.initialized {
            IntlInitializationPlan::AlreadyInitialized
        } else if self.locale.is_none()
            && intl.default_locale.is_none()
            && intl.hooks.default_locale.is_none()
        {
            IntlInitializationPlan::RequiresHostLocale
        } else {
            IntlInitializationPlan::Initialize
        }
    }
}
