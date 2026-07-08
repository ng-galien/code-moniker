use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{Lang, kinds};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::linkage::binding::{
	ExternalOrigin, ReferenceLinkageDecision, ResolutionScope, UnknownReason,
};
use crate::linkage::catalog::CandidateCatalog;
use crate::linkage::catalog::{SymbolOrdinal, SymbolSet};
use crate::snapshot::{RecordTable, ReferenceRecord};
use crate::source::{CodeIndexMaterial, IndexedSourceFile};

pub(super) struct LombokSemantics<'a> {
	material: &'a CodeIndexMaterial,
	candidates: &'a CandidateCatalog,
	annotated_types: FxHashMap<Moniker, TypeAnnotations>,
	type_aliases: FxHashMap<Moniker, Moniker>,
	fields: FxHashMap<(Moniker, Vec<u8>), FieldInfo>,
}

impl<'a> LombokSemantics<'a> {
	pub(super) fn build(
		material: &'a CodeIndexMaterial,
		candidates: &'a CandidateCatalog,
		references: &RecordTable<ReferenceRecord>,
	) -> Self {
		let mut semantics = Self {
			material,
			candidates,
			annotated_types: FxHashMap::default(),
			type_aliases: FxHashMap::default(),
			fields: FxHashMap::default(),
		};
		let field_annotations = semantics.index_annotations(references);
		if semantics.needs_accessor_index(&field_annotations) {
			semantics.index_types_and_fields(field_annotations);
		}
		semantics
	}

	pub(super) fn is_empty(&self) -> bool {
		self.annotated_types.is_empty() && self.fields.is_empty()
	}

	pub(super) fn resolve_reference(
		&self,
		decision: &ReferenceLinkageDecision,
		references: &RecordTable<ReferenceRecord>,
	) -> Option<ReferenceLinkageDecision> {
		let reference_idx = match decision {
			ReferenceLinkageDecision::Unknown {
				reason: UnknownReason::NoCandidate,
				reference_idx,
				..
			}
			| ReferenceLinkageDecision::External { reference_idx, .. } => *reference_idx,
			_ => return None,
		};
		let reference = &references[reference_idx];
		self.resolve_logger_call(reference_idx, reference)
			.or_else(|| self.resolve_accessor_call(reference_idx, reference))
	}

	fn resolve_accessor_call(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
	) -> Option<ReferenceLinkageDecision> {
		if !matches!(reference.kind.as_str(), "method_call" | "calls") {
			return None;
		}
		let owner = callable_owner(self.material.reference_target(&reference.id)?)?;
		let owner = self.type_aliases.get(&owner)?;
		let accessor = AccessorCall::from_reference(reference)?;
		let field = self.fields.get(&(owner.clone(), accessor.field_name))?;
		if !self.supports_accessor(owner, field, accessor.kind) {
			return None;
		}
		if !field.supports_accessor(accessor.kind) {
			return None;
		}
		Some(ReferenceLinkageDecision::resolved(
			ResolutionScope::Injected,
			reference_idx,
			reference.id.clone(),
			SymbolSet::from_symbol(field.symbol),
		))
	}

	fn supports_accessor(
		&self,
		owner: &Moniker,
		field: &FieldInfo,
		accessor: AccessorKind,
	) -> bool {
		self.annotated_types
			.get(owner)
			.is_some_and(|annotations| annotations.supports(accessor))
			|| field.annotations.supports(accessor)
	}

	fn resolve_logger_call(
		&self,
		reference_idx: usize,
		reference: &ReferenceRecord,
	) -> Option<ReferenceLinkageDecision> {
		if reference.kind != "method_call" || reference.receiver.as_deref() != Some("log") {
			return None;
		}
		let source = self.material.symbol_moniker(&reference.source_symbol)?;
		let annotation = self.logger_annotation_for(source)?;
		let logger_type = annotation.logger_type()?;
		let target = method_target(
			&external_type_target(source.as_view().project(), logger_type),
			reference.call_name.as_deref()?,
			reference.call_arity,
		);
		Some(ReferenceLinkageDecision::external_target(
			ExternalOrigin::Injected,
			reference_idx,
			reference.id.clone(),
			target,
		))
	}

