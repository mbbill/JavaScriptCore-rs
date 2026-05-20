//! Inspector protocol contracts.
//!
//! The inspector is a host/debugging boundary. This module records agents,
//! sessions, frontend channels, runtime domains, and instrumentation hooks
//! without implementing protocol transport.

use crate::bytecode::{BytecodeIndex, SourceProviderId};
use crate::debugger::{
    DebuggerBreakpointId, DebuggerCallFrameDescriptor, DebuggerDiagnosticReport,
    DebuggerPauseReason, DebuggerSourceId,
};
use crate::gc::{HeapId, HeapSnapshotId, HeapSnapshotKind};
use crate::runtime::{CodeBlockId, GlobalObjectId, HostHookId, ObjectId, RuntimeValue};
use crate::wasm::{WasmDebugServerState, WasmModuleId};

/// Inspector protocol domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InspectorDomain {
    Runtime,
    Debugger,
    Console,
    Heap,
    ScriptProfiler,
    Target,
    Audit,
    WasmDebugger,
    Page,
    Network,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct InspectorHeapObservationId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorHeapObservationKind {
    HeapDomainEnabled,
    SnapshotRequested,
    SnapshotChunkReported,
    SnapshotCompleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorHeapObservationRecord {
    pub id: InspectorHeapObservationId,
    pub session: Option<InspectorSessionId>,
    pub kind: InspectorHeapObservationKind,
    pub heap: Option<HeapId>,
    pub snapshot: Option<HeapSnapshotId>,
    pub snapshot_kind: HeapSnapshotKind,
    pub global_object: Option<GlobalObjectId>,
    pub observed_node_count: usize,
    pub observed_edge_count: usize,
}

/// Owner of immutable inspector protocol/session/agent schemas.
///
/// Live sessions and agents remain owned by the inspector controller and
/// frontend router. These owners describe static protocol metadata only.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InspectorSchemaOwner {
    #[default]
    InspectorController,
    InspectorAgent,
    GeneratedProtocolJson,
    TestFixture,
}

/// Authority allowed to replace inspector schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum InspectorRegistryMutationAuthority {
    #[default]
    GeneratedDataRefresh,
    CrateInitialization,
    SessionBootstrap,
}

/// Provenance for generated inspector protocol metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct InspectorSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl InspectorSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Protocol field family without JSON parsing or validation behavior.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InspectorProtocolFieldKind {
    Boolean,
    Integer,
    Number,
    String,
    Object,
    Array,
    Any,
}

/// Immutable field metadata for an inspector protocol command or event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolFieldSchema {
    pub name: &'static str,
    pub kind: InspectorProtocolFieldKind,
    pub required: bool,
}

/// Immutable method metadata for one inspector protocol command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolMethodSchema {
    pub domain: InspectorDomain,
    pub name: &'static str,
    pub ordinal: u32,
    pub request_fields: &'static [InspectorProtocolFieldSchema],
    pub response_fields: &'static [InspectorProtocolFieldSchema],
    pub requires_enabled_agent: bool,
    pub may_have_async_response: bool,
}

impl InspectorProtocolMethodSchema {
    pub const fn request_fields(self) -> &'static [InspectorProtocolFieldSchema] {
        self.request_fields
    }

    pub const fn response_fields(self) -> &'static [InspectorProtocolFieldSchema] {
        self.response_fields
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_non_empty(self.name, InspectorValidationError::EmptyMethodName)?;
        validate_fields(self.request_fields)?;
        validate_fields(self.response_fields)
    }
}

/// Immutable event metadata for one frontend notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolEventSchema {
    pub domain: InspectorDomain,
    pub name: &'static str,
    pub ordinal: u32,
    pub payload_fields: &'static [InspectorProtocolFieldSchema],
}

impl InspectorProtocolEventSchema {
    pub const fn payload_fields(self) -> &'static [InspectorProtocolFieldSchema] {
        self.payload_fields
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_non_empty(self.name, InspectorValidationError::EmptyEventName)?;
        validate_fields(self.payload_fields)
    }
}

/// Immutable protocol domain schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolDomainSchema {
    pub domain: InspectorDomain,
    pub methods: &'static [InspectorProtocolMethodSchema],
    pub events: &'static [InspectorProtocolEventSchema],
    pub owner: InspectorSchemaOwner,
    pub mutation_authority: InspectorRegistryMutationAuthority,
    pub provenance: InspectorSchemaProvenance,
}

impl InspectorProtocolDomainSchema {
    pub const fn methods(self) -> &'static [InspectorProtocolMethodSchema] {
        self.methods
    }

    pub const fn events(self) -> &'static [InspectorProtocolEventSchema] {
        self.events
    }

    pub fn method_named(self, name: &str) -> Option<&'static InspectorProtocolMethodSchema> {
        self.methods
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn event_named(self, name: &str) -> Option<&'static InspectorProtocolEventSchema> {
        self.events
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_unique_method_schemas(self.domain, self.methods)?;
        validate_unique_event_schemas(self.domain, self.events)?;
        validate_non_empty(
            self.provenance.generator,
            InspectorValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            InspectorValidationError::EmptyProvenanceField,
        )
    }
}

