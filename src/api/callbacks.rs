use core::ffi::c_void;

use crate::api::exception::{ApiExceptionResult, ApiExceptionSemanticError, ApiThrowDisposition};
use crate::api::handles::{
    ApiClassRef, ApiGlobalContext, ApiObjectRef, ApiPropertyNameAccumulatorRef, ApiStringRef,
    ApiValueRef,
};
use crate::api::lock::ApiEntryScope;

/// JavaScript value type exposed through the C API.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiValueType {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Object,
    Symbol,
    BigInt,
}

/// Object factory family exposed by `JSObjectRef.h`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiObjectCreationKind {
    CustomClassInstance,
    HostFunction,
    HostConstructor,
    Array,
    Date,
    Error,
    RegExp,
    DeferredPromise,
    ScriptFunction,
}

/// Fallback rule when a host callback declines a property operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiPropertyFallback {
    StaticValues,
    StaticFunctions,
    ParentClassChain,
    PrototypeChain,
    DefaultObjectClass,
}

/// Lifecycle state for ref-counted `OpaqueJSClass` metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiClassLifecycle {
    DefinitionBorrowed,
    StaticTablesCopied,
    ContextDataCached,
    PrototypeCachedWeakly,
    ReleasedAfterInstances,
}

/// Authority over private data carried by callback objects.
///
/// Embedder private data is installed before class initialization callbacks run.
/// Finalizers observe it without a context and may run on any thread, so they
/// cannot allocate GC objects or enter APIs that require `JSContextRef`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiPrivateDataAuthority {
    EmbedderOwned,
    VisibleDuringInitialize,
    FinalizerOnlyCleanup,
    NoDefaultStorage,
}

/// Owner of immutable API class and callback schema tables.
///
/// Runtime class instances remain owned by the C API class registry and context
/// group. These schema owners only describe where a static descriptor table was
/// authored before any callback execution paths consume it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ApiSchemaOwner {
    #[default]
    CApiClassDefinition,
    CApiCallbackBridge,
    GeneratedHeaderMetadata,
    TestFixture,
}

/// Authority allowed to replace a published API schema registry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ApiRegistryMutationAuthority {
    #[default]
    ClassDefinitionImport,
    ContextGroupInitialization,
    GeneratedDataRefresh,
}

/// Provenance for generated or hand-authored API descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ApiSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl ApiSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Stable callback slot identity in `JSClassDefinition`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ApiCallbackKind {
    Initialize,
    Finalize,
    HasProperty,
    GetProperty,
    SetProperty,
    DeleteProperty,
    GetPropertyNames,
    CallAsFunction,
    CallAsConstructor,
    HasInstance,
    ConvertToType,
}

/// Immutable metadata for one callback slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiCallbackDescriptor {
    pub kind: ApiCallbackKind,
    pub name: &'static str,
    pub requires_entry_scope: bool,
    pub may_throw: bool,
    pub observes_private_data: bool,
}

/// Static class-member table family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ApiStaticMemberKind {
    Value,
    Function,
}

/// Immutable metadata for one static value/function table entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiStaticMemberDescriptor {
    pub name: &'static str,
    pub kind: ApiStaticMemberKind,
    pub attributes: ApiPropertyAttributes,
    pub callback: Option<ApiCallbackKind>,
}

/// Prototype relationship recorded in a static API class schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiClassPrototypeSchema {
    AutomaticPrototype,
    NoAutomaticPrototype,
    PrototypeClassName(&'static str),
}

/// Immutable class descriptor table entry.
///
/// The C API class registry owns live `OpaqueJSClass` instances and any copied
/// callback pointers. This schema borrows generated/static metadata only; it
/// does not allocate classes, call callbacks, or validate embedder tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiClassDescriptor {
    pub name: &'static str,
    pub attributes: ApiClassAttributes,
    pub lifecycle: ApiClassLifecycle,
    pub private_data_authority: ApiPrivateDataAuthority,
    pub prototype: ApiClassPrototypeSchema,
    pub callbacks: &'static [ApiCallbackDescriptor],
    pub static_members: &'static [ApiStaticMemberDescriptor],
    pub owner: ApiSchemaOwner,
    pub mutation_authority: ApiRegistryMutationAuthority,
    pub provenance: ApiSchemaProvenance,
}