	fn logger_annotation_for(&self, source: &Moniker) -> Option<LombokLogBackend> {
		if let Some(logger) = self
			.annotated_types
			.get(source)
			.and_then(|annotations| annotations.logger)
		{
			return Some(logger);
		}
		let mut owner = source.parent();
		while let Some(current) = owner {
			if let Some(logger) = self
				.annotated_types
				.get(&current)
				.and_then(|annotations| annotations.logger)
			{
				return Some(logger);
			}
			owner = current.parent();
		}
		None
	}

	fn needs_accessor_index(
		&self,
		field_annotations: &FxHashMap<(Moniker, Vec<u8>), TypeAnnotations>,
	) -> bool {
		self.annotated_types
			.values()
			.any(TypeAnnotations::supports_any_accessor)
			|| field_annotations
				.values()
				.any(TypeAnnotations::supports_any_accessor)
	}

	fn index_types_and_fields(
		&mut self,
		field_annotations: FxHashMap<(Moniker, Vec<u8>), TypeAnnotations>,
	) {
		for (file_idx, file) in self.material.files.iter().enumerate() {
			let facts = JavaSymbolFacts::from_file(file_idx, file, self.candidates);
			self.type_aliases.extend(facts.type_aliases);
			self.fields.extend(facts.fields);
		}
		for (key, annotations) in field_annotations {
			if let Some(field) = self.fields.get_mut(&key) {
				field.annotations.merge(annotations);
			}
		}
	}

	fn index_annotations(
		&mut self,
		references: &RecordTable<ReferenceRecord>,
	) -> FxHashMap<(Moniker, Vec<u8>), TypeAnnotations> {
		let mut field_annotations = FxHashMap::<(Moniker, Vec<u8>), TypeAnnotations>::default();
		for reference in references.iter() {
			if reference.kind != "annotates" {
				continue;
			}
			let Some(target) = self.material.reference_target(&reference.id) else {
				continue;
			};
			let Some(annotation) = lombok_annotation(target) else {
				continue;
			};
			let Some(source) = self.material.symbol_moniker(&reference.source_symbol) else {
				continue;
			};
			if is_java_type_moniker(source) {
				self.annotated_types
					.entry(source.clone())
					.or_default()
					.add(annotation);
			} else if let Some(key) = field_key(source) {
				field_annotations.entry(key).or_default().add(annotation);
			}
		}
		field_annotations
	}
}

#[derive(Default)]
struct JavaSymbolFacts {
	type_aliases: FxHashMap<Moniker, Moniker>,
	fields: FxHashMap<(Moniker, Vec<u8>), FieldInfo>,
}

impl JavaSymbolFacts {
	fn from_file(file_idx: usize, file: &IndexedSourceFile, candidates: &CandidateCatalog) -> Self {
		if file.lang != Lang::Java {
			return Self::default();
		}
		file.graph
			.defs()
			.enumerate()
			.fold(Self::default(), |mut facts, (def_idx, def)| {
				if is_java_type_kind(&def.kind) {
					facts.add_type_aliases(&def.moniker);
				} else if def.kind == kinds::FIELD
					&& let Some(owner) = field_owner(file, def.parent)
					&& let Some(symbol) = candidates.symbol_at(file_idx, def_idx)
				{
					facts.add_field(owner, &def.moniker, symbol, &def.signature);
				}
				facts
			})
	}

	fn add_type_aliases(&mut self, owner: &Moniker) {
		self.type_aliases.insert(owner.clone(), owner.clone());
		self.type_aliases
			.insert(path_alias_for_type(owner), owner.clone());
	}

	fn add_field(
		&mut self,
		owner: &Moniker,
		field: &Moniker,
		symbol: SymbolOrdinal,
		signature: &[u8],
	) {
		if is_java_type_moniker(owner) {
			self.fields.insert(
				(owner.clone(), field_name(field).to_vec()),
				FieldInfo::new(symbol, signature),
			);
		}
	}
}