/// Structural inspector descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InspectorValidationError {
    EmptyFieldName,
    EmptyMethodName,
    EmptyEventName,
    EmptyAgentImplementationName,
    EmptySessionName,
    EmptyConnectionKinds,
    EmptyProvenanceField,
    DuplicateProtocolDomain(InspectorDomain),
    DuplicateMethodName(InspectorDomain, &'static str),
    DuplicateMethodOrdinal(InspectorDomain, u32),
    DuplicateEventName(InspectorDomain, &'static str),
    DuplicateEventOrdinal(InspectorDomain, u32),
    DuplicateFieldName(&'static str),
    DuplicateAgentDomain(InspectorDomain),
    DuplicateSessionName(&'static str),
    DuplicateConnectionKind(InspectorFrontendConnectionKind),
    MethodDomainMismatch {
        expected: InspectorDomain,
        actual: InspectorDomain,
    },
    EventDomainMismatch {
        expected: InspectorDomain,
        actual: InspectorDomain,
    },
    AgentMethodDomainMismatch {
        expected: InspectorDomain,
        actual: InspectorDomain,
    },
    AgentEventDomainMismatch {
        expected: InspectorDomain,
        actual: InspectorDomain,
    },
    AgentOwnsDestroyedInstance,
    SessionRouterDispatcherMismatch,
    CommandRequiresNonzeroRequestId,
    CommandMethodNotInRegistry,
    CommandAgentNotInRegistry(InspectorDomain),
    FrontendEventNotInRegistry,
    FrontendEventSessionNotInRegistry,
    ResponseRequiresNonzeroRequestId,
    AsyncResponseNotAllowed,
    SuccessResponseCarriesError,
    ProtocolErrorMissingMessage,
    ResponseFieldCountMismatch {
        expected: usize,
        actual: usize,
    },
    ExecutionEventMissingCallFrame(InspectorExecutionEventKind),
    ExecutionPauseMissingReason,
    ExecutionPauseMissingCallFrames,
}

/// Reason passed to agent teardown callbacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorDisconnectReason {
    InspectedTargetDestroyed,
    InspectorDestroyed,
}

/// Agent lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorAgentState {
    Created,
    FrontendAttached,
    Enabled,
    Suspended,
    Disabled,
    Destroyed,
    ValuesDiscarded,
    AgentDiscarded,
}

/// Common agent descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorAgentDescriptor {
    pub domain: InspectorDomain,
    pub state: InspectorAgentState,
    pub backend_hook: Option<HostHookId>,
    pub owns_agent_instance: bool,
}

/// Immutable agent registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorAgentSchema {
    pub domain: InspectorDomain,
    pub implementation_name: &'static str,
    pub methods: &'static [InspectorProtocolMethodSchema],
    pub events: &'static [InspectorProtocolEventSchema],
    pub initial_state: InspectorAgentState,
    pub owns_agent_instance: bool,
    pub owner: InspectorSchemaOwner,
    pub mutation_authority: InspectorRegistryMutationAuthority,
    pub provenance: InspectorSchemaProvenance,
}

impl InspectorAgentSchema {
    pub const fn methods(self) -> &'static [InspectorProtocolMethodSchema] {
        self.methods
    }

    pub const fn events(self) -> &'static [InspectorProtocolEventSchema] {
        self.events
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_non_empty(
            self.implementation_name,
            InspectorValidationError::EmptyAgentImplementationName,
        )?;
        if self.initial_state == InspectorAgentState::Destroyed && self.owns_agent_instance {
            return Err(InspectorValidationError::AgentOwnsDestroyedInstance);
        }
        for method in self.methods {
            method.validate()?;
            if method.domain != self.domain {
                return Err(InspectorValidationError::AgentMethodDomainMismatch {
                    expected: self.domain,
                    actual: method.domain,
                });
            }
        }
        for event in self.events {
            event.validate()?;
            if event.domain != self.domain {
                return Err(InspectorValidationError::AgentEventDomainMismatch {
                    expected: self.domain,
                    actual: event.domain,
                });
            }
        }
        validate_non_empty(
            self.provenance.generator,
            InspectorValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            InspectorValidationError::EmptyProvenanceField,
        )
    }
}

/// Registry ownership state for `UniqueRef<InspectorAgentBase>` entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorAgentRegistryState {
    pub registered_agents: usize,
    pub frontend_backend_created: bool,
    pub last_disconnect_reason: Option<InspectorDisconnectReason>,
}

/// Inspector protocol message identity without JSON parsing.
///
/// This is frontend protocol correlation only; it does not own source, object,
/// or heap-cell identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InspectorRequestId(pub i64);

/// Backend dispatcher route for one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolCommand {
    pub request_id: InspectorRequestId,
    pub domain: InspectorDomain,
    pub method_ordinal: u32,
    pub requires_enabled_agent: bool,
    pub may_have_async_response: bool,
}

impl InspectorProtocolCommand {
    pub const fn from_method(
        request_id: InspectorRequestId,
        method: InspectorProtocolMethodSchema,
    ) -> Self {
        Self {
            request_id,
            domain: method.domain,
            method_ordinal: method.ordinal,
            requires_enabled_agent: method.requires_enabled_agent,
            may_have_async_response: method.may_have_async_response,
        }
    }

    pub fn validate(
        &self,
        registry: InspectorSchemaRegistry,
    ) -> Result<(), InspectorValidationError> {
        if self.request_id.0 == 0 {
            return Err(InspectorValidationError::CommandRequiresNonzeroRequestId);
        }
        let Some(domain) = registry.domain(self.domain) else {
            return Err(InspectorValidationError::CommandMethodNotInRegistry);
        };
        if domain
            .methods
            .iter()
            .any(|method| method.ordinal == self.method_ordinal)
        {
            Ok(())
        } else {
            Err(InspectorValidationError::CommandMethodNotInRegistry)
        }
    }
}

/// Frontend command identity after parsing but before backend dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolRequest {
    pub request_id: InspectorRequestId,
    pub domain: InspectorDomain,
    pub method_name: &'static str,
}

/// Pure route decision for a protocol command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorProtocolRoute {
    pub command: InspectorProtocolCommand,
    pub agent_initial_state: Option<InspectorAgentState>,
    pub request_field_count: usize,
    pub required_request_field_count: usize,
    pub response_field_count: usize,
}

impl InspectorProtocolRoute {
    pub fn command_response_outcome(
        self,
        response: InspectorCommandResponse,
    ) -> Result<InspectorCommandResponseOutcome, InspectorValidationError> {
        if self.command.request_id.0 == 0 {
            return Err(InspectorValidationError::ResponseRequiresNonzeroRequestId);
        }

        match response.kind {
            InspectorCommandResponseKind::Success => {
                if response.error_message_present {
                    return Err(InspectorValidationError::SuccessResponseCarriesError);
                }
                if response.field_count != self.response_field_count {
                    return Err(InspectorValidationError::ResponseFieldCountMismatch {
                        expected: self.response_field_count,
                        actual: response.field_count,
                    });
                }
            }
            InspectorCommandResponseKind::ProtocolError(_) => {
                if !response.error_message_present {
                    return Err(InspectorValidationError::ProtocolErrorMissingMessage);
                }
            }
            InspectorCommandResponseKind::AsyncPending => {
                if !self.command.may_have_async_response {
                    return Err(InspectorValidationError::AsyncResponseNotAllowed);
                }
            }
            InspectorCommandResponseKind::NotSent => {}
        }

        Ok(InspectorCommandResponseOutcome {
            request_id: self.command.request_id,
            domain: self.command.domain,
            method_ordinal: self.command.method_ordinal,
            kind: response.kind,
            should_send_response: matches!(
                response.kind,
                InspectorCommandResponseKind::Success
                    | InspectorCommandResponseKind::ProtocolError(_)
            ),
            awaits_async_response: response.kind == InspectorCommandResponseKind::AsyncPending,
            field_count: response.field_count,
            error_message_present: response.error_message_present,
        })
    }
}