impl ApiClassDescriptor {
    pub const fn callbacks(self) -> &'static [ApiCallbackDescriptor] {
        self.callbacks
    }

    pub const fn static_members(self) -> &'static [ApiStaticMemberDescriptor] {
        self.static_members
    }

    pub fn callback(self, kind: ApiCallbackKind) -> Option<&'static ApiCallbackDescriptor> {
        self.callbacks
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn static_member(self, name: &str) -> Option<&'static ApiStaticMemberDescriptor> {
        self.static_members
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn select(self) -> Result<ApiClassSelection, ApiDescriptorSelectionError> {
        self.validate()
            .map_err(ApiDescriptorSelectionError::InvalidClassDescriptor)?;
        Ok(ApiClassSelection {
            class_name: self.name,
            lifecycle: self.lifecycle,
            private_data_authority: self.private_data_authority,
            prototype: self.prototype,
            can_call_as_function: self.attributes.can_call_as_function,
            can_call_as_constructor: self.attributes.can_call_as_constructor,
            static_member_count: self.static_members.len(),
        })
    }

    pub fn select_static_member(
        self,
        name: &str,
        fallback: ApiPropertyFallback,
    ) -> Result<ApiStaticMemberSelection, ApiDescriptorSelectionError> {
        self.validate()
            .map_err(ApiDescriptorSelectionError::InvalidClassDescriptor)?;
        let Some(member) = self.static_member(name) else {
            return Err(ApiDescriptorSelectionError::StaticMemberNotFound);
        };
        member
            .validate()
            .map_err(ApiDescriptorSelectionError::InvalidClassDescriptor)?;

        let callback = match member.callback {
            Some(kind) => Some(
                self.callback(kind)
                    .ok_or(ApiDescriptorSelectionError::CallbackNotFound(kind))?,
            ),
            None => None,
        };

        Ok(ApiStaticMemberSelection {
            class_name: self.name,
            member_name: member.name,
            kind: member.kind,
            attributes: member.attributes,
            callback_kind: member.callback,
            callback_may_throw: callback.is_some_and(|descriptor| descriptor.may_throw),
            fallback,
        })
    }
}

/// Process-level view of immutable API class schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApiClassSchemaRegistry {
    pub classes: &'static [ApiClassDescriptor],
}

impl ApiClassSchemaRegistry {
    pub const fn new(classes: &'static [ApiClassDescriptor]) -> Self {
        Self { classes }
    }

    pub const fn classes(self) -> &'static [ApiClassDescriptor] {
        self.classes
    }

    pub fn class_named(self, name: &str) -> Option<&'static ApiClassDescriptor> {
        self.classes
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn select_class(
        self,
        name: &str,
    ) -> Result<ApiClassSelection, ApiDescriptorSelectionError> {
        self.validate()
            .map_err(ApiDescriptorSelectionError::InvalidClassDescriptor)?;
        self.class_named(name)
            .ok_or(ApiDescriptorSelectionError::ClassNotFound)?
            .select()
    }

    pub fn select_static_member(
        self,
        class_name: &str,
        member_name: &str,
        fallback: ApiPropertyFallback,
    ) -> Result<ApiStaticMemberSelection, ApiDescriptorSelectionError> {
        self.validate()
            .map_err(ApiDescriptorSelectionError::InvalidClassDescriptor)?;
        self.class_named(class_name)
            .ok_or(ApiDescriptorSelectionError::ClassNotFound)?
            .select_static_member(member_name, fallback)
    }

    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        validate_unique_names(
            self.classes.iter().map(|descriptor| descriptor.name),
            ApiClassValidationError::DuplicateClassName,
        )?;

        for descriptor in self.classes {
            descriptor.validate()?;
        }

        Ok(())
    }
}

/// Pure API class selection result used before any class allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiClassSelection {
    pub class_name: &'static str,
    pub lifecycle: ApiClassLifecycle,
    pub private_data_authority: ApiPrivateDataAuthority,
    pub prototype: ApiClassPrototypeSchema,
    pub can_call_as_function: bool,
    pub can_call_as_constructor: bool,
    pub static_member_count: usize,
}

/// Pure API static-member selection result used before callback invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiStaticMemberSelection {
    pub class_name: &'static str,
    pub member_name: &'static str,
    pub kind: ApiStaticMemberKind,
    pub attributes: ApiPropertyAttributes,
    pub callback_kind: Option<ApiCallbackKind>,
    pub callback_may_throw: bool,
    pub fallback: ApiPropertyFallback,
}

/// Non-executing completion category for a host callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiCallbackResultKind {
    Void,
    Boolean(bool),
    Value,
    Object,
    NotHandled,
    Threw,
    Terminated,
}

/// Semantic callback outcome after the C ABI shim has returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiCallbackSemanticOutcome {
    pub callback: ApiCallbackKind,
    pub result: ApiCallbackResultKind,
    pub throw_disposition: ApiThrowDisposition,
    pub exception_present: bool,
    pub may_throw: bool,
    pub requires_entry_scope: bool,
    pub used_fallback: bool,
    pub observed_private_data: bool,
}