struct FieldInfo {
	symbol: SymbolOrdinal,
	annotations: TypeAnnotations,
	facts: FieldFacts,
}

impl FieldInfo {
	fn new(symbol: SymbolOrdinal, signature: &[u8]) -> Self {
		Self {
			symbol,
			annotations: TypeAnnotations::default(),
			facts: FieldFacts::from_signature(signature),
		}
	}

	fn supports_accessor(&self, accessor: AccessorKind) -> bool {
		match accessor {
			AccessorKind::Getter { boolean_prefix } => {
				!boolean_prefix || self.facts.primitive_boolean
			}
			AccessorKind::Setter => !self.facts.final_field,
			AccessorKind::Wither => true,
		}
	}
}

#[derive(Default)]
struct FieldFacts {
	final_field: bool,
	primitive_boolean: bool,
}

impl FieldFacts {
	fn from_signature(signature: &[u8]) -> Self {
		let signature = trim_ascii(signature);
		let (final_field, ty) = signature
			.strip_prefix(b"final ")
			.map(|ty| (true, trim_ascii(ty)))
			.unwrap_or((false, signature));
		Self {
			final_field,
			primitive_boolean: ty == b"boolean",
		}
	}
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
	let start = bytes
		.iter()
		.position(|byte| !byte.is_ascii_whitespace())
		.unwrap_or(bytes.len());
	let end = bytes
		.iter()
		.rposition(|byte| !byte.is_ascii_whitespace())
		.map(|idx| idx + 1)
		.unwrap_or(start);
	&bytes[start..end]
}

fn field_owner(file: &IndexedSourceFile, parent_idx: Option<usize>) -> Option<&Moniker> {
	parent_idx.map(|idx| &file.graph.def_at(idx).moniker)
}

fn field_key(moniker: &Moniker) -> Option<(Moniker, Vec<u8>)> {
	moniker.parent().zip(
		moniker
			.as_view()
			.segments()
			.last()
			.filter(|segment| segment.kind == kinds::FIELD)
			.map(|segment| segment.name.to_vec()),
	)
}

#[derive(Default)]
struct TypeAnnotations {
	logger: Option<LombokLogBackend>,
	effects: FxHashSet<LombokEffect>,
}

impl TypeAnnotations {
	fn add(&mut self, annotation: &'static LombokAnnotation) {
		match annotation.effect {
			LombokEffect::Logger(backend) => self.logger = Some(backend),
			effect => {
				self.effects.insert(effect);
			}
		}
	}

	fn merge(&mut self, other: Self) {
		if self.logger.is_none() {
			self.logger = other.logger;
		}
		self.effects.extend(other.effects);
	}

	fn supports_any_accessor(&self) -> bool {
		self.effects.contains(&LombokEffect::Getter)
			|| self.effects.contains(&LombokEffect::Setter)
			|| self.effects.contains(&LombokEffect::Data)
			|| self.effects.contains(&LombokEffect::Value)
			|| self.effects.contains(&LombokEffect::With)
	}

	fn supports(&self, accessor: AccessorKind) -> bool {
		match accessor {
			AccessorKind::Getter { .. } => {
				self.effects.contains(&LombokEffect::Getter)
					|| self.effects.contains(&LombokEffect::Data)
					|| self.effects.contains(&LombokEffect::Value)
			}
			AccessorKind::Setter => {
				self.effects.contains(&LombokEffect::Setter)
					|| self.effects.contains(&LombokEffect::Data)
			}
			AccessorKind::Wither => self.effects.contains(&LombokEffect::With),
		}
	}
}

struct AccessorCall {
	kind: AccessorKind,
	field_name: Vec<u8>,
}