/// Pure route decision for a frontend notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorFrontendEventRoute {
    pub event: InspectorFrontendEvent,
    pub payload_field_count: usize,
    pub required_payload_field_count: usize,
}

/// Common backend-dispatcher protocol error category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorProtocolErrorCode {
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    InternalError,
    ServerError,
}

/// Command response category after an agent returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorCommandResponseKind {
    Success,
    ProtocolError(InspectorProtocolErrorCode),
    AsyncPending,
    NotSent,
}

/// Non-transport command response record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorCommandResponse {
    pub kind: InspectorCommandResponseKind,
    pub field_count: usize,
    pub error_message_present: bool,
}

/// Semantic command outcome before frontend transport serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorCommandResponseOutcome {
    pub request_id: InspectorRequestId,
    pub domain: InspectorDomain,
    pub method_ordinal: u32,
    pub kind: InspectorCommandResponseKind,
    pub should_send_response: bool,
    pub awaits_async_response: bool,
    pub field_count: usize,
    pub error_message_present: bool,
}

/// Backend dispatcher state while routing frontend JSON messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorBackendDispatcherState {
    pub active: bool,
    pub current_request: Option<InspectorRequestId>,
    pub pending_protocol_error_count: usize,
    pub has_fallback_dispatcher: bool,
}

/// Frontend event descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorFrontendEvent {
    pub domain: InspectorDomain,
    pub event_ordinal: u32,
    pub session: InspectorSessionId,
}

/// Inspector session identity.
///
/// Sessions own frontend/backend routing state. They borrow VM/debugger state
/// through agent descriptors and must release retained remote objects
/// explicitly through object-group lifetime rules.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InspectorSessionId(pub u64);

/// Frontend channel state without transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorFrontendChannelState {
    Detached,
    Attaching,
    Attached,
    Detaching,
}

/// Frontend connection family tracked by `FrontendRouter`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorFrontendConnectionKind {
    Local,
    Remote,
}

/// Inspector session boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorSession {
    pub id: InspectorSessionId,
    pub channel_state: InspectorFrontendChannelState,
    pub has_backend_dispatcher: bool,
    pub has_frontend_router: bool,
    pub frontend_count: usize,
    pub connection_kind: Option<InspectorFrontendConnectionKind>,
}

/// Immutable session shape used by inspector controller setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorSessionSchema {
    pub name: &'static str,
    pub allowed_connection_kinds: &'static [InspectorFrontendConnectionKind],
    pub creates_backend_dispatcher: bool,
    pub creates_frontend_router: bool,
    pub retains_remote_objects: bool,
    pub owner: InspectorSchemaOwner,
    pub mutation_authority: InspectorRegistryMutationAuthority,
    pub provenance: InspectorSchemaProvenance,
}

impl InspectorSessionSchema {
    pub const fn allowed_connection_kinds(self) -> &'static [InspectorFrontendConnectionKind] {
        self.allowed_connection_kinds
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_non_empty(self.name, InspectorValidationError::EmptySessionName)?;
        if self.allowed_connection_kinds.is_empty() {
            return Err(InspectorValidationError::EmptyConnectionKinds);
        }
        validate_unique_connection_kinds(self.allowed_connection_kinds)?;
        if self.creates_backend_dispatcher != self.creates_frontend_router {
            return Err(InspectorValidationError::SessionRouterDispatcherMismatch);
        }
        validate_non_empty(
            self.provenance.generator,
            InspectorValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            InspectorValidationError::EmptyProvenanceField,
        )
    }
}

/// Builder for immutable inspector session schemas.
#[derive(Clone, Copy, Debug)]
pub struct InspectorSessionSchemaBuilder {
    schema: InspectorSessionSchema,
}

impl InspectorSessionSchemaBuilder {
    pub const fn new(
        name: &'static str,
        allowed_connection_kinds: &'static [InspectorFrontendConnectionKind],
        provenance: InspectorSchemaProvenance,
    ) -> Self {
        Self {
            schema: InspectorSessionSchema {
                name,
                allowed_connection_kinds,
                creates_backend_dispatcher: true,
                creates_frontend_router: true,
                retains_remote_objects: false,
                owner: InspectorSchemaOwner::InspectorController,
                mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
                provenance,
            },
        }
    }

    pub const fn backend_dispatcher(mut self, creates: bool) -> Self {
        self.schema.creates_backend_dispatcher = creates;
        self
    }

    pub const fn frontend_router(mut self, creates: bool) -> Self {
        self.schema.creates_frontend_router = creates;
        self
    }

    pub const fn retains_remote_objects(mut self, retains: bool) -> Self {
        self.schema.retains_remote_objects = retains;
        self
    }

    pub fn build(self) -> Result<InspectorSessionSchema, InspectorValidationError> {
        self.schema.validate()?;
        Ok(self.schema)
    }
}

/// Registry of immutable inspector protocol, session, and agent schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectorSchemaRegistry {
    pub protocol_domains: &'static [InspectorProtocolDomainSchema],
    pub agents: &'static [InspectorAgentSchema],
    pub sessions: &'static [InspectorSessionSchema],
}

impl InspectorSchemaRegistry {
    pub const fn new(
        protocol_domains: &'static [InspectorProtocolDomainSchema],
        agents: &'static [InspectorAgentSchema],
        sessions: &'static [InspectorSessionSchema],
    ) -> Self {
        Self {
            protocol_domains,
            agents,
            sessions,
        }
    }