impl ApiCallbackSemanticOutcome {
    pub fn from_descriptor(
        descriptor: ApiCallbackDescriptor,
        result: ApiCallbackResultKind,
        exception: ApiExceptionResult,
        used_fallback: bool,
    ) -> Result<Self, ApiCallbackSemanticError> {
        descriptor
            .validate()
            .map_err(ApiCallbackSemanticError::InvalidDescriptor)?;
        exception
            .validate()
            .map_err(ApiCallbackSemanticError::InvalidExceptionResult)?;

        if !descriptor.may_throw && exception.disposition() != ApiThrowDisposition::DidNotThrow {
            return Err(ApiCallbackSemanticError::NonThrowingCallbackReportedThrow(
                descriptor.kind,
            ));
        }
        match result {
            ApiCallbackResultKind::Threw
                if exception.disposition() != ApiThrowDisposition::PendingException =>
            {
                return Err(ApiCallbackSemanticError::ThrowOutcomeMissingException(
                    descriptor.kind,
                ));
            }
            ApiCallbackResultKind::Terminated
                if exception.disposition() != ApiThrowDisposition::Terminated =>
            {
                return Err(ApiCallbackSemanticError::TerminationOutcomeMismatch(
                    descriptor.kind,
                ));
            }
            ApiCallbackResultKind::Threw | ApiCallbackResultKind::Terminated => {}
            _ if exception.disposition() != ApiThrowDisposition::DidNotThrow => {
                return Err(ApiCallbackSemanticError::HandledOutcomeCarriesException(
                    descriptor.kind,
                ));
            }
            _ => {}
        }

        if used_fallback && result != ApiCallbackResultKind::NotHandled {
            return Err(ApiCallbackSemanticError::FallbackUsedAfterHandledCallback(
                descriptor.kind,
            ));
        }

        Ok(Self {
            callback: descriptor.kind,
            result,
            throw_disposition: exception.disposition(),
            exception_present: exception.exception().is_some(),
            may_throw: descriptor.may_throw,
            requires_entry_scope: descriptor.requires_entry_scope,
            used_fallback,
            observed_private_data: descriptor.observes_private_data,
        })
    }
}

/// API callback semantic validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiCallbackSemanticError {
    InvalidDescriptor(ApiClassValidationError),
    InvalidExceptionResult(ApiExceptionSemanticError),
    NonThrowingCallbackReportedThrow(ApiCallbackKind),
    ThrowOutcomeMissingException(ApiCallbackKind),
    TerminationOutcomeMismatch(ApiCallbackKind),
    HandledOutcomeCarriesException(ApiCallbackKind),
    FallbackUsedAfterHandledCallback(ApiCallbackKind),
}

/// API descriptor lookup and selection failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiDescriptorSelectionError {
    InvalidClassDescriptor(ApiClassValidationError),
    ClassNotFound,
    StaticMemberNotFound,
    CallbackNotFound(ApiCallbackKind),
}