impl AccessorCall {
	fn from_reference(reference: &ReferenceRecord) -> Option<Self> {
		let call = reference.call_name.as_deref()?;
		let arity = reference.call_arity?;
		let (kind, property) = if arity == 0 {
			call.strip_prefix("get")
				.map(|name| {
					(
						AccessorKind::Getter {
							boolean_prefix: false,
						},
						name,
					)
				})
				.or_else(|| {
					call.strip_prefix("is").map(|name| {
						(
							AccessorKind::Getter {
								boolean_prefix: true,
							},
							name,
						)
					})
				})?
		} else if arity == 1 {
			call.strip_prefix("set")
				.map(|name| (AccessorKind::Setter, name))
				.or_else(|| {
					call.strip_prefix("with")
						.map(|name| (AccessorKind::Wither, name))
				})?
		} else {
			return None;
		};
		if property.is_empty() {
			return None;
		}
		Some(Self {
			kind,
			field_name: decapitalize_java_property(property),
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum AccessorKind {
	Getter { boolean_prefix: bool },
	Setter,
	Wither,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LombokEffect {
	Logger(LombokLogBackend),
	Getter,
	Setter,
	Data,
	Value,
	With,
	Builder,
	Constructor,
	ObjectMethods,
	NullCheck,
	Cleanup,
	Concurrency,
	ExceptionFlow,
	Delegate,
	FieldNames,
	FieldDefaults,
	UtilityClass,
	ExtensionMethod,
	GeneratedMarker,
	ConfigMarker,
	Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LombokLogBackend {
	JavaUtil,
	Commons,
	Log4j,
	Log4j2,
	Slf4j,
	XSlf4j,
	JBoss,
	Flogger,
	Custom,
}

impl LombokLogBackend {
	fn logger_type(self) -> Option<&'static [&'static str]> {
		match self {
			Self::JavaUtil => Some(&["java", "util", "logging", "Logger"]),
			Self::Commons => Some(&["org", "apache", "commons", "logging", "Log"]),
			Self::Log4j => Some(&["org", "apache", "log4j", "Logger"]),
			Self::Log4j2 => Some(&["org", "apache", "logging", "log4j", "Logger"]),
			Self::Slf4j => Some(&["org", "slf4j", "Logger"]),
			Self::XSlf4j => Some(&["org", "slf4j", "ext", "XLogger"]),
			Self::JBoss => Some(&["org", "jboss", "logging", "Logger"]),
			Self::Flogger => Some(&["com", "google", "common", "flogger", "FluentLogger"]),
			Self::Custom => None,
		}
	}
}

struct LombokAnnotation {
	name: &'static str,
	path: &'static [&'static str],
	effect: LombokEffect,
}

fn lombok_annotation(target: &Moniker) -> Option<&'static LombokAnnotation> {
	let path = java_package_path(target)?;
	LOMBOK_ANNOTATIONS.iter().find(|annotation| {
		annotation.path == path.as_slice()
			&& target
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == annotation.name.as_bytes())
	})
}

fn java_package_path(target: &Moniker) -> Option<Vec<&str>> {
	let mut in_java = false;
	let mut packages = Vec::new();
	for segment in target.as_view().segments() {
		if segment.kind == kinds::LANG && segment.name == b"java" {
			in_java = true;
			continue;
		}
		if !in_java {
			continue;
		}
		if segment.kind == kinds::PACKAGE {
			packages.push(std::str::from_utf8(segment.name).ok()?);
			continue;
		}
		break;
	}
	(packages.first().copied() == Some("lombok")).then_some(packages)
}

fn is_java_type_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::CLASS | kinds::INTERFACE | kinds::RECORD | kinds::ENUM | kinds::ANNOTATION_TYPE
	)
}

fn is_java_type_moniker(moniker: &Moniker) -> bool {
	moniker
		.as_view()
		.segments()
		.last()
		.is_some_and(|segment| is_java_type_kind(segment.kind))
}

fn path_alias_for_type(moniker: &Moniker) -> Moniker {
	let view = moniker.as_view();
	let segments = view.segments().collect::<Vec<_>>();
	let mut builder = MonikerBuilder::new();
	builder.project(view.project());
	for (idx, segment) in segments.iter().enumerate() {
		let kind = if idx + 1 == segments.len() && is_java_type_kind(segment.kind) {
			kinds::PATH
		} else {
			segment.kind
		};
		builder.segment(kind, segment.name);
	}
	builder.build()
}