    pub const fn protocol_domains(self) -> &'static [InspectorProtocolDomainSchema] {
        self.protocol_domains
    }

    pub const fn agents(self) -> &'static [InspectorAgentSchema] {
        self.agents
    }

    pub const fn sessions(self) -> &'static [InspectorSessionSchema] {
        self.sessions
    }

    pub fn domain(self, domain: InspectorDomain) -> Option<&'static InspectorProtocolDomainSchema> {
        self.protocol_domains
            .iter()
            .find(|descriptor| descriptor.domain == domain)
    }

    pub fn agent(self, domain: InspectorDomain) -> Option<&'static InspectorAgentSchema> {
        self.agents
            .iter()
            .find(|descriptor| descriptor.domain == domain)
    }

    pub fn session_named(self, name: &str) -> Option<&'static InspectorSessionSchema> {
        self.sessions
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn route_command(
        self,
        request: InspectorProtocolRequest,
    ) -> Result<InspectorProtocolRoute, InspectorValidationError> {
        self.validate()?;
        if request.request_id.0 == 0 {
            return Err(InspectorValidationError::CommandRequiresNonzeroRequestId);
        }

        let Some(domain) = self.domain(request.domain) else {
            return Err(InspectorValidationError::CommandMethodNotInRegistry);
        };
        let Some(method) = domain.method_named(request.method_name) else {
            return Err(InspectorValidationError::CommandMethodNotInRegistry);
        };
        let agent = self.agent(request.domain);
        if method.requires_enabled_agent && agent.is_none() {
            return Err(InspectorValidationError::CommandAgentNotInRegistry(
                request.domain,
            ));
        }

        Ok(InspectorProtocolRoute {
            command: InspectorProtocolCommand::from_method(request.request_id, *method),
            agent_initial_state: agent.map(|agent| agent.initial_state),
            request_field_count: method.request_fields.len(),
            required_request_field_count: method
                .request_fields
                .iter()
                .filter(|field| field.required)
                .count(),
            response_field_count: method.response_fields.len(),
        })
    }

    pub fn route_frontend_event(
        self,
        session_name: &str,
        session: InspectorSessionId,
        domain: InspectorDomain,
        event_name: &str,
    ) -> Result<InspectorFrontendEventRoute, InspectorValidationError> {
        self.validate()?;
        if self.session_named(session_name).is_none() {
            return Err(InspectorValidationError::FrontendEventSessionNotInRegistry);
        }
        let Some(domain_schema) = self.domain(domain) else {
            return Err(InspectorValidationError::FrontendEventNotInRegistry);
        };
        let Some(event) = domain_schema.event_named(event_name) else {
            return Err(InspectorValidationError::FrontendEventNotInRegistry);
        };

        Ok(InspectorFrontendEventRoute {
            event: InspectorFrontendEvent {
                domain,
                event_ordinal: event.ordinal,
                session,
            },
            payload_field_count: event.payload_fields.len(),
            required_payload_field_count: event
                .payload_fields
                .iter()
                .filter(|field| field.required)
                .count(),
        })
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        validate_unique_domains(self.protocol_domains)?;
        validate_unique_agents(self.agents)?;
        validate_unique_sessions(self.sessions)?;
        for domain in self.protocol_domains {
            domain.validate()?;
        }
        for agent in self.agents {
            agent.validate()?;
            if self.domain(agent.domain).is_none() {
                return Err(InspectorValidationError::DuplicateProtocolDomain(
                    agent.domain,
                ));
            }
        }
        for session in self.sessions {
            session.validate()?;
        }
        Ok(())
    }
}

/// Target kind exposed by inspector target discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorTargetType {
    Page,
    Frame,
    DedicatedWorker,
    ServiceWorker,
}

/// Target pause/connection state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorTargetState {
    pub target_type: InspectorTargetType,
    pub is_provisional: bool,
    pub is_paused: bool,
    pub has_resume_callback: bool,
    pub channel_state: InspectorFrontendChannelState,
}

/// Instrumentation event observed by inspector agents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectorInstrumentationKind {
    DidParseScript(DebuggerSourceId),
    FailedToParseScript,
    DidCreateNativeExecutable,
    WillEnterCallFrame,
    DidPause,
    DidContinue,
    DidQueueMicrotask,
    WillRunMicrotask,
    DidRunMicrotask,
    ConsoleMessage,
    HeapSnapshotRequested,
    WasmModuleRegistered(WasmModuleId),
    ApiExceptionReported,
    FrontendInitialized,
}

/// Instrumentation dispatch record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorInstrumentationEvent {
    pub kind: InspectorInstrumentationKind,
    pub source: Option<SourceProviderId>,
    pub code_block: Option<CodeBlockId>,
}

/// Inspector-local execution event class before protocol serialization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InspectorExecutionEventKind {
    ScriptParsed,
    ScriptParseFailed,
    NativeExecutableCreated,
    CallFrameEntered,
    Paused,
    Continued,
    MicrotaskQueued,
    MicrotaskStarted,
    MicrotaskFinished,
    ConsoleMessage,
    HeapSnapshotRequested,
    ApiExceptionReported,
    FrontendInitialized,
    WasmModuleRegistered,
}

/// Execution event observed by inspector agents without frontend transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorExecutionEventRecord {
    pub session: Option<InspectorSessionId>,
    pub kind: InspectorExecutionEventKind,
    pub instrumentation: InspectorInstrumentationKind,
    pub global_object: Option<GlobalObjectId>,
    pub source: Option<SourceProviderId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub call_frame: Option<DebuggerCallFrameDescriptor>,
    pub pause_reason: Option<DebuggerPauseReason>,
    pub call_frame_count: usize,
    pub timestamp_ticks: u64,
}

impl InspectorExecutionEventRecord {
    pub fn from_instrumentation(
        session: Option<InspectorSessionId>,
        event: InspectorInstrumentationEvent,
        global_object: Option<GlobalObjectId>,
    ) -> Self {
        Self {
            session,
            kind: inspector_execution_event_kind(event.kind),
            instrumentation: event.kind,
            global_object,
            source: event.source,
            code_block: event.code_block,
            bytecode_index: None,
            call_frame: None,
            pause_reason: None,
            call_frame_count: 0,
            timestamp_ticks: 0,
        }
    }

    pub const fn with_call_frame(mut self, frame: DebuggerCallFrameDescriptor) -> Self {
        self.call_frame = Some(frame);
        self.call_frame_count = 1;
        self
    }