/// Structural API class/callback descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiClassValidationError {
    EmptyClassName,
    EmptyCallbackName,
    EmptyStaticMemberName,
    EmptyProvenanceField,
    DuplicateClassName(&'static str),
    DuplicateCallbackKind(ApiCallbackKind),
    DuplicateCallbackName(&'static str),
    DuplicateStaticMemberName(&'static str),
    CallbackRequiresEntryScope(ApiCallbackKind),
    FinalizeRequiresEntryScope,
    FinalizeMayThrow,
    StaticValueUsesFunctionCallback(&'static str),
    StaticFunctionMissingCallCallback(&'static str),
    StaticFunctionUsesNonCallCallback(&'static str),
    CallableCallbackWithoutAttribute,
    ConstructorCallbackWithoutAttribute,
    NoAutomaticPrototypeMismatch,
    StaticValuePointerCountMismatch,
    StaticFunctionPointerCountMismatch,
}

impl ApiCallbackDescriptor {
    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        validate_non_empty(self.name, ApiClassValidationError::EmptyCallbackName)?;

        if self.kind == ApiCallbackKind::Finalize {
            if self.requires_entry_scope {
                return Err(ApiClassValidationError::FinalizeRequiresEntryScope);
            }
            if self.may_throw {
                return Err(ApiClassValidationError::FinalizeMayThrow);
            }
            return Ok(());
        }

        if !self.requires_entry_scope {
            return Err(ApiClassValidationError::CallbackRequiresEntryScope(
                self.kind,
            ));
        }

        Ok(())
    }
}

impl ApiStaticMemberDescriptor {
    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        validate_non_empty(self.name, ApiClassValidationError::EmptyStaticMemberName)?;

        match (self.kind, self.callback) {
            (ApiStaticMemberKind::Value, Some(ApiCallbackKind::CallAsFunction))
            | (ApiStaticMemberKind::Value, Some(ApiCallbackKind::CallAsConstructor)) => Err(
                ApiClassValidationError::StaticValueUsesFunctionCallback(self.name),
            ),
            (ApiStaticMemberKind::Function, None) => Err(
                ApiClassValidationError::StaticFunctionMissingCallCallback(self.name),
            ),
            (ApiStaticMemberKind::Function, Some(ApiCallbackKind::CallAsFunction)) => Ok(()),
            (ApiStaticMemberKind::Function, Some(_)) => Err(
                ApiClassValidationError::StaticFunctionUsesNonCallCallback(self.name),
            ),
            _ => Ok(()),
        }
    }
}

impl ApiClassDescriptor {
    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        validate_non_empty(self.name, ApiClassValidationError::EmptyClassName)?;
        validate_non_empty(
            self.provenance.generator,
            ApiClassValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            ApiClassValidationError::EmptyProvenanceField,
        )?;
        validate_unique_callback_descriptors(self.callbacks)?;
        validate_unique_names(
            self.static_members.iter().map(|descriptor| descriptor.name),
            ApiClassValidationError::DuplicateStaticMemberName,
        )?;

        for callback in self.callbacks {
            callback.validate()?;
        }
        for member in self.static_members {
            member.validate()?;
        }

        if self.attributes.no_automatic_prototype
            && self.prototype != ApiClassPrototypeSchema::NoAutomaticPrototype
        {
            return Err(ApiClassValidationError::NoAutomaticPrototypeMismatch);
        }

        Ok(())
    }
}

/// Builder for immutable API class descriptors.
#[derive(Clone, Copy, Debug)]
pub struct ApiClassDescriptorBuilder {
    descriptor: ApiClassDescriptor,
}

impl ApiClassDescriptorBuilder {
    pub const fn new(name: &'static str, provenance: ApiSchemaProvenance) -> Self {
        Self {
            descriptor: ApiClassDescriptor {
                name,
                attributes: ApiClassAttributes {
                    can_call_as_function: false,
                    can_call_as_constructor: false,
                    no_automatic_prototype: false,
                },
                lifecycle: ApiClassLifecycle::DefinitionBorrowed,
                private_data_authority: ApiPrivateDataAuthority::EmbedderOwned,
                prototype: ApiClassPrototypeSchema::AutomaticPrototype,
                callbacks: &[],
                static_members: &[],
                owner: ApiSchemaOwner::CApiClassDefinition,
                mutation_authority: ApiRegistryMutationAuthority::ClassDefinitionImport,
                provenance,
            },
        }
    }

    pub const fn attributes(mut self, attributes: ApiClassAttributes) -> Self {
        self.descriptor.attributes = attributes;
        self
    }

    pub const fn lifecycle(mut self, lifecycle: ApiClassLifecycle) -> Self {
        self.descriptor.lifecycle = lifecycle;
        self
    }

    pub const fn private_data_authority(mut self, authority: ApiPrivateDataAuthority) -> Self {
        self.descriptor.private_data_authority = authority;
        self
    }

    pub const fn prototype(mut self, prototype: ApiClassPrototypeSchema) -> Self {
        self.descriptor.prototype = prototype;
        self
    }

    pub const fn callbacks(mut self, callbacks: &'static [ApiCallbackDescriptor]) -> Self {
        self.descriptor.callbacks = callbacks;
        self
    }

    pub const fn static_members(
        mut self,
        static_members: &'static [ApiStaticMemberDescriptor],
    ) -> Self {
        self.descriptor.static_members = static_members;
        self
    }

    pub const fn owner(mut self, owner: ApiSchemaOwner) -> Self {
        self.descriptor.owner = owner;
        self
    }

    pub const fn mutation_authority(mut self, authority: ApiRegistryMutationAuthority) -> Self {
        self.descriptor.mutation_authority = authority;
        self
    }

    pub fn build(self) -> Result<ApiClassDescriptor, ApiClassValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

/// Canonical callback slots copied from `JSClassDefinition`.
pub const API_CLASS_CALLBACK_DESCRIPTORS: &[ApiCallbackDescriptor] = &[
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::Initialize,
        name: "initialize",
        requires_entry_scope: true,
        may_throw: false,
        observes_private_data: true,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::Finalize,
        name: "finalize",
        requires_entry_scope: false,
        may_throw: false,
        observes_private_data: true,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::HasProperty,
        name: "hasProperty",
        requires_entry_scope: true,
        may_throw: false,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::GetProperty,
        name: "getProperty",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::SetProperty,
        name: "setProperty",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::DeleteProperty,
        name: "deleteProperty",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::GetPropertyNames,
        name: "getPropertyNames",
        requires_entry_scope: true,
        may_throw: false,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::CallAsFunction,
        name: "callAsFunction",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::CallAsConstructor,
        name: "callAsConstructor",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::HasInstance,
        name: "hasInstance",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
    ApiCallbackDescriptor {
        kind: ApiCallbackKind::ConvertToType,
        name: "convertToType",
        requires_entry_scope: true,
        may_throw: true,
        observes_private_data: false,
    },
];

/// Empty class table published until generated API class metadata is imported.
pub const API_CLASS_SCHEMA_REGISTRY: ApiClassSchemaRegistry =
    ApiClassSchemaRegistry { classes: &[] };

/// Finalizer callback for host private data.
///
/// # Safety
///
/// The callback receives embedder-owned private data during finalization. It
/// must not reenter the VM unless the final API contract explicitly permits it.
pub type ApiFinalizeCallback = unsafe extern "C" fn(private_data: *mut c_void);

pub type ApiInitializeCallback = extern "C" fn(scope: &ApiEntryScope<'_>, object: ApiObjectRef);

pub type ApiHasPropertyCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    property_name: ApiStringRef,
) -> bool;

pub type ApiGetPropertyCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    property_name: ApiStringRef,
    exception: *mut ApiExceptionResult,
) -> ApiValueRef;

pub type ApiSetPropertyCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    property_name: ApiStringRef,
    value: ApiValueRef,
    exception: *mut ApiExceptionResult,
) -> bool;

pub type ApiDeletePropertyCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    property_name: ApiStringRef,
    exception: *mut ApiExceptionResult,
) -> bool;

pub type ApiGetPropertyNamesCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    accumulator: ApiPropertyNameAccumulatorRef,
);

/// Host function callback shape.
///
/// The call occurs inside an `ApiEntryScope`; implementations may reenter the
/// engine only through documented API entry points and must report exceptions
/// through `ApiExceptionResult`.
pub type ApiHostFunction = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    this_object: ApiObjectRef,
    arguments: *const ApiValueRef,
    argument_count: usize,
    exception: *mut ApiExceptionResult,
) -> ApiValueRef;

pub type ApiHostConstructor = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    constructor: ApiObjectRef,
    arguments: *const ApiValueRef,
    argument_count: usize,
    exception: *mut ApiExceptionResult,
) -> ApiObjectRef;

pub type ApiHasInstanceCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    constructor: ApiObjectRef,
    possible_instance: ApiValueRef,
    exception: *mut ApiExceptionResult,
) -> bool;

pub type ApiConvertToTypeCallback = extern "C" fn(
    scope: &ApiEntryScope<'_>,
    object: ApiObjectRef,
    value_type: ApiValueType,
    exception: *mut ApiExceptionResult,
) -> ApiValueRef;

/// Static property attributes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiPropertyAttributes {
    pub read_only: bool,
    pub dont_enum: bool,
    pub dont_delete: bool,
}

/// Prototype policy copied from `JSClassDefinition`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiClassPrototypePolicy {
    AutomaticPrototype,
    NoAutomaticPrototype,
    PrototypeClass(ApiClassRef),
}

/// Static class value entry.
#[derive(Clone, Copy, Debug)]
pub struct ApiStaticValue {
    pub name: *const u8,
    pub get_property: Option<ApiGetPropertyCallback>,
    pub set_property: Option<ApiSetPropertyCallback>,
    pub attributes: ApiPropertyAttributes,
}

/// Static class function entry.
#[derive(Clone, Copy, Debug)]
pub struct ApiStaticFunction {
    pub name: *const u8,
    pub call_as_function: Option<ApiHostFunction>,
    pub attributes: ApiPropertyAttributes,
}

/// Host-defined class attributes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiClassAttributes {
    pub can_call_as_function: bool,
    pub can_call_as_constructor: bool,
    pub no_automatic_prototype: bool,
}

/// Static member table copied out of a class definition.
#[derive(Clone, Copy, Debug)]
pub struct ApiClassStaticTable {
    pub values: *const ApiStaticValue,
    pub value_count: usize,
    pub functions: *const ApiStaticFunction,
    pub function_count: usize,
}

impl ApiClassStaticTable {
    pub const fn empty() -> Self {
        Self {
            values: core::ptr::null(),
            value_count: 0,
            functions: core::ptr::null(),
            function_count: 0,
        }
    }

    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        if self.values.is_null() != (self.value_count == 0) {
            return Err(ApiClassValidationError::StaticValuePointerCountMismatch);
        }
        if self.functions.is_null() != (self.function_count == 0) {
            return Err(ApiClassValidationError::StaticFunctionPointerCountMismatch);
        }
        Ok(())
    }
}

/// Callback table copied from a class definition.
#[derive(Clone, Copy, Debug)]
pub struct ApiClassCallbacks {
    pub initialize: Option<ApiInitializeCallback>,
    pub finalize: Option<ApiFinalizeCallback>,
    pub has_property: Option<ApiHasPropertyCallback>,
    pub get_property: Option<ApiGetPropertyCallback>,
    pub set_property: Option<ApiSetPropertyCallback>,
    pub delete_property: Option<ApiDeletePropertyCallback>,
    pub get_property_names: Option<ApiGetPropertyNamesCallback>,
    pub call_as_function: Option<ApiHostFunction>,
    pub call_as_constructor: Option<ApiHostConstructor>,
    pub has_instance: Option<ApiHasInstanceCallback>,
    pub convert_to_type: Option<ApiConvertToTypeCallback>,
}

impl ApiClassCallbacks {
    pub const fn empty() -> Self {
        Self {
            initialize: None,
            finalize: None,
            has_property: None,
            get_property: None,
            set_property: None,
            delete_property: None,
            get_property_names: None,
            call_as_function: None,
            call_as_constructor: None,
            has_instance: None,
            convert_to_type: None,
        }
    }
}

/// Host-defined class metadata.
///
/// Class metadata owns callback tables and private-data behavior. It must be
/// retained independently from any object instance that references it.
#[derive(Clone, Copy, Debug)]
pub struct ApiClass {
    attributes: ApiClassAttributes,
    callbacks: ApiClassCallbacks,
    parent_class: Option<ApiClassRef>,
    prototype_policy: ApiClassPrototypePolicy,
    static_table: ApiClassStaticTable,
}