fn field_name(moniker: &Moniker) -> &[u8] {
	moniker
		.as_view()
		.segments()
		.last()
		.map(|segment| segment.name)
		.unwrap_or_default()
}

fn callable_owner(target: &Moniker) -> Option<Moniker> {
	let last = target.as_view().segments().last()?;
	if matches!(last.kind, kinds::METHOD | kinds::CONSTRUCTOR) {
		return target.parent();
	}
	Some(target.clone())
}

fn method_target(owner: &Moniker, call_name: &str, call_arity: Option<usize>) -> Moniker {
	let arity = call_arity.unwrap_or_default();
	let mut segment = Vec::with_capacity(call_name.len() + 2 + arity.saturating_mul(2));
	segment.extend_from_slice(call_name.as_bytes());
	segment.push(b'(');
	for idx in 0..arity {
		if idx > 0 {
			segment.push(b',');
		}
		segment.push(b'_');
	}
	segment.push(b')');
	MonikerBuilder::from_view(owner.as_view())
		.segment(kinds::METHOD, &segment)
		.build()
}

fn external_type_target(project: &[u8], path: &[&str]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(project);
	if let Some((head, tail)) = path.split_first() {
		builder.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for piece in tail {
			builder.segment(kinds::PATH, piece.as_bytes());
		}
	}
	builder.build()
}

fn decapitalize_java_property(property: &str) -> Vec<u8> {
	let bytes = property.as_bytes();
	if bytes.len() > 1 && bytes[0].is_ascii_uppercase() && bytes[1].is_ascii_uppercase() {
		return bytes.to_vec();
	}
	let mut out = bytes.to_vec();
	if let Some(first) = out.first_mut() {
		first.make_ascii_lowercase();
	}
	out
}

