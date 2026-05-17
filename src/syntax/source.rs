use std::sync::Arc;

/// Immutable source storage plus host-visible origin metadata.
///
/// This mirrors the role of JSC's `SourceProvider`: source storage is stable
/// for the whole parse, while URL, taint, source type, and directive metadata
/// are host/debugger-visible. Bytecode cache callbacks and provider locking are
/// intentionally not modeled here because they belong to VM/embedder ownership,
/// not syntax-front-end ownership.
#[derive(Clone, Debug)]
pub struct SourceProvider {
    origin: SourceOrigin,
    text: SourceText,
    source_type: SourceProviderSourceType,
}

impl SourceProvider {
    pub fn new(origin: SourceOrigin, text: SourceText) -> Self {
        Self {
            origin,
            text,
            source_type: SourceProviderSourceType::Program,
        }
    }

    pub fn with_source_type(
        origin: SourceOrigin,
        text: SourceText,
        source_type: SourceProviderSourceType,
    ) -> Self {
        Self {
            origin,
            text,
            source_type,
        }
    }

    pub fn origin(&self) -> &SourceOrigin {
        &self.origin
    }

    pub fn text(&self) -> &SourceText {
        &self.text
    }

    pub fn source_type(&self) -> SourceProviderSourceType {
        self.source_type
    }

    pub fn encoding(&self) -> SourceEncoding {
        self.text.encoding()
    }
}

/// Source bytes in the encoding specialization used by the lexer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceText {
    Latin1(Vec<u8>),
    Utf16(Vec<u16>),
}

impl SourceText {
    pub fn encoding(&self) -> SourceEncoding {
        match self {
            Self::Latin1(_) => SourceEncoding::Latin1,
            Self::Utf16(_) => SourceEncoding::Utf16,
        }
    }

    /// Length in the storage unit consumed by the selected lexer
    /// specialization: bytes for Latin1 and 16-bit code units for UTF-16.
    pub fn unit_len(&self) -> u32 {
        match self {
            Self::Latin1(text) => text.len(),
            Self::Utf16(text) => text.len(),
        }
        .try_into()
        .unwrap_or(u32::MAX)
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Latin1(text) => text.is_empty(),
            Self::Utf16(text) => text.is_empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceProviderSourceType {
    Program,
    Module,
    WebAssembly,
    Json,
    ImportMap,
}

impl SourceProviderSourceType {
    pub fn is_module_type(self) -> bool {
        matches!(self, Self::Module | Self::Json)
    }
}

/// Host and debugger-facing source identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceOrigin {
    /// Canonical origin URL used for relative module paths and diagnostics.
    pub url: Option<String>,
    /// Display URL. JSC keeps this separate from `SourceOrigin` because it may
    /// come from embedder-facing sourceURL data rather than the actual origin.
    pub source_url: Option<String>,
    pub pre_redirect_url: Option<String>,
    pub source_url_directive: Option<String>,
    pub source_mapping_url_directive: Option<String>,
    pub taint: SourceTaint,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceTaint {
    #[default]
    Untainted,
    PotentiallyTainted,
    Tainted,
}

/// Ranged parser view into a provider.
///
/// `SourceCode` shares immutable provider storage and carries the line/column
/// offset used when parsing function bodies, module records, direct eval, or
/// cached source slices. All spans stored by the lexer/parser are relative to
/// this view and must be translated through `SourceBoundary` before surfacing
/// as user diagnostics.
#[derive(Clone, Debug)]
pub struct SourceCode {
    provider: Arc<SourceProvider>,
    range: SourceSpan,
    first_line: u32,
    start_column: u32,
}

impl SourceCode {
    pub fn new(provider: Arc<SourceProvider>, range: SourceSpan) -> Self {
        Self {
            provider,
            range,
            first_line: 1,
            start_column: 0,
        }
    }

    pub fn with_start_position(
        provider: Arc<SourceProvider>,
        range: SourceSpan,
        first_line: u32,
        start_column: u32,
    ) -> Self {
        Self {
            provider,
            range,
            first_line,
            start_column,
        }
    }

    pub fn provider(&self) -> &Arc<SourceProvider> {
        &self.provider
    }

    pub fn range(&self) -> SourceSpan {
        self.range
    }

    pub fn first_line(&self) -> u32 {
        self.first_line
    }

    pub fn start_column(&self) -> u32 {
        self.start_column
    }

    pub fn boundary(&self, span: SourceSpan) -> SourceBoundary {
        SourceBoundary {
            span,
            origin: self.provider.origin().clone(),
            source_type: self.provider.source_type(),
            encoding: self.provider.encoding(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceEncoding {
    Latin1,
    Utf16,
}

/// Stable byte/code-unit offset into a `SourceCode` range.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourcePosition(pub u32);

/// Half-open source range. The unit is determined by `SourceEncoding`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    pub fn new(start: SourcePosition, end: SourcePosition) -> Self {
        Self { start, end }
    }

    pub fn at(position: SourcePosition) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn unit_len(self) -> u32 {
        self.end.0.saturating_sub(self.start.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LineColumn {
    pub line: u32,
    pub column: u32,
}

/// Diagnostic-ready source boundary.
///
/// The parser should carry compact `SourceSpan`s internally. This richer value
/// is the boundary where diagnostics, debugger hooks, module analysis errors,
/// and embedder callbacks receive source identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBoundary {
    pub span: SourceSpan,
    pub origin: SourceOrigin,
    pub source_type: SourceProviderSourceType,
    pub encoding: SourceEncoding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub boundary: SourceBoundary,
    pub kind: DiagnosticKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Note,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    Lexical,
    Syntax,
    EarlyError,
    ModuleResolution,
    SourceDirective,
}

/// Sink boundary for parser-owned diagnostics.
///
/// Implementations may collect, stream to inspector tooling, or convert into
/// VM errors. The syntax module only names that boundary.
pub trait DiagnosticSink {
    fn report(&mut self, diagnostic: Diagnostic);
}