impl ApiClass {
    pub const fn new(
        attributes: ApiClassAttributes,
        finalize: Option<ApiFinalizeCallback>,
    ) -> Self {
        Self {
            attributes,
            callbacks: ApiClassCallbacks {
                finalize,
                ..ApiClassCallbacks::empty()
            },
            parent_class: None,
            prototype_policy: ApiClassPrototypePolicy::AutomaticPrototype,
            static_table: ApiClassStaticTable::empty(),
        }
    }

    pub const fn with_callbacks(
        attributes: ApiClassAttributes,
        callbacks: ApiClassCallbacks,
        parent_class: Option<ApiClassRef>,
    ) -> Self {
        Self {
            attributes,
            callbacks,
            parent_class,
            prototype_policy: ApiClassPrototypePolicy::AutomaticPrototype,
            static_table: ApiClassStaticTable::empty(),
        }
    }

    pub const fn with_definition(
        attributes: ApiClassAttributes,
        callbacks: ApiClassCallbacks,
        parent_class: Option<ApiClassRef>,
        prototype_policy: ApiClassPrototypePolicy,
        static_table: ApiClassStaticTable,
    ) -> Self {
        Self {
            attributes,
            callbacks,
            parent_class,
            prototype_policy,
            static_table,
        }
    }

    pub const fn attributes(self) -> ApiClassAttributes {
        self.attributes
    }

    pub const fn finalize(self) -> Option<ApiFinalizeCallback> {
        self.callbacks.finalize
    }

    pub const fn callbacks(self) -> ApiClassCallbacks {
        self.callbacks
    }

    pub const fn parent_class(self) -> Option<ApiClassRef> {
        self.parent_class
    }

    pub const fn prototype_policy(self) -> ApiClassPrototypePolicy {
        self.prototype_policy
    }

    pub const fn static_table(self) -> ApiClassStaticTable {
        self.static_table
    }

    pub fn validate(self) -> Result<(), ApiClassValidationError> {
        if self.callbacks.call_as_function.is_some() && !self.attributes.can_call_as_function {
            return Err(ApiClassValidationError::CallableCallbackWithoutAttribute);
        }
        if self.callbacks.call_as_constructor.is_some() && !self.attributes.can_call_as_constructor
        {
            return Err(ApiClassValidationError::ConstructorCallbackWithoutAttribute);
        }
        if self.attributes.no_automatic_prototype
            && self.prototype_policy != ApiClassPrototypePolicy::NoAutomaticPrototype
        {
            return Err(ApiClassValidationError::NoAutomaticPrototypeMismatch);
        }
        self.static_table.validate()
    }
}

/// Builder for copied API class definitions.
#[derive(Clone, Copy, Debug)]
pub struct ApiClassBuilder {
    attributes: ApiClassAttributes,
    callbacks: ApiClassCallbacks,
    parent_class: Option<ApiClassRef>,
    prototype_policy: ApiClassPrototypePolicy,
    static_table: ApiClassStaticTable,
}

impl ApiClassBuilder {
    pub const fn new(attributes: ApiClassAttributes) -> Self {
        Self {
            attributes,
            callbacks: ApiClassCallbacks::empty(),
            parent_class: None,
            prototype_policy: ApiClassPrototypePolicy::AutomaticPrototype,
            static_table: ApiClassStaticTable::empty(),
        }
    }

    pub const fn callbacks(mut self, callbacks: ApiClassCallbacks) -> Self {
        self.callbacks = callbacks;
        self
    }

    pub const fn parent_class(mut self, parent_class: ApiClassRef) -> Self {
        self.parent_class = Some(parent_class);
        self
    }

    pub const fn prototype_policy(mut self, policy: ApiClassPrototypePolicy) -> Self {
        self.prototype_policy = policy;
        self
    }

    pub const fn static_table(mut self, table: ApiClassStaticTable) -> Self {
        self.static_table = table;
        self
    }

    pub fn build(self) -> Result<ApiClass, ApiClassValidationError> {
        let class = ApiClass::with_definition(
            self.attributes,
            self.callbacks,
            self.parent_class,
            self.prototype_policy,
            self.static_table,
        );
        class.validate()?;
        Ok(class)
    }
}

/// Per-context-group data attached to a class.
#[derive(Clone, Copy, Debug)]
pub struct ApiClassContextData {
    class_ref: ApiClassRef,
    cached_prototype: Option<ApiObjectRef>,
    static_table_revision: u64,
}

impl ApiClassContextData {
    pub const fn new(class_ref: ApiClassRef, static_table_revision: u64) -> Self {
        Self {
            class_ref,
            cached_prototype: None,
            static_table_revision,
        }
    }

    pub const fn with_cached_prototype(
        class_ref: ApiClassRef,
        cached_prototype: ApiObjectRef,
        static_table_revision: u64,
    ) -> Self {
        Self {
            class_ref,
            cached_prototype: Some(cached_prototype),
            static_table_revision,
        }
    }

    pub const fn class_ref(self) -> ApiClassRef {
        self.class_ref
    }