const LOMBOK_ANNOTATIONS: &[LombokAnnotation] = &[
	ann("AllArgsConstructor", &["lombok"], LombokEffect::Constructor),
	ann("Builder", &["lombok"], LombokEffect::Builder),
	ann("Default", &["lombok", "Builder"], LombokEffect::Builder),
	ann("ObtainVia", &["lombok", "Builder"], LombokEffect::Builder),
	ann("Cleanup", &["lombok"], LombokEffect::Cleanup),
	ann(
		"CustomLog",
		&["lombok"],
		LombokEffect::Logger(LombokLogBackend::Custom),
	),
	ann("Data", &["lombok"], LombokEffect::Data),
	ann("Delegate", &["lombok"], LombokEffect::Delegate),
	ann(
		"EqualsAndHashCode",
		&["lombok"],
		LombokEffect::ObjectMethods,
	),
	ann(
		"Exclude",
		&["lombok", "EqualsAndHashCode"],
		LombokEffect::ObjectMethods,
	),
	ann(
		"Include",
		&["lombok", "EqualsAndHashCode"],
		LombokEffect::ObjectMethods,
	),
	ann("Generated", &["lombok"], LombokEffect::GeneratedMarker),
	ann("Getter", &["lombok"], LombokEffect::Getter),
	ann("Locked", &["lombok"], LombokEffect::Concurrency),
	ann("Read", &["lombok"], LombokEffect::Concurrency),
	ann("Write", &["lombok"], LombokEffect::Concurrency),
	ann("NoArgsConstructor", &["lombok"], LombokEffect::Constructor),
	ann("NonNull", &["lombok"], LombokEffect::NullCheck),
	ann(
		"RequiredArgsConstructor",
		&["lombok"],
		LombokEffect::Constructor,
	),
	ann("Setter", &["lombok"], LombokEffect::Setter),
	ann("Singular", &["lombok"], LombokEffect::Builder),
	ann("SneakyThrows", &["lombok"], LombokEffect::ExceptionFlow),
	ann("Synchronized", &["lombok"], LombokEffect::Concurrency),
	ann("ToString", &["lombok"], LombokEffect::ObjectMethods),
	ann(
		"Exclude",
		&["lombok", "ToString"],
		LombokEffect::ObjectMethods,
	),
	ann(
		"Include",
		&["lombok", "ToString"],
		LombokEffect::ObjectMethods,
	),
	ann("Value", &["lombok"], LombokEffect::Value),
	ann("var", &["lombok"], LombokEffect::Unsupported),
	ann("val", &["lombok"], LombokEffect::Unsupported),
	ann("With", &["lombok"], LombokEffect::With),
	ann(
		"CommonsLog",
		&["lombok", "extern", "apachecommons"],
		LombokEffect::Logger(LombokLogBackend::Commons),
	),
	ann(
		"Flogger",
		&["lombok", "extern", "flogger"],
		LombokEffect::Logger(LombokLogBackend::Flogger),
	),
	ann(
		"JBossLog",
		&["lombok", "extern", "jbosslog"],
		LombokEffect::Logger(LombokLogBackend::JBoss),
	),
	ann(
		"Log",
		&["lombok", "extern", "java"],
		LombokEffect::Logger(LombokLogBackend::JavaUtil),
	),
	ann(
		"Log4j",
		&["lombok", "extern", "log4j"],
		LombokEffect::Logger(LombokLogBackend::Log4j),
	),
	ann(
		"Log4j2",
		&["lombok", "extern", "log4j"],
		LombokEffect::Logger(LombokLogBackend::Log4j2),
	),
	ann(
		"Slf4j",
		&["lombok", "extern", "slf4j"],
		LombokEffect::Logger(LombokLogBackend::Slf4j),
	),
	ann(
		"XSlf4j",
		&["lombok", "extern", "slf4j"],
		LombokEffect::Logger(LombokLogBackend::XSlf4j),
	),
	ann(
		"Accessors",
		&["lombok", "experimental"],
		LombokEffect::ConfigMarker,
	),
	ann(
		"Delegate",
		&["lombok", "experimental"],
		LombokEffect::Delegate,
	),
	ann(
		"ExtensionMethod",
		&["lombok", "experimental"],
		LombokEffect::ExtensionMethod,
	),
	ann(
		"FieldDefaults",
		&["lombok", "experimental"],
		LombokEffect::FieldDefaults,
	),
	ann(
		"FieldNameConstants",
		&["lombok", "experimental"],
		LombokEffect::FieldNames,
	),
	ann(
		"Exclude",
		&["lombok", "experimental", "FieldNameConstants"],
		LombokEffect::FieldNames,
	),
	ann(
		"Include",
		&["lombok", "experimental", "FieldNameConstants"],
		LombokEffect::FieldNames,
	),
	ann(
		"Helper",
		&["lombok", "experimental"],
		LombokEffect::Unsupported,
	),
	ann(
		"Jacksonized",
		&["lombok", "experimental"],
		LombokEffect::ConfigMarker,
	),
	ann(
		"NonFinal",
		&["lombok", "experimental"],
		LombokEffect::ConfigMarker,
	),
	ann(
		"PackagePrivate",
		&["lombok", "experimental"],
		LombokEffect::ConfigMarker,
	),
	ann(
		"StandardException",
		&["lombok", "experimental"],
		LombokEffect::Constructor,
	),
	ann(
		"SuperBuilder",
		&["lombok", "experimental"],
		LombokEffect::Builder,
	),
	ann(
		"Tolerate",
		&["lombok", "experimental"],
		LombokEffect::ConfigMarker,
	),
	ann(
		"UtilityClass",
		&["lombok", "experimental"],
		LombokEffect::UtilityClass,
	),
	ann("WithBy", &["lombok", "experimental"], LombokEffect::With),
	ann("Wither", &["lombok", "experimental"], LombokEffect::With),
	ann(
		"var",
		&["lombok", "experimental"],
		LombokEffect::Unsupported,
	),
];

const fn ann(
	name: &'static str,
	path: &'static [&'static str],
	effect: LombokEffect,
) -> LombokAnnotation {
	LombokAnnotation { name, path, effect }
}