    pub const fn with_pause(
        mut self,
        reason: DebuggerPauseReason,
        call_frame_count: usize,
    ) -> Self {
        self.pause_reason = Some(reason);
        self.call_frame_count = call_frame_count;
        self
    }

    pub const fn with_bytecode_index(mut self, bytecode_index: BytecodeIndex) -> Self {
        self.bytecode_index = Some(bytecode_index);
        self
    }

    pub const fn with_timestamp_ticks(mut self, ticks: u64) -> Self {
        self.timestamp_ticks = ticks;
        self
    }

    pub fn validate(self) -> Result<(), InspectorValidationError> {
        if let Some(frame) = self.call_frame {
            frame
                .validate()
                .map_err(|_| InspectorValidationError::ExecutionEventMissingCallFrame(self.kind))?;
        }
        if self.kind == InspectorExecutionEventKind::CallFrameEntered && self.call_frame.is_none() {
            return Err(InspectorValidationError::ExecutionEventMissingCallFrame(
                self.kind,
            ));
        }
        if self.kind == InspectorExecutionEventKind::Paused {
            if self.pause_reason.is_none() {
                return Err(InspectorValidationError::ExecutionPauseMissingReason);
            }
            if self.call_frame_count == 0 {
                return Err(InspectorValidationError::ExecutionPauseMissingCallFrames);
            }
        }
        Ok(())
    }
}

/// Inspector diagnostics assembled without protocol transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorDiagnosticReport {
    pub session: Option<InspectorSessionId>,
    pub execution_events: Vec<InspectorExecutionEventRecord>,
    pub heap_observations: Vec<InspectorHeapObservationRecord>,
    pub debugger_pause_visible: bool,
    pub tier_fallback_visible: bool,
    pub protocol_transport_required: bool,
}

impl InspectorDiagnosticReport {
    pub fn from_debugger_report(
        session: Option<InspectorSessionId>,
        global_object: Option<GlobalObjectId>,
        report: &DebuggerDiagnosticReport,
        heap_observations: Vec<InspectorHeapObservationRecord>,
        timestamp_ticks: u64,
    ) -> Result<Self, InspectorValidationError> {
        let call_frame = report.call_frames.first().copied();
        let mut event = InspectorExecutionEventRecord::from_instrumentation(
            session,
            InspectorInstrumentationEvent {
                kind: InspectorInstrumentationKind::DidPause,
                source: None,
                code_block: None,
            },
            global_object,
        )
        .with_pause(report.pause.reason, report.call_frames.len())
        .with_timestamp_ticks(timestamp_ticks);
        if let Some(frame) = call_frame {
            event = event.with_call_frame(frame);
        }
        event.validate()?;

        Ok(Self {
            session,
            execution_events: vec![event],
            heap_observations,
            debugger_pause_visible: report.pause.should_notify_clients,
            tier_fallback_visible: report.tier_fallback.is_some(),
            protocol_transport_required: false,
        })
    }
}

fn inspector_execution_event_kind(
    kind: InspectorInstrumentationKind,
) -> InspectorExecutionEventKind {
    match kind {
        InspectorInstrumentationKind::DidParseScript(_) => {
            InspectorExecutionEventKind::ScriptParsed
        }
        InspectorInstrumentationKind::FailedToParseScript => {
            InspectorExecutionEventKind::ScriptParseFailed
        }
        InspectorInstrumentationKind::DidCreateNativeExecutable => {
            InspectorExecutionEventKind::NativeExecutableCreated
        }
        InspectorInstrumentationKind::WillEnterCallFrame => {
            InspectorExecutionEventKind::CallFrameEntered
        }
        InspectorInstrumentationKind::DidPause => InspectorExecutionEventKind::Paused,
        InspectorInstrumentationKind::DidContinue => InspectorExecutionEventKind::Continued,
        InspectorInstrumentationKind::DidQueueMicrotask => {
            InspectorExecutionEventKind::MicrotaskQueued
        }
        InspectorInstrumentationKind::WillRunMicrotask => {
            InspectorExecutionEventKind::MicrotaskStarted
        }
        InspectorInstrumentationKind::DidRunMicrotask => {
            InspectorExecutionEventKind::MicrotaskFinished
        }
        InspectorInstrumentationKind::ConsoleMessage => InspectorExecutionEventKind::ConsoleMessage,
        InspectorInstrumentationKind::HeapSnapshotRequested => {
            InspectorExecutionEventKind::HeapSnapshotRequested
        }
        InspectorInstrumentationKind::WasmModuleRegistered(_) => {
            InspectorExecutionEventKind::WasmModuleRegistered
        }
        InspectorInstrumentationKind::ApiExceptionReported => {
            InspectorExecutionEventKind::ApiExceptionReported
        }
        InspectorInstrumentationKind::FrontendInitialized => {
            InspectorExecutionEventKind::FrontendInitialized
        }
    }
}

/// Debugger-agent breakpoint mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorBreakpointBinding {
    pub protocol_breakpoint_id: u64,
    pub debugger_breakpoint: DebuggerBreakpointId,
    pub source: Option<DebuggerSourceId>,
    pub resolved: bool,
    pub protocol_owned: bool,
}

/// Inspector-specific pause bookkeeping layered on top of `Debugger`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorPauseDetails {
    pub paused_global_object: Option<GlobalObjectId>,
    pub reason: Option<DebuggerPauseReason>,
    pub protocol_data_present: bool,
    pub current_call_stack_present: bool,
    pub should_dispatch_resumed_when_idle: bool,
}

/// Inspector-visible call frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorCallFrame {
    pub call_frame_id: u64,
    pub debugger_frame: DebuggerCallFrameDescriptor,
    pub scope_chain_length: u32,
}

/// Remote object descriptor returned by runtime/debugger domains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorRemoteObject {
    pub object: Option<ObjectId>,
    pub value: Option<RuntimeValue>,
    pub object_group: u32,
    pub return_by_value: bool,
}

/// Object-group retention controlled by the injected script.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorObjectGroup {
    pub group_name: String,
    pub retained_remote_object_count: usize,
    pub may_generate_preview: bool,
}