    pub const fn cached_prototype(self) -> Option<ApiObjectRef> {
        self.cached_prototype
    }

    pub const fn static_table_revision(self) -> u64 {
        self.static_table_revision
    }
}

/// Private property storage shape for callback objects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiCallbackPrivatePropertyMap {
    pub property_count: u32,
    pub barrier_generation: u64,
    pub is_locked: bool,
}

/// Host callback object data tied to an API global context.
///
/// Mutation authority for object properties stays with the callback table and
/// the engine's ordinary object semantics. The raw private pointer is an
/// embedder resource; Rust must not infer ownership or thread affinity from it.
#[derive(Clone, Copy, Debug)]
pub struct ApiCallbackObject {
    global_context: ApiGlobalContext,
    class: ApiClass,
    object: ApiObjectRef,
    private_data: *mut c_void,
    private_properties: Option<ApiCallbackPrivatePropertyMap>,
}

impl ApiCallbackObject {
    pub const fn new(
        global_context: ApiGlobalContext,
        class: ApiClass,
        object: ApiObjectRef,
    ) -> Self {
        Self {
            global_context,
            class,
            object,
            private_data: core::ptr::null_mut(),
            private_properties: None,
        }
    }

    pub const fn with_private_data(
        global_context: ApiGlobalContext,
        class: ApiClass,
        object: ApiObjectRef,
        private_data: *mut c_void,
        private_properties: Option<ApiCallbackPrivatePropertyMap>,
    ) -> Self {
        Self {
            global_context,
            class,
            object,
            private_data,
            private_properties,
        }
    }

    pub const fn object(self) -> ApiObjectRef {
        self.object
    }

    pub const fn global_context(self) -> ApiGlobalContext {
        self.global_context
    }

    pub const fn class(self) -> ApiClass {
        self.class
    }

    pub const fn private_data(self) -> *mut c_void {
        self.private_data
    }

    pub const fn private_properties(self) -> Option<ApiCallbackPrivatePropertyMap> {
        self.private_properties
    }
}

