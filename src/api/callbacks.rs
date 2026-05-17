use core::ffi::c_void;

use crate::api::exception::ApiExceptionResult;
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