/// Injected script cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorInjectedScriptRecord {
    pub script_id: i32,
    pub global_object: Option<GlobalObjectId>,
    pub connected: bool,
    pub object_groups: Vec<InspectorObjectGroup>,
    pub has_event_value: bool,
    pub has_exception_value: bool,
}

/// Console message retention and frontend-delivery contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorConsoleMessage {
    pub source_ordinal: u32,
    pub type_ordinal: u32,
    pub level_ordinal: u32,
    pub source: Option<SourceProviderId>,
    pub global_object: Option<GlobalObjectId>,
    pub repeat_count: u32,
    pub argument_count: u32,
    pub call_stack_depth: u32,
    pub cleared: bool,
}

/// Evaluation authority supplied by `InspectorEnvironment`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorEvaluationAuthority {
    pub can_access_inspected_script_state: bool,
    pub has_function_call_handler: bool,
    pub has_evaluate_handler: bool,
    pub may_mute_console: bool,
}

/// Sampling-profiler lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingProfilerState {
    Unsupported,
    Available,
    Running,
    Paused,
    Stopped,
}

/// Sampling-profiler frame family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplingProfilerFrameKind {
    Executable,
    Wasm,
    Host,
    RegExp,
    Native,
    Unknown,
}

/// Sampling-profiler sample descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplingProfilerSample {
    pub frame_kind: SamplingProfilerFrameKind,
    pub source: Option<SourceProviderId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_offset: Option<u32>,
    pub timestamp_ticks: u64,
}

/// Type-profiler query key.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TypeProfilerQuery {
    pub source: DebuggerSourceId,
    pub divot: u32,
    pub function_return: bool,
}

/// Type-profiler location metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeProfilerLocation {
    pub query: TypeProfilerQuery,
    pub observed_type_set: u32,
    pub variable_id: u64,
}

/// Control-flow basic-block profiling range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlFlowBlockRange {
    pub source: DebuggerSourceId,
    pub start_offset: i32,
    pub end_offset: i32,
    pub has_executed: bool,
    pub execution_count: usize,
}

/// Script profiler agent state shared by sampling, type, and control-flow profilers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectorScriptProfilerState {
    pub sampling_state: SamplingProfilerState,
    pub type_profiler_enabled: bool,
    pub control_flow_profiler_enabled: bool,
    pub sample_count: usize,
    pub type_location_count: usize,
    pub basic_block_count: usize,
    pub active_evaluate_script: bool,
    pub tracking_frontend: bool,
}

/// Wasm debugger target exposed through inspector discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectorWasmDebuggerTarget {
    pub module: Option<WasmModuleId>,
    pub server_state: WasmDebugServerState,
    pub has_local_debugger: bool,
}

const INSPECTOR_SCHEMA_PROVENANCE: InspectorSchemaProvenance = InspectorSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/inspector/mod.rs",
    revision: 1,
};