fn validate_non_empty(
    value: &'static str,
    error: ApiClassValidationError,
) -> Result<(), ApiClassValidationError> {
    if value.is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_unique_names<I, F>(names: I, duplicate: F) -> Result<(), ApiClassValidationError>
where
    I: Clone + Iterator<Item = &'static str>,
    F: Fn(&'static str) -> ApiClassValidationError,
{
    let outer = names.clone();
    for (index, name) in outer.enumerate() {
        for other in names.clone().skip(index + 1) {
            if name == other {
                return Err(duplicate(name));
            }
        }
    }
    Ok(())
}

fn validate_unique_callback_descriptors(
    callbacks: &[ApiCallbackDescriptor],
) -> Result<(), ApiClassValidationError> {
    for (index, callback) in callbacks.iter().enumerate() {
        for other in callbacks.iter().skip(index + 1) {
            if callback.kind == other.kind {
                return Err(ApiClassValidationError::DuplicateCallbackKind(
                    callback.kind,
                ));
            }
            if callback.name == other.name {
                return Err(ApiClassValidationError::DuplicateCallbackName(
                    callback.name,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handles::ApiOpaqueHandle;
    use core::ffi::c_void;
    use core::ptr::NonNull;

    const TEST_PROVENANCE: ApiSchemaProvenance =
        ApiSchemaProvenance::new("test", "api/callbacks.rs", 1);
    const TEST_CALLBACKS: &[ApiCallbackDescriptor] = &[
        ApiCallbackDescriptor {
            kind: ApiCallbackKind::GetProperty,
            name: "getProperty",
            requires_entry_scope: true,
            may_throw: true,
            observes_private_data: false,
        },
        ApiCallbackDescriptor {
            kind: ApiCallbackKind::CallAsFunction,
            name: "callAsFunction",
            requires_entry_scope: true,
            may_throw: true,
            observes_private_data: false,
        },
    ];
    const TEST_MEMBERS: &[ApiStaticMemberDescriptor] = &[
        ApiStaticMemberDescriptor {
            name: "answer",
            kind: ApiStaticMemberKind::Value,
            attributes: ApiPropertyAttributes {
                read_only: true,
                dont_enum: false,
                dont_delete: false,
            },
            callback: Some(ApiCallbackKind::GetProperty),
        },
        ApiStaticMemberDescriptor {
            name: "call",
            kind: ApiStaticMemberKind::Function,
            attributes: ApiPropertyAttributes {
                read_only: true,
                dont_enum: false,
                dont_delete: true,
            },
            callback: Some(ApiCallbackKind::CallAsFunction),
        },
    ];
    const TEST_CLASS: ApiClassDescriptor = ApiClassDescriptor {
        name: "HostClass",
        attributes: ApiClassAttributes {
            can_call_as_function: false,
            can_call_as_constructor: false,
            no_automatic_prototype: false,
        },
        lifecycle: ApiClassLifecycle::StaticTablesCopied,
        private_data_authority: ApiPrivateDataAuthority::EmbedderOwned,
        prototype: ApiClassPrototypeSchema::AutomaticPrototype,
        callbacks: TEST_CALLBACKS,
        static_members: TEST_MEMBERS,
        owner: ApiSchemaOwner::TestFixture,
        mutation_authority: ApiRegistryMutationAuthority::ClassDefinitionImport,
        provenance: TEST_PROVENANCE,
    };
    const TEST_CLASSES: &[ApiClassDescriptor] = &[TEST_CLASS];
    const TEST_REGISTRY: ApiClassSchemaRegistry = ApiClassSchemaRegistry::new(TEST_CLASSES);

    fn value_ref() -> ApiValueRef {
        let raw = NonNull::<c_void>::dangling();
        let handle = unsafe { ApiOpaqueHandle::from_raw(raw) };
        unsafe { ApiValueRef::from_opaque(handle) }
    }

    #[test]
    fn validates_canonical_callback_descriptors() {
        for descriptor in API_CLASS_CALLBACK_DESCRIPTORS {
            assert_eq!(descriptor.validate(), Ok(()));
        }
    }

    #[test]
    fn rejects_static_function_without_call_callback() {
        let member = ApiStaticMemberDescriptor {
            name: "f",
            kind: ApiStaticMemberKind::Function,
            attributes: ApiPropertyAttributes {
                read_only: false,
                dont_enum: false,
                dont_delete: false,
            },
            callback: None,
        };

        assert_eq!(
            member.validate(),
            Err(ApiClassValidationError::StaticFunctionMissingCallCallback(
                "f"
            ))
        );
    }

    #[test]
    fn builds_valid_class_descriptor() {
        let descriptor = ApiClassDescriptorBuilder::new("Host", TEST_PROVENANCE)
            .callbacks(API_CLASS_CALLBACK_DESCRIPTORS)
            .build();

        assert!(descriptor.is_ok());
    }

    #[test]
    fn selects_class_descriptor_without_allocating_class() {
        let selection = TEST_REGISTRY
            .select_class("HostClass")
            .expect("class selection");

        assert_eq!(
            selection,
            ApiClassSelection {
                class_name: "HostClass",
                lifecycle: ApiClassLifecycle::StaticTablesCopied,
                private_data_authority: ApiPrivateDataAuthority::EmbedderOwned,
                prototype: ApiClassPrototypeSchema::AutomaticPrototype,
                can_call_as_function: false,
                can_call_as_constructor: false,
                static_member_count: 2,
            }
        );
    }

    #[test]
    fn selects_static_member_callback_metadata() {
        let selection = TEST_REGISTRY
            .select_static_member("HostClass", "answer", ApiPropertyFallback::ParentClassChain)
            .expect("member selection");

        assert_eq!(selection.member_name, "answer");
        assert_eq!(selection.callback_kind, Some(ApiCallbackKind::GetProperty));
        assert!(selection.callback_may_throw);
        assert_eq!(selection.fallback, ApiPropertyFallback::ParentClassChain);
    }

    #[test]
    fn rejects_missing_static_member_selection() {
        assert_eq!(
            TEST_REGISTRY.select_static_member(
                "HostClass",
                "missing",
                ApiPropertyFallback::StaticValues,
            ),
            Err(ApiDescriptorSelectionError::StaticMemberNotFound)
        );
    }

    #[test]
    fn callback_semantics_accept_throwing_get_property_exception() {
        let outcome = ApiCallbackSemanticOutcome::from_descriptor(
            TEST_CALLBACKS[0],
            ApiCallbackResultKind::Threw,
            ApiExceptionResult::pending(value_ref()),
            false,
        )
        .expect("callback outcome");

        assert_eq!(outcome.callback, ApiCallbackKind::GetProperty);
        assert!(outcome.exception_present);
        assert!(outcome.may_throw);
    }

    #[test]
    fn callback_semantics_reject_non_throwing_callback_exception() {
        assert_eq!(
            ApiCallbackSemanticOutcome::from_descriptor(
                API_CLASS_CALLBACK_DESCRIPTORS[0],
                ApiCallbackResultKind::Threw,
                ApiExceptionResult::pending(value_ref()),
                false,
            ),
            Err(ApiCallbackSemanticError::NonThrowingCallbackReportedThrow(
                ApiCallbackKind::Initialize
            ))
        );
    }

    #[test]
    fn callback_semantics_allow_fallback_only_for_not_handled() {
        let outcome = ApiCallbackSemanticOutcome::from_descriptor(
            TEST_CALLBACKS[0],
            ApiCallbackResultKind::NotHandled,
            ApiExceptionResult::none(),
            true,
        )
        .expect("fallback outcome");

        assert!(outcome.used_fallback);

        assert_eq!(
            ApiCallbackSemanticOutcome::from_descriptor(
                TEST_CALLBACKS[0],
                ApiCallbackResultKind::Value,
                ApiExceptionResult::none(),
                true,
            ),
            Err(ApiCallbackSemanticError::FallbackUsedAfterHandledCallback(
                ApiCallbackKind::GetProperty
            ))
        );
    }
}