pub const INSPECTOR_PROTOCOL_DOMAIN_SCHEMAS: &[InspectorProtocolDomainSchema] = &[
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Runtime,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Debugger,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Console,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Heap,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::ScriptProfiler,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Target,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Audit,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::WasmDebugger,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Page,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorProtocolDomainSchema {
        domain: InspectorDomain::Network,
        methods: &[],
        events: &[],
        owner: InspectorSchemaOwner::GeneratedProtocolJson,
        mutation_authority: InspectorRegistryMutationAuthority::GeneratedDataRefresh,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
];

pub const INSPECTOR_AGENT_SCHEMAS: &[InspectorAgentSchema] = &[
    InspectorAgentSchema {
        domain: InspectorDomain::Runtime,
        implementation_name: "RuntimeAgent",
        methods: &[],
        events: &[],
        initial_state: InspectorAgentState::Created,
        owns_agent_instance: true,
        owner: InspectorSchemaOwner::InspectorAgent,
        mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorAgentSchema {
        domain: InspectorDomain::Debugger,
        implementation_name: "DebuggerAgent",
        methods: &[],
        events: &[],
        initial_state: InspectorAgentState::Created,
        owns_agent_instance: true,
        owner: InspectorSchemaOwner::InspectorAgent,
        mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorAgentSchema {
        domain: InspectorDomain::Console,
        implementation_name: "ConsoleAgent",
        methods: &[],
        events: &[],
        initial_state: InspectorAgentState::Created,
        owns_agent_instance: true,
        owner: InspectorSchemaOwner::InspectorAgent,
        mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
    InspectorAgentSchema {
        domain: InspectorDomain::ScriptProfiler,
        implementation_name: "ScriptProfilerAgent",
        methods: &[],
        events: &[],
        initial_state: InspectorAgentState::Created,
        owns_agent_instance: true,
        owner: InspectorSchemaOwner::InspectorAgent,
        mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
        provenance: INSPECTOR_SCHEMA_PROVENANCE,
    },
];

pub const INSPECTOR_SESSION_CONNECTION_KINDS: &[InspectorFrontendConnectionKind] = &[
    InspectorFrontendConnectionKind::Local,
    InspectorFrontendConnectionKind::Remote,
];

pub const INSPECTOR_SESSION_SCHEMAS: &[InspectorSessionSchema] = &[InspectorSessionSchema {
    name: "frontend-session",
    allowed_connection_kinds: INSPECTOR_SESSION_CONNECTION_KINDS,
    creates_backend_dispatcher: true,
    creates_frontend_router: true,
    retains_remote_objects: true,
    owner: InspectorSchemaOwner::InspectorController,
    mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
    provenance: INSPECTOR_SCHEMA_PROVENANCE,
}];

pub const INSPECTOR_SCHEMA_REGISTRY: InspectorSchemaRegistry = InspectorSchemaRegistry {
    protocol_domains: INSPECTOR_PROTOCOL_DOMAIN_SCHEMAS,
    agents: INSPECTOR_AGENT_SCHEMAS,
    sessions: INSPECTOR_SESSION_SCHEMAS,
};

fn validate_non_empty(
    value: &'static str,
    error: InspectorValidationError,
) -> Result<(), InspectorValidationError> {
    if value.is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_fields(
    fields: &[InspectorProtocolFieldSchema],
) -> Result<(), InspectorValidationError> {
    for (index, field) in fields.iter().enumerate() {
        validate_non_empty(field.name, InspectorValidationError::EmptyFieldName)?;
        for other in fields.iter().skip(index + 1) {
            if field.name == other.name {
                return Err(InspectorValidationError::DuplicateFieldName(field.name));
            }
        }
    }
    Ok(())
}

fn validate_unique_method_schemas(
    domain: InspectorDomain,
    methods: &[InspectorProtocolMethodSchema],
) -> Result<(), InspectorValidationError> {
    for (index, method) in methods.iter().enumerate() {
        method.validate()?;
        if method.domain != domain {
            return Err(InspectorValidationError::MethodDomainMismatch {
                expected: domain,
                actual: method.domain,
            });
        }
        for other in methods.iter().skip(index + 1) {
            if method.name == other.name {
                return Err(InspectorValidationError::DuplicateMethodName(
                    domain,
                    method.name,
                ));
            }
            if method.ordinal == other.ordinal {
                return Err(InspectorValidationError::DuplicateMethodOrdinal(
                    domain,
                    method.ordinal,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_event_schemas(
    domain: InspectorDomain,
    events: &[InspectorProtocolEventSchema],
) -> Result<(), InspectorValidationError> {
    for (index, event) in events.iter().enumerate() {
        event.validate()?;
        if event.domain != domain {
            return Err(InspectorValidationError::EventDomainMismatch {
                expected: domain,
                actual: event.domain,
            });
        }
        for other in events.iter().skip(index + 1) {
            if event.name == other.name {
                return Err(InspectorValidationError::DuplicateEventName(
                    domain, event.name,
                ));
            }
            if event.ordinal == other.ordinal {
                return Err(InspectorValidationError::DuplicateEventOrdinal(
                    domain,
                    event.ordinal,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_domains(
    domains: &[InspectorProtocolDomainSchema],
) -> Result<(), InspectorValidationError> {
    for (index, domain) in domains.iter().enumerate() {
        for other in domains.iter().skip(index + 1) {
            if domain.domain == other.domain {
                return Err(InspectorValidationError::DuplicateProtocolDomain(
                    domain.domain,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_agents(agents: &[InspectorAgentSchema]) -> Result<(), InspectorValidationError> {
    for (index, agent) in agents.iter().enumerate() {
        for other in agents.iter().skip(index + 1) {
            if agent.domain == other.domain {
                return Err(InspectorValidationError::DuplicateAgentDomain(agent.domain));
            }
        }
    }
    Ok(())
}

fn validate_unique_sessions(
    sessions: &[InspectorSessionSchema],
) -> Result<(), InspectorValidationError> {
    for (index, session) in sessions.iter().enumerate() {
        for other in sessions.iter().skip(index + 1) {
            if session.name == other.name {
                return Err(InspectorValidationError::DuplicateSessionName(session.name));
            }
        }
    }
    Ok(())
}

fn validate_unique_connection_kinds(
    kinds: &[InspectorFrontendConnectionKind],
) -> Result<(), InspectorValidationError> {
    for (index, kind) in kinds.iter().enumerate() {
        for other in kinds.iter().skip(index + 1) {
            if kind == other {
                return Err(InspectorValidationError::DuplicateConnectionKind(*kind));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    const FIELD: InspectorProtocolFieldSchema = InspectorProtocolFieldSchema {
        name: "objectId",
        kind: InspectorProtocolFieldKind::String,
        required: true,
    };
    static METHOD_FIELDS: &[InspectorProtocolFieldSchema] = &[FIELD];
    static METHOD: InspectorProtocolMethodSchema = InspectorProtocolMethodSchema {
        domain: InspectorDomain::Runtime,
        name: "getProperties",
        ordinal: 1,
        request_fields: METHOD_FIELDS,
        response_fields: &[],
        requires_enabled_agent: true,
        may_have_async_response: false,
    };
    static EVENT: InspectorProtocolEventSchema = InspectorProtocolEventSchema {
        domain: InspectorDomain::Runtime,
        name: "executionContextCreated",
        ordinal: 2,
        payload_fields: METHOD_FIELDS,
    };
    static METHODS: &[InspectorProtocolMethodSchema] = &[METHOD];
    static EVENTS: &[InspectorProtocolEventSchema] = &[EVENT];
    static DOMAIN: InspectorProtocolDomainSchema = InspectorProtocolDomainSchema {
        domain: InspectorDomain::Runtime,
        methods: METHODS,
        events: EVENTS,
        owner: InspectorSchemaOwner::TestFixture,
        mutation_authority: InspectorRegistryMutationAuthority::CrateInitialization,
        provenance: InspectorSchemaProvenance::new("test", "inspector/mod.rs", 1),
    };
    static AGENT: InspectorAgentSchema = InspectorAgentSchema {
        domain: InspectorDomain::Runtime,
        implementation_name: "RuntimeAgent",
        methods: METHODS,
        events: EVENTS,
        initial_state: InspectorAgentState::Created,
        owns_agent_instance: true,
        owner: InspectorSchemaOwner::TestFixture,
        mutation_authority: InspectorRegistryMutationAuthority::SessionBootstrap,
        provenance: InspectorSchemaProvenance::new("test", "inspector/mod.rs", 1),
    };
    static TEST_DOMAINS: &[InspectorProtocolDomainSchema] = &[DOMAIN];
    static TEST_AGENTS: &[InspectorAgentSchema] = &[AGENT];
    static REGISTRY: InspectorSchemaRegistry =
        InspectorSchemaRegistry::new(TEST_DOMAINS, TEST_AGENTS, INSPECTOR_SESSION_SCHEMAS);

    #[test]
    fn validates_builtin_inspector_registry() {
        assert_eq!(INSPECTOR_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn builds_command_from_method_schema() {
        let command = InspectorProtocolCommand::from_method(InspectorRequestId(7), METHOD);

        assert_eq!(command.validate(REGISTRY), Ok(()));
    }

    #[test]
    fn rejects_zero_request_id() {
        let command = InspectorProtocolCommand::from_method(InspectorRequestId(0), METHOD);

        assert_eq!(
            command.validate(REGISTRY),
            Err(InspectorValidationError::CommandRequiresNonzeroRequestId)
        );
    }

    #[test]
    fn routes_command_by_domain_and_method_name() {
        let route = REGISTRY
            .route_command(InspectorProtocolRequest {
                request_id: InspectorRequestId(9),
                domain: InspectorDomain::Runtime,
                method_name: "getProperties",
            })
            .expect("route");

        assert_eq!(route.command.method_ordinal, 1);
        assert_eq!(
            route.agent_initial_state,
            Some(InspectorAgentState::Created)
        );
        assert_eq!(route.request_field_count, 1);
        assert_eq!(route.required_request_field_count, 1);
    }

    #[test]
    fn rejects_missing_command_route() {
        assert_eq!(
            REGISTRY.route_command(InspectorProtocolRequest {
                request_id: InspectorRequestId(9),
                domain: InspectorDomain::Runtime,
                method_name: "missing",
            }),
            Err(InspectorValidationError::CommandMethodNotInRegistry)
        );
    }

    #[test]
    fn routes_frontend_event_by_schema_row() {
        let route = REGISTRY
            .route_frontend_event(
                "frontend-session",
                InspectorSessionId(4),
                InspectorDomain::Runtime,
                "executionContextCreated",
            )
            .expect("route");

        assert_eq!(route.event.event_ordinal, 2);
        assert_eq!(route.payload_field_count, 1);
    }

    #[test]
    fn command_response_semantics_accept_success_field_count() {
        let route = REGISTRY
            .route_command(InspectorProtocolRequest {
                request_id: InspectorRequestId(9),
                domain: InspectorDomain::Runtime,
                method_name: "getProperties",
            })
            .expect("route");

        let outcome = route
            .command_response_outcome(InspectorCommandResponse {
                kind: InspectorCommandResponseKind::Success,
                field_count: 0,
                error_message_present: false,
            })
            .expect("response outcome");

        assert!(outcome.should_send_response);
        assert!(!outcome.awaits_async_response);
    }

    #[test]
    fn command_response_semantics_reject_success_with_error() {
        let route = REGISTRY
            .route_command(InspectorProtocolRequest {
                request_id: InspectorRequestId(9),
                domain: InspectorDomain::Runtime,
                method_name: "getProperties",
            })
            .expect("route");

        assert_eq!(
            route.command_response_outcome(InspectorCommandResponse {
                kind: InspectorCommandResponseKind::Success,
                field_count: 0,
                error_message_present: true,
            }),
            Err(InspectorValidationError::SuccessResponseCarriesError)
        );
    }

    #[test]
    fn command_response_semantics_reject_async_for_sync_method() {
        let route = REGISTRY
            .route_command(InspectorProtocolRequest {
                request_id: InspectorRequestId(9),
                domain: InspectorDomain::Runtime,
                method_name: "getProperties",
            })
            .expect("route");

        assert_eq!(
            route.command_response_outcome(InspectorCommandResponse {
                kind: InspectorCommandResponseKind::AsyncPending,
                field_count: 0,
                error_message_present: false,
            }),
            Err(InspectorValidationError::AsyncResponseNotAllowed)
        );
    }

    #[test]
    fn records_inspector_execution_event_from_instrumentation() {
        let record = InspectorExecutionEventRecord::from_instrumentation(
            Some(InspectorSessionId(1)),
            InspectorInstrumentationEvent {
                kind: InspectorInstrumentationKind::DidPause,
                source: Some(SourceProviderId(2)),
                code_block: Some(CodeBlockId(CellId(3))),
            },
            Some(GlobalObjectId(ObjectId(CellId(4)))),
        )
        .with_pause(DebuggerPauseReason::DebuggerStatement, 1)
        .with_bytecode_index(BytecodeIndex::from_offset(12))
        .with_timestamp_ticks(99);

        assert_eq!(record.validate(), Ok(()));
        assert_eq!(record.kind, InspectorExecutionEventKind::Paused);
        assert_eq!(record.call_frame_count, 1);
    }

    #[test]
    fn rejects_pause_execution_event_without_reason() {
        let record = InspectorExecutionEventRecord::from_instrumentation(
            None,
            InspectorInstrumentationEvent {
                kind: InspectorInstrumentationKind::DidPause,
                source: None,
                code_block: None,
            },
            None,
        );

        assert_eq!(
            record.validate(),
            Err(InspectorValidationError::ExecutionPauseMissingReason)
        );
    }

    #[test]
    fn diagnostic_report_adapts_debugger_pause_without_transport() {
        let debugger_report = DebuggerDiagnosticReport {
            pause: crate::debugger::DebuggerPauseSemanticOutcome {
                reason: DebuggerPauseReason::DebuggerStatement,
                should_pause: true,
                should_notify_clients: true,
                exposes_call_frames: true,
                consumes_step: false,
                breakpoint: None,
            },
            call_frames: vec![DebuggerCallFrameDescriptor {
                frame: Some(crate::runtime::CallFrameId(1)),
                stack_frame: Some(crate::runtime::StackFrameId(2)),
                caller: None,
                kind: crate::debugger::DebuggerCallFrameKind::Function,
                source: None,
                position: crate::debugger::DebuggerPosition { line: 1, column: 0 },
                lexical_scope: None,
                this_object: None,
                is_tail_deleted: false,
                is_valid: true,
            }],
            tier_fallback: Some(
                crate::debugger::DebuggerTierFallbackDiagnostic::from_record(
                    crate::jit::TierFallbackResultRecord {
                        owner: CodeBlockId(CellId(9)),
                        from_tier: crate::jit::JitType::Baseline,
                        attempted_tier: crate::jit::JitType::Dfg,
                        reason: crate::jit::TierFallbackReason::UnsupportedTier,
                        target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
                        bytecode_index: Some(BytecodeIndex::from_offset(3)),
                        resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
                        preserves_profile: true,
                        should_count_invalidation: true,
                        clears_active_request: true,
                    },
                ),
            ),
            invalid_frame_count: 0,
        };

        let report = InspectorDiagnosticReport::from_debugger_report(
            Some(InspectorSessionId(2)),
            Some(GlobalObjectId(crate::runtime::ObjectId(CellId(3)))),
            &debugger_report,
            vec![],
            42,
        )
        .expect("inspector diagnostics");

        assert_eq!(report.execution_events.len(), 1);
        assert!(report.debugger_pause_visible);
        assert!(report.tier_fallback_visible);
        assert!(!report.protocol_transport_required);
    }
}
