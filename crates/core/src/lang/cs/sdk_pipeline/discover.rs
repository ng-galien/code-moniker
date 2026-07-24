// code-moniker: ignore-file[smell-feature-envy-local, smell-data-clumps-param-names, smell-god-type-local-metrics, smell-large-type, smell-vertical-layout]
// TODO(smell): split C# discovery into classification, member/local declaration handling, using resolution, call/type-ref resolution, and emission phases before enabling these guardrails here.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::Position;
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	callable_segment_slots, extend_callable_slots, extend_segment, join_bytes_with_comma,
	slot_signature_bytes,
};
use crate::lang::sdk::{DiscoveredDef, Namespace, RefHints, ResolvedRef};
use crate::lang::tree_util::{find_named_child, node_position, node_slice};

use super::super::canonicalize::{parameter_list_slots, parameter_slots};
use super::super::kinds;

type CallableMetadata = HashMap<Moniker, (Vec<u8>, Option<usize>)>;

pub(super) struct DiscoveredCsFile {
	pub(super) root: Moniker,
	pub(super) defs: Vec<DiscoveredDef>,
	pub(super) refs: Vec<ResolvedRef>,
}

#[derive(Default)]
struct CsDefAttrs<'a> {
	visibility: &'static [u8],
	signature: &'a [u8],
}

#[derive(Default)]
struct CsRefAttrs<'a> {
	confidence: &'static [u8],
	receiver_hint: &'a [u8],
	alias: &'a [u8],
	call_name: &'a [u8],
	call_arity: Option<usize>,
}

struct CsRefSpec {
	kind: &'static [u8],
	target: Moniker,
	confidence: &'static [u8],
	position: Position,
	receiver_hint: &'static [u8],
	alias: &'static [u8],
}

struct CsSymbol<'src> {
	moniker: Moniker,
	kind: &'static [u8],
	visibility: &'static [u8],
	signature: Option<Vec<u8>>,
	body: Option<Node<'src>>,
	position: Position,
	annotated_by: Vec<CsRefSpec>,
}

enum CsNodeShape<'src> {
	Annotation { kind: &'static [u8] },
	CsSymbol(CsSymbol<'src>),
	Skip,
	Recurse,
}

struct CsBuilder {
	root: Moniker,
	defs: Vec<DiscoveredDef>,
	refs: Vec<ResolvedRef>,
	seen_defs: HashSet<Moniker>,
}

impl CsBuilder {
	fn new(root: Moniker) -> Self {
		Self {
			root,
			defs: Vec::new(),
			refs: Vec::new(),
			seen_defs: HashSet::new(),
		}
	}

	fn contains(&self, moniker: &Moniker) -> bool {
		moniker == &self.root || self.seen_defs.contains(moniker)
	}

	fn add_def(
		&mut self,
		moniker: Moniker,
		kind: &'static [u8],
		parent: &Moniker,
		position: Option<Position>,
	) -> Result<(), ()> {
		self.add_def_attrs(moniker, kind, parent, position, &CsDefAttrs::default())
	}

	fn add_def_attrs(
		&mut self,
		moniker: Moniker,
		kind: &'static [u8],
		parent: &Moniker,
		position: Option<Position>,
		attrs: &CsDefAttrs<'_>,
	) -> Result<(), ()> {
		if self.contains(&moniker) || !self.contains(parent) || !parent.is_ancestor_of(&moniker) {
			return Err(());
		}
		let name = moniker
			.as_view()
			.segments()
			.last()
			.map(|segment| segment.name.to_vec())
			.unwrap_or_default();
		self.seen_defs.insert(moniker.clone());
		self.defs.push(DiscoveredDef {
			moniker,
			parent: parent.clone(),
			namespace: namespace_for(kind),
			name,
			kind,
			visibility: attrs.visibility,
			signature: attrs.signature.to_vec(),
			position,
			call_name: Vec::new(),
			call_arity: None,
		});
		Ok(())
	}

	fn add_ref_attrs(
		&mut self,
		source: &Moniker,
		target: Moniker,
		kind: &'static [u8],
		position: Option<Position>,
		attrs: &CsRefAttrs<'_>,
	) -> Result<(), ()> {
		if !self.contains(source) {
			return Err(());
		}
		self.refs.push(ResolvedRef {
			source: source.clone(),
			target,
			kind,
			position,
			confidence: attrs.confidence,
			hints: RefHints {
				receiver_hint: attrs.receiver_hint.to_vec(),
				alias: attrs.alias.to_vec(),
				namespace: None,
				call_name: attrs.call_name.to_vec(),
				call_arity: attrs.call_arity,
			},
		});
		Ok(())
	}

	fn finish(mut self, metadata: &CallableMetadata) -> DiscoveredCsFile {
		for definition in &mut self.defs {
			if let Some((call_name, call_arity)) = metadata.get(&definition.moniker) {
				definition.call_name.clone_from(call_name);
				definition.call_arity = *call_arity;
			} else if matches!(definition.kind, b"method" | b"constructor") {
				definition.call_name =
					crate::core::moniker::query::bare_callable_name(&definition.name).to_vec();
			}
		}
		DiscoveredCsFile {
			root: self.root,
			defs: self.defs,
			refs: self.refs,
		}
	}
}

fn namespace_for(kind: &[u8]) -> Namespace {
	match kind {
		b"class" | b"interface" | b"struct" | b"record" | b"enum" | b"delegate" => Namespace::Type,
		b"module" => Namespace::Module,
		_ => Namespace::Value,
	}
}

#[derive(Clone)]
pub(super) struct ImportEntry {
	pub confidence: &'static [u8],
	pub module_prefix: Moniker,
}

pub(super) struct CsDiscover<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) imports: RefCell<HashMap<Vec<u8>, ImportEntry>>,
	pub(super) local_scope: RefCell<Vec<HashSet<Vec<u8>>>>,
	pub(super) type_table: HashMap<&'src [u8], Moniker>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), Vec<u8>>,
}

struct PendingAnnotation {
	kind: &'static [u8],
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

pub(super) fn discover<'src>(
	module: Moniker,
	source: &'src str,
	root: Node<'src>,
	deep: bool,
) -> DiscoveredCsFile {
	let mut type_table = HashMap::new();
	collect_type_table(root, source.as_bytes(), &module, &mut type_table);
	let mut callable_table = HashMap::new();
	let mut callable_metadata = HashMap::new();
	collect_callable_table(
		root,
		source.as_bytes(),
		&module,
		&mut callable_table,
		&mut callable_metadata,
	);
	let discover = CsDiscover {
		module: module.clone(),
		source_bytes: source.as_bytes(),
		deep,
		imports: RefCell::new(HashMap::new()),
		local_scope: RefCell::new(Vec::new()),
		type_table,
		callable_table,
	};
	let mut builder = CsBuilder::new(module.clone());
	discover.walk(root, &module, &mut builder);
	builder.finish(&callable_metadata)
}

impl CsDiscover<'_> {
	fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let mut cursor = node.walk();
		let mut pending = None;
		for child in node.children(&mut cursor) {
			match self.classify(child, scope, self.source_bytes, graph) {
				CsNodeShape::Annotation { kind } => {
					self.extend_or_flush(&mut pending, kind, child, scope, graph)
				}
				CsNodeShape::CsSymbol(symbol) => {
					self.flush_pending(&mut pending, scope, graph);
					self.emit_symbol(child, scope, symbol, graph);
				}
				CsNodeShape::Skip => self.flush_pending(&mut pending, scope, graph),
				CsNodeShape::Recurse => {
					self.flush_pending(&mut pending, scope, graph);
					self.walk(child, scope, graph);
				}
			}
		}
		self.flush_pending(&mut pending, scope, graph);
	}

	fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		match self.classify(node, scope, self.source_bytes, graph) {
			CsNodeShape::Annotation { kind } => self.emit_annotation_range(
				kind,
				node.start_byte() as u32,
				node.end_byte() as u32,
				scope,
				graph,
			),
			CsNodeShape::CsSymbol(symbol) => self.emit_symbol(node, scope, symbol, graph),
			CsNodeShape::Skip => {}
			CsNodeShape::Recurse => self.walk(node, scope, graph),
		}
	}

	fn emit_symbol(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		symbol: CsSymbol<'_>,
		graph: &mut CsBuilder,
	) {
		let parent = symbol
			.moniker
			.parent()
			.filter(|parent| parent != scope && graph.contains(parent))
			.unwrap_or_else(|| scope.clone());
		let attrs = CsDefAttrs {
			visibility: symbol.visibility,
			signature: symbol.signature.as_deref().unwrap_or_default(),
		};
		if graph
			.add_def_attrs(
				symbol.moniker.clone(),
				symbol.kind,
				&parent,
				Some(symbol.position),
				&attrs,
			)
			.is_err()
		{
			return;
		}
		for reference in symbol.annotated_by {
			let attrs = CsRefAttrs {
				confidence: reference.confidence,
				receiver_hint: reference.receiver_hint,
				alias: reference.alias,
				..CsRefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				&symbol.moniker,
				reference.target,
				reference.kind,
				Some(reference.position),
				&attrs,
			);
		}
		self.before_body(node, symbol.kind, &symbol.moniker, self.source_bytes, graph);
		if let Some(body) = symbol.body {
			self.walk(body, &symbol.moniker, graph);
		}
		self.after_body(symbol.kind, &symbol.moniker);
		self.on_symbol_emitted(node, symbol.kind, &symbol.moniker, self.source_bytes, graph);
	}

	fn extend_or_flush(
		&self,
		pending: &mut Option<PendingAnnotation>,
		kind: &'static [u8],
		child: Node<'_>,
		scope: &Moniker,
		graph: &mut CsBuilder,
	) {
		let start_row = child.start_position().row;
		let end_row = child.end_position().row;
		let start_byte = child.start_byte() as u32;
		let end_byte = child.end_byte() as u32;
		if let Some(annotation) = pending.as_mut() {
			if annotation.kind == kind && start_row <= annotation.end_row + 1 {
				annotation.end_byte = end_byte;
				annotation.end_row = end_row;
				return;
			}
			self.emit_annotation_range(
				annotation.kind,
				annotation.start_byte,
				annotation.end_byte,
				scope,
				graph,
			);
		}
		*pending = Some(PendingAnnotation {
			kind,
			start_byte,
			end_byte,
			end_row,
		});
	}

	fn flush_pending(
		&self,
		pending: &mut Option<PendingAnnotation>,
		scope: &Moniker,
		graph: &mut CsBuilder,
	) {
		if let Some(annotation) = pending.take() {
			self.emit_annotation_range(
				annotation.kind,
				annotation.start_byte,
				annotation.end_byte,
				scope,
				graph,
			);
		}
	}

	fn emit_annotation_range(
		&self,
		kind: &'static [u8],
		start_byte: u32,
		end_byte: u32,
		scope: &Moniker,
		graph: &mut CsBuilder,
	) {
		let moniker = crate::lang::callable::extend_segment_u32(scope, kind, start_byte);
		let _ = graph.add_def(moniker, kind, scope, Some((start_byte, end_byte)));
	}

	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CsBuilder,
	) -> CsNodeShape<'src> {
		match node.kind() {
			"comment" => CsNodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"namespace_declaration" | "file_scoped_namespace_declaration" => CsNodeShape::Recurse,
			"class_declaration" => self.classify_type(node, scope, source, kinds::CLASS),
			"struct_declaration" => self.classify_type(node, scope, source, kinds::STRUCT),
			"interface_declaration" => self.classify_type(node, scope, source, kinds::INTERFACE),
			"enum_declaration" => self.classify_type(node, scope, source, kinds::ENUM),
			"record_declaration" => self.classify_record(node, scope, source, kinds::RECORD),
			"record_struct_declaration" => self.classify_record(node, scope, source, kinds::STRUCT),
			"method_declaration" => self.classify_callable(node, scope, source, kinds::METHOD),
			"constructor_declaration" => {
				self.classify_callable(node, scope, source, kinds::CONSTRUCTOR)
			}
			"field_declaration" => {
				self.handle_field(node, scope, graph);
				CsNodeShape::Skip
			}
			"property_declaration" => self.classify_property(node, scope, source, graph),
			"using_directive" => {
				self.handle_using(node, scope, graph);
				CsNodeShape::Skip
			}
			"invocation_expression" => {
				self.handle_invocation(node, scope, graph);
				CsNodeShape::Skip
			}
			"object_creation_expression" => {
				self.handle_object_creation(node, scope, graph);
				CsNodeShape::Skip
			}
			"local_declaration_statement" => {
				self.handle_local_declaration(node, scope, graph);
				CsNodeShape::Skip
			}
			"foreach_statement" => {
				self.handle_foreach(node, scope, graph);
				CsNodeShape::Skip
			}
			_ => CsNodeShape::Recurse,
		}
	}

	fn before_body(
		&self,
		node: Node<'_>,
		kind: &[u8],
		moniker: &Moniker,
		_source: &[u8],
		graph: &mut CsBuilder,
	) {
		if kind == kinds::ENUM {
			self.emit_enum_constants(node, moniker, graph);
			return;
		}
		if kind == kinds::METHOD || kind == kinds::CONSTRUCTOR {
			if let Some(rt) = node.child_by_field_name("returns") {
				self.emit_uses_type(rt, moniker, graph);
			}
			if let Some(params) = node.child_by_field_name("parameters") {
				self.emit_param_defs_and_types(params, moniker, graph);
			}
			return;
		}
		if kind == kinds::RECORD || (kind == kinds::STRUCT && is_record_struct(node)) {
			self.emit_record_primary_constructor(node, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if kind == kinds::METHOD || kind == kinds::CONSTRUCTOR {
			self.pop_local_scope();
		}
	}

	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		_source: &[u8],
		graph: &mut CsBuilder,
	) {
		if sym_kind != kinds::RECORD && !(sym_kind == kinds::STRUCT && is_record_struct(node)) {
			return;
		}
		if find_named_child(node, "declaration_list").is_none() {
			self.emit_record_primary_constructor(node, sym_moniker, graph);
		}
	}
}

impl<'src_lang> CsDiscover<'src_lang> {
	fn classify_type<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> CsNodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return CsNodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kind, name);

		let mut annotated_by: Vec<CsRefSpec> = Vec::new();
		if let Some(bases) = find_named_child(node, "base_list") {
			self.collect_base_list_refs(bases, &mut annotated_by);
		}
		self.collect_attribute_refs(node, &mut annotated_by);

		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		CsNodeShape::CsSymbol(CsSymbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, default_vis),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn emit_enum_constants(&self, enum_node: Node<'_>, parent: &Moniker, graph: &mut CsBuilder) {
		let Some(body) = enum_node
			.child_by_field_name("body")
			.or_else(|| find_named_child(enum_node, "enum_member_declaration_list"))
		else {
			return;
		};
		let mut cursor = body.walk();
		for member in body.named_children(&mut cursor) {
			if member.kind() != "enum_member_declaration" {
				continue;
			}
			let Some(name_node) = member
				.child_by_field_name("name")
				.or_else(|| member.named_child(0))
			else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() {
				continue;
			}
			let moniker = extend_segment(parent, kinds::ENUM_CONSTANT, name);
			let _ = graph.add_def(
				moniker,
				kinds::ENUM_CONSTANT,
				parent,
				Some(node_position(member)),
			);
		}
	}

	fn classify_record<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> CsNodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return CsNodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kind, name);

		let mut annotated_by: Vec<CsRefSpec> = Vec::new();
		if let Some(bases) = find_named_child(node, "base_list") {
			self.collect_base_list_refs(bases, &mut annotated_by);
		}
		self.collect_attribute_refs(node, &mut annotated_by);

		let default_vis = if scope == &self.module {
			kinds::VIS_PACKAGE
		} else {
			kinds::VIS_PRIVATE
		};
		CsNodeShape::CsSymbol(CsSymbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, default_vis),
			signature: None,
			body: find_named_child(node, "declaration_list"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_callable<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		kind: &'static [u8],
	) -> CsNodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return CsNodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let slots = parameter_slots(node, source);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let moniker = extend_callable_slots(scope, kind, name, &slots);

		let mut annotated_by: Vec<CsRefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut annotated_by);

		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.record_param_locals(params);
		}

		CsNodeShape::CsSymbol(CsSymbol {
			moniker,
			kind,
			visibility: modifier_visibility(node, kinds::VIS_PRIVATE),
			signature: Some(signature),
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_property<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut CsBuilder,
	) -> CsNodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return CsNodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let moniker = extend_segment(scope, kinds::PROPERTY, name);

		let mut annotated_by: Vec<CsRefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut annotated_by);

		CsNodeShape::CsSymbol(CsSymbol {
			moniker,
			kind: kinds::PROPERTY,
			visibility: modifier_visibility(node, kinds::VIS_PRIVATE),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by,
		})
	}

	fn emit_record_primary_constructor(
		&self,
		node: Node<'_>,
		record: &Moniker,
		graph: &mut CsBuilder,
	) {
		let Some(plist) = find_named_child(node, "parameter_list") else {
			return;
		};
		let Some(name_node) = node.child_by_field_name("name") else {
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		let slots = parameter_list_slots(plist, self.source_bytes);
		let signature =
			join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
		let ctor = extend_callable_slots(record, kinds::CONSTRUCTOR, name, &slots);
		let attrs = CsDefAttrs {
			visibility: kinds::VIS_PUBLIC,
			signature: &signature,
		};
		let _ = graph.add_def_attrs(
			ctor,
			kinds::CONSTRUCTOR,
			record,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn record_param_locals(&self, params: Node<'_>) {
		let mut cursor = params.walk();
		for p in params.named_children(&mut cursor) {
			if p.kind() != "parameter" {
				continue;
			}
			let Some(name_node) = p.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() || name == b"_" {
				continue;
			}
			self.record_local(name);
		}
	}

	fn emit_param_defs_and_types(
		&self,
		params: Node<'_>,
		callable: &Moniker,
		graph: &mut CsBuilder,
	) {
		let mut cursor = params.walk();
		for p in params.named_children(&mut cursor) {
			if p.kind() != "parameter" {
				continue;
			}
			let Some(name_node) = p.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() || name == b"_" {
				continue;
			}
			if let Some(type_node) = p.child_by_field_name("type") {
				self.emit_uses_type(type_node, callable, graph);
				self.emit_typed_binding(type_node, callable, name, graph);
			}
			if self.deep {
				let m = extend_segment(callable, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(name_node)));
			}
		}
	}

	fn emit_typed_binding(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		name: &[u8],
		graph: &mut CsBuilder,
	) {
		let resolved = match type_node.kind() {
			"generic_name" => first_identifier_child(type_node)
				.and_then(|identifier| self.resolve_type_node(identifier)),
			_ => self.resolve_type_node(type_node),
		};
		let Some((target, confidence)) = resolved else {
			return;
		};
		let attrs = CsRefAttrs {
			confidence,
			alias: name,
			..CsRefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::TYPED_AS,
			Some(node_position(type_node)),
			&attrs,
		);
	}

	fn emit_typed_source(&self, type_node: Node<'_>, source: &Moniker, graph: &mut CsBuilder) {
		let resolved = match type_node.kind() {
			"generic_name" => first_identifier_child(type_node)
				.and_then(|identifier| self.resolve_type_node(identifier)),
			_ => self.resolve_type_node(type_node),
		};
		let Some((target, confidence)) = resolved else {
			return;
		};
		let attrs = CsRefAttrs {
			confidence,
			..CsRefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			source,
			target,
			kinds::TYPED_AS,
			Some(node_position(type_node)),
			&attrs,
		);
	}

	fn handle_field(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let visibility = modifier_visibility(node, kinds::VIS_PRIVATE);
		self.emit_attribute_refs_on(node, scope, graph);
		let Some(decl) = find_named_child(node, "variable_declaration") else {
			return;
		};
		let type_node = decl.child_by_field_name("type");
		if let Some(type_node) = type_node {
			self.emit_uses_type(type_node, scope, graph);
		}
		let mut cursor = decl.walk();
		for child in decl.named_children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, self.source_bytes);
			let m = extend_segment(scope, kinds::FIELD, name);
			let attrs = CsDefAttrs {
				visibility,
				..CsDefAttrs::default()
			};
			let _ = graph.add_def_attrs(
				m.clone(),
				kinds::FIELD,
				scope,
				Some(node_position(child)),
				&attrs,
			);
			if let Some(type_node) = type_node {
				self.emit_typed_source(type_node, &m, graph);
			}
		}
	}

	fn handle_local_declaration(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let Some(decl) = find_named_child(node, "variable_declaration") else {
			return;
		};
		let type_node = decl.child_by_field_name("type");
		if let Some(type_node) = type_node {
			self.emit_uses_type(type_node, scope, graph);
		}
		let in_callable = is_callable_scope(scope, &self.module);
		let mut cursor = decl.walk();
		for child in decl.named_children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			if in_callable && let Some(name_node) = child.child_by_field_name("name") {
				let name = node_slice(name_node, self.source_bytes);
				if !name.is_empty() && name != b"_" {
					self.record_local(name);
					if let Some(type_node) = type_node {
						if type_node.kind() == "implicit_type" {
							self.emit_inferred_local_type(child, scope, name, graph);
						} else {
							self.emit_typed_binding(type_node, scope, name, graph);
						}
					}
					if self.deep {
						let m = extend_segment(scope, kinds::LOCAL, name);
						let _ =
							graph.add_def(m, kinds::LOCAL, scope, Some(node_position(name_node)));
					}
				}
			}
			let mut dc = child.walk();
			for c in child.named_children(&mut dc) {
				if c.kind() != "identifier" {
					self.recurse_subtree(c, scope, graph);
				}
			}
		}
	}

	fn emit_inferred_local_type(
		&self,
		declarator: Node<'_>,
		scope: &Moniker,
		name: &[u8],
		graph: &mut CsBuilder,
	) {
		let initializer = declarator.child_by_field_name("value").or_else(|| {
			let mut cursor = declarator.walk();
			declarator
				.named_children(&mut cursor)
				.find(|child| child.kind() == "object_creation_expression")
		});
		let Some(type_node) = initializer
			.filter(|initializer| initializer.kind() == "object_creation_expression")
			.and_then(|initializer| initializer.child_by_field_name("type"))
		else {
			return;
		};
		self.emit_typed_binding(type_node, scope, name, graph);
	}

	fn handle_foreach(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		if let Some(t) = node.child_by_field_name("type") {
			self.emit_uses_type(t, scope, graph);
		}
		let in_callable = is_callable_scope(scope, &self.module);
		if in_callable
			&& let Some(left) = node.child_by_field_name("left")
			&& left.kind() == "identifier"
		{
			let name = node_slice(left, self.source_bytes);
			if !name.is_empty() && name != b"_" {
				self.record_local(name);
				if self.deep {
					let m = extend_segment(scope, kinds::LOCAL, name);
					let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(left)));
				}
			}
		}
		if let Some(right) = node.child_by_field_name("right") {
			self.recurse_subtree(right, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
	}

	fn handle_using(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let pos = node_position(node);
		let alias_node = node.child_by_field_name("name");
		let mut path_node: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if matches!(c.kind(), "qualified_name" | "identifier")
				&& Some(c.id()) != alias_node.map(|n| n.id())
			{
				path_node = Some(c);
			}
		}
		let Some(path_node) = path_node else { return };
		let pieces = collect_qualified_pieces(path_node, self.source_bytes);
		if pieces.is_empty() {
			return;
		}
		let confidence = stdlib_or_imported(&pieces);
		let alias = alias_node
			.and_then(|n| n.utf8_text(self.source_bytes).ok())
			.unwrap_or("");
		let bind_name = if !alias.is_empty() {
			alias
		} else {
			pieces.last().copied().unwrap_or("")
		};

		let module_prefix = build_module_target(self.module.as_view().project(), &pieces);
		if !bind_name.is_empty() {
			self.imports.borrow_mut().insert(
				bind_name.as_bytes().to_vec(),
				ImportEntry {
					confidence,
					module_prefix: module_prefix.clone(),
				},
			);
		}
		let attrs = CsRefAttrs {
			confidence,
			alias: alias.as_bytes(),
			..CsRefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			module_prefix,
			kinds::IMPORTS_MODULE,
			Some(pos),
			&attrs,
		);
	}

	fn handle_invocation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let pos = node_position(node);
		let call_arity = node
			.child_by_field_name("arguments")
			.map(|arguments| arguments.named_child_count());
		let Some(callee) = node.child_by_field_name("function") else {
			self.walk_children(node, scope, graph);
			return;
		};
		match callee.kind() {
			"identifier" => self.emit_simple_call(callee, scope, pos, call_arity, graph),
			"member_access_expression" => {
				self.emit_member_call(callee, scope, pos, call_arity, graph);
			}
			_ => self.recurse_subtree(callee, scope, graph),
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn emit_simple_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		call_arity: Option<usize>,
		graph: &mut CsBuilder,
	) {
		let name = node_slice(callee, self.source_bytes);
		if name.is_empty() {
			return;
		}
		if let Some(entry) = self.import_entry_for(name) {
			let target = extend_segment(&entry.module_prefix, kinds::FUNCTION, name);
			let attrs = CsRefAttrs {
				confidence: entry.confidence,
				call_name: name,
				call_arity,
				..CsRefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
			return;
		}
		let Some(conf) = self.name_confidence(name) else {
			return;
		};
		let target = if conf == kinds::CONF_LOCAL {
			extend_segment(scope, kinds::LOCAL, name)
		} else {
			self.lookup_callable_in_scope(scope, name, kinds::METHOD)
				.unwrap_or_else(|| extend_segment(&self.module, kinds::FUNCTION, name))
		};
		let attrs = CsRefAttrs {
			confidence: conf,
			call_name: name,
			call_arity,
			..CsRefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
	}

	fn emit_member_call(
		&self,
		callee: Node<'_>,
		scope: &Moniker,
		pos: (u32, u32),
		call_arity: Option<usize>,
		graph: &mut CsBuilder,
	) {
		let Some(name_node) = callee.child_by_field_name("name") else {
			self.walk_children(callee, scope, graph);
			return;
		};
		let name = node_slice(name_node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let operand = callee.child_by_field_name("expression");
		let target = extend_segment(&self.module, kinds::METHOD, name);
		let hint = operand
			.map(|o| receiver_hint(o, self.source_bytes))
			.unwrap_or(b"");
		let attrs = CsRefAttrs {
			receiver_hint: hint,
			confidence: kinds::CONF_NAME_MATCH,
			call_name: name,
			call_arity,
			..CsRefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
		if let Some(op) = operand {
			self.recurse_subtree(op, scope, graph);
		}
	}

	fn handle_object_creation(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		if let Some(type_node) = node.child_by_field_name("type") {
			self.emit_type_ref(type_node, scope, kinds::INSTANTIATES, graph);
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn emit_uses_type(&self, type_node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		self.emit_type_ref(type_node, scope, kinds::USES_TYPE, graph);
	}

	fn emit_type_ref(
		&self,
		type_node: Node<'_>,
		scope: &Moniker,
		ref_kind: &'static [u8],
		graph: &mut CsBuilder,
	) {
		match type_node.kind() {
			"predefined_type" | "implicit_type" => {}
			"identifier" | "qualified_name" => {
				if let Some((target, confidence)) = self.resolve_type_node(type_node) {
					let attrs = CsRefAttrs {
						confidence,
						..CsRefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						ref_kind,
						Some(node_position(type_node)),
						&attrs,
					);
				}
			}
			"generic_name" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					match c.kind() {
						"identifier" => {
							if let Some((target, confidence)) = self.resolve_type_node(c) {
								let attrs = CsRefAttrs {
									confidence,
									..CsRefAttrs::default()
								};
								let _ = graph.add_ref_attrs(
									scope,
									target,
									ref_kind,
									Some(node_position(c)),
									&attrs,
								);
							}
						}
						"type_argument_list" => {
							let mut ac = c.walk();
							for arg in c.named_children(&mut ac) {
								self.emit_type_ref(arg, scope, kinds::USES_TYPE, graph);
							}
						}
						_ => {}
					}
				}
			}
			"array_type" | "nullable_type" | "pointer_type" => {
				if let Some(inner) = type_node.child_by_field_name("type") {
					self.emit_type_ref(inner, scope, ref_kind, graph);
				}
			}
			"tuple_type" => {
				let mut cursor = type_node.walk();
				for c in type_node.named_children(&mut cursor) {
					if let Some(t) = c.child_by_field_name("type") {
						self.emit_type_ref(t, scope, ref_kind, graph);
					}
				}
			}
			_ => {}
		}
	}

	fn resolve_type_node(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		match type_node.kind() {
			"identifier" => {
				let name = node_slice(type_node, self.source_bytes);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name, kinds::CLASS))
			}
			"qualified_name" => {
				if let Some((target, confidence)) = self.resolve_qualified_type(type_node) {
					return Some((target, confidence));
				}
				let leaf = qualified_leaf_identifier(type_node)?;
				let name = node_slice(leaf, self.source_bytes);
				if name.is_empty() {
					return None;
				}
				Some(self.resolve_type_target(name, kinds::CLASS))
			}
			_ => None,
		}
	}

	fn resolve_qualified_type(&self, type_node: Node<'_>) -> Option<(Moniker, &'static [u8])> {
		let raw = type_node.utf8_text(self.source_bytes).ok()?;
		let normalized = raw.strip_prefix("global::").unwrap_or(raw);
		let pieces = normalized.split('.').collect::<Vec<_>>();
		if pieces.len() < 2 || !matches!(pieces[0], "System" | "mscorlib") {
			return None;
		}
		let sdk_owned = is_csharp_sdk_type_path(&pieces);
		let (target, confidence) = if sdk_owned {
			(
				csharp_sdk_target(self.module.as_view().project(), &pieces),
				kinds::CONF_EXTERNAL,
			)
		} else {
			(
				build_module_target(self.module.as_view().project(), &pieces),
				kinds::CONF_IMPORTED,
			)
		};
		Some((target, confidence))
	}

	fn collect_base_list_refs(&self, base_list: Node<'_>, out: &mut Vec<CsRefSpec>) {
		let mut cursor = base_list.walk();
		for entry in base_list.named_children(&mut cursor) {
			let (leaf_node, name) = match entry.kind() {
				"identifier" => {
					let n = node_slice(entry, self.source_bytes);
					(entry, n)
				}
				"qualified_name" => {
					let Some(leaf) = qualified_leaf_identifier(entry) else {
						continue;
					};
					(leaf, node_slice(leaf, self.source_bytes))
				}
				"generic_name" => {
					let Some(leaf) = first_identifier_child(entry) else {
						continue;
					};
					(leaf, node_slice(leaf, self.source_bytes))
				}
				_ => continue,
			};
			if name.is_empty() {
				continue;
			}
			let (target, confidence) = self.resolve_type_target(name, kinds::CLASS);
			out.push(CsRefSpec {
				kind: kinds::EXTENDS,
				target,
				confidence,
				position: node_position(leaf_node),
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn collect_attribute_refs(&self, node: Node<'_>, out: &mut Vec<CsRefSpec>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "attribute_list" {
				continue;
			}
			let mut alc = child.walk();
			for attr in child.named_children(&mut alc) {
				if attr.kind() != "attribute" {
					continue;
				}
				let Some(name_node) = attr.child_by_field_name("name") else {
					continue;
				};
				let leaf = match name_node.kind() {
					"identifier" => Some(name_node),
					"qualified_name" => qualified_leaf_identifier(name_node),
					_ => None,
				};
				let Some(leaf) = leaf else { continue };
				let name = node_slice(leaf, self.source_bytes);
				if name.is_empty() {
					continue;
				}
				let (target, confidence) = self.resolve_type_target(name, kinds::CLASS);
				out.push(CsRefSpec {
					kind: kinds::ANNOTATES,
					target,
					confidence,
					position: node_position(attr),
					receiver_hint: b"",
					alias: b"",
				});
			}
		}
	}

	fn emit_attribute_refs_on(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		let mut refs: Vec<CsRefSpec> = Vec::new();
		self.collect_attribute_refs(node, &mut refs);
		for r in refs {
			let attrs = CsRefAttrs {
				confidence: r.confidence,
				..CsRefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, r.target, r.kind, Some(r.position), &attrs);
		}
	}

	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		self.dispatch(node, scope, graph);
	}

	fn walk_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut CsBuilder) {
		self.walk(node, scope, graph);
	}

	fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashSet::new());
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
	}

	fn record_local(&self, name: &[u8]) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn is_local_name(&self, name: &[u8]) -> bool {
		self.local_scope
			.borrow()
			.iter()
			.any(|frame| frame.contains(name))
	}

	fn name_confidence(&self, name: &[u8]) -> Option<&'static [u8]> {
		crate::lang::kinds::name_confidence_for(self.is_local_name(name), self.deep)
	}

	fn import_entry_for(&self, name: &[u8]) -> Option<ImportEntry> {
		self.imports.borrow().get(name).cloned()
	}

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).map(|e| e.confidence)
	}

	fn resolve_type_target(&self, name: &[u8], fallback_kind: &[u8]) -> (Moniker, &'static [u8]) {
		if let Some(m) = self.type_table.get(name) {
			return (m.clone(), kinds::CONF_RESOLVED);
		}
		if let Some(pieces) = clr_system_path(name) {
			let target = build_module_target(self.module.as_view().project(), pieces);
			return (target, kinds::CONF_EXTERNAL);
		}
		let target = extend_segment(&self.module, fallback_kind, name);
		let confidence = self
			.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH);
		(target, confidence)
	}

	fn lookup_callable_in_scope(
		&self,
		scope: &Moniker,
		name: &[u8],
		kind: &[u8],
	) -> Option<Moniker> {
		let parent = enclosing_type(scope, &self.module).unwrap_or_else(|| self.module.clone());
		let seg = self.callable_table.get(&(parent.clone(), name.to_vec()))?;
		Some(extend_segment(&parent, kind, seg))
	}
}

fn enclosing_type(scope: &Moniker, module: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segs: Vec<_> = view.segments().collect();
	let idx = segs.iter().rposition(|s| {
		matches!(
			s.kind,
			b"class" | b"struct" | b"record" | b"interface" | b"enum"
		)
	})?;
	let mut b = crate::core::moniker::MonikerBuilder::new();
	b.project(view.project());
	for s in &segs[..=idx] {
		b.segment(s.kind, s.name);
	}
	let out = b.build();
	if &out == module { None } else { Some(out) }
}

pub(super) fn collect_callable_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<(Moniker, Vec<u8>), Vec<u8>>,
	metadata: &mut HashMap<Moniker, (Vec<u8>, Option<usize>)>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		match child.kind() {
			"class_declaration"
			| "struct_declaration"
			| "record_declaration"
			| "record_struct_declaration"
			| "interface_declaration"
			| "enum_declaration" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let Ok(name) = name_node.utf8_text(source) else {
					continue;
				};
				let kind: &[u8] = match child.kind() {
					"class_declaration" => kinds::CLASS,
					"struct_declaration" | "record_struct_declaration" => kinds::STRUCT,
					"record_declaration" => kinds::RECORD,
					"interface_declaration" => kinds::INTERFACE,
					"enum_declaration" => kinds::ENUM,
					_ => continue,
				};
				let scope = extend_segment(parent, kind, name.as_bytes());
				if let Some(body) = child
					.child_by_field_name("body")
					.or_else(|| find_named_child(child, "declaration_list"))
				{
					collect_callable_table(body, source, &scope, out, metadata);
				}
				if matches!(
					child.kind(),
					"record_declaration" | "record_struct_declaration"
				) && let Some(plist) = find_named_child(child, "parameter_list")
				{
					let slots = parameter_list_slots(plist, source);
					let seg = callable_segment_slots(name.as_bytes(), &slots);
					let moniker = extend_segment(&scope, kinds::CONSTRUCTOR, &seg);
					metadata.insert(
						moniker,
						(
							name.as_bytes().to_vec(),
							parameter_list_arity(plist, source),
						),
					);
					out.insert((scope.clone(), name.as_bytes().to_vec()), seg);
				}
			}
			"method_declaration" | "constructor_declaration" | "local_function_statement" => {
				let Some(name_node) = child.child_by_field_name("name") else {
					continue;
				};
				let name = node_slice(name_node, source);
				let slots = parameter_slots(child, source);
				let seg = callable_segment_slots(name, &slots);
				let kind = match child.kind() {
					"method_declaration" => Some(kinds::METHOD),
					"constructor_declaration" => Some(kinds::CONSTRUCTOR),
					_ => None,
				};
				if let Some(kind) = kind {
					metadata.insert(
						extend_segment(parent, kind, &seg),
						(name.to_vec(), callable_node_arity(child, source)),
					);
				}
				out.insert((parent.clone(), name.to_vec()), seg);
			}
			_ => collect_callable_table(child, source, parent, out, metadata),
		}
	}
}

fn callable_node_arity(callable: Node<'_>, source: &[u8]) -> Option<usize> {
	callable
		.child_by_field_name("parameters")
		.or_else(|| find_named_child(callable, "parameter_list"))
		.and_then(|parameters| parameter_list_arity(parameters, source))
}

fn parameter_list_arity(parameters: Node<'_>, _source: &[u8]) -> Option<usize> {
	let mut count = 0;
	let mut cursor = parameters.walk();
	for parameter in parameters.children(&mut cursor) {
		match parameter.kind() {
			"params" => return None,
			"parameter" => count += 1,
			_ => {}
		}
	}
	Some(count)
}

pub(super) fn collect_type_table<'src>(
	root: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	out: &mut HashMap<&'src [u8], Moniker>,
) {
	let mut cursor = root.walk();
	for child in root.children(&mut cursor) {
		let kind: Option<&[u8]> = match child.kind() {
			"class_declaration" => Some(kinds::CLASS),
			"struct_declaration" => Some(kinds::STRUCT),
			"record_declaration" => Some(kinds::RECORD),
			"record_struct_declaration" => Some(kinds::STRUCT),
			"interface_declaration" => Some(kinds::INTERFACE),
			"enum_declaration" => Some(kinds::ENUM),
			_ => None,
		};
		if let Some(kind) = kind {
			let Some(name_node) = child.child_by_field_name("name") else {
				continue;
			};
			let Ok(name) = name_node.utf8_text(source) else {
				continue;
			};
			let m = extend_segment(parent, kind, name.as_bytes());
			out.entry(name.as_bytes()).or_insert_with(|| m.clone());
			if let Some(body) = child
				.child_by_field_name("body")
				.or_else(|| find_named_child(child, "declaration_list"))
			{
				collect_type_table(body, source, &m, out);
			}
		} else {
			collect_type_table(child, source, parent, out);
		}
	}
}

fn modifier_visibility(node: Node<'_>, default: &'static [u8]) -> &'static [u8] {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "modifier" {
			continue;
		}
		let mut mc = child.walk();
		for kw in child.children(&mut mc) {
			match kw.kind() {
				"public" => return kinds::VIS_PUBLIC,
				"protected" => return kinds::VIS_PROTECTED,
				"private" => return kinds::VIS_PRIVATE,
				"internal" => return kinds::VIS_PACKAGE,
				_ => {}
			}
		}
	}
	default
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::METHOD
}

fn is_record_struct(node: Node<'_>) -> bool {
	node.kind() == "record_struct_declaration"
}

fn collect_qualified_pieces<'src>(node: Node<'_>, source: &'src [u8]) -> Vec<&'src str> {
	let mut out = Vec::new();
	collect_qualified(node, source, &mut out);
	out
}

fn collect_qualified<'src>(node: Node<'_>, source: &'src [u8], out: &mut Vec<&'src str>) {
	match node.kind() {
		"identifier" => {
			if let Ok(s) = node.utf8_text(source)
				&& !s.is_empty()
			{
				out.push(s);
			}
		}
		"qualified_name" => {
			if let Some(q) = node.child_by_field_name("qualifier") {
				collect_qualified(q, source, out);
			}
			if let Some(name) = node.child_by_field_name("name") {
				collect_qualified(name, source, out);
			}
		}
		_ => {}
	}
}

fn qualified_leaf_identifier(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	let mut last = None;
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier" {
			last = Some(c);
		}
	}
	last
}

fn first_identifier_child(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.find(|c| c.kind() == "identifier")
}

fn receiver_hint<'a>(obj: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_MEMBER, HINT_SUBSCRIPT, HINT_THIS};
	match obj.kind() {
		"this_expression" => HINT_THIS,
		"identifier" => obj.utf8_text(source).unwrap_or("").as_bytes(),
		"member_access_expression" => HINT_MEMBER,
		"invocation_expression" => HINT_CALL,
		"element_access_expression" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn clr_system_path(name: &[u8]) -> Option<&'static [&'static str]> {
	let n = std::str::from_utf8(name).ok()?;
	let path: &[&'static str] = match n {
		"Object" => &["System", "Object"],
		"String" => &["System", "String"],
		"StringBuilder" => &["System", "Text", "StringBuilder"],
		"Encoding" => &["System", "Text", "Encoding"],
		"Regex" => &["System", "Text", "RegularExpressions", "Regex"],
		"CultureInfo" => &["System", "Globalization", "CultureInfo"],
		"ResourceManager" => &["System", "Resources", "ResourceManager"],
		"Stream" => &["System", "IO", "Stream"],
		"MemoryStream" => &["System", "IO", "MemoryStream"],
		"FileInfo" => &["System", "IO", "FileInfo"],
		"DirectoryInfo" => &["System", "IO", "DirectoryInfo"],
		"Exception" => &["System", "Exception"],
		"ArgumentException" => &["System", "ArgumentException"],
		"ArgumentNullException" => &["System", "ArgumentNullException"],
		"InvalidOperationException" => &["System", "InvalidOperationException"],
		"NotImplementedException" => &["System", "NotImplementedException"],
		"NotSupportedException" => &["System", "NotSupportedException"],
		"FormatException" => &["System", "FormatException"],
		"NullReferenceException" => &["System", "NullReferenceException"],
		"Type" => &["System", "Type"],
		"Action" => &["System", "Action"],
		"Func" => &["System", "Func"],
		"Predicate" => &["System", "Predicate"],
		"EventHandler" => &["System", "EventHandler"],
		"Lazy" => &["System", "Lazy"],
		"Nullable" => &["System", "Nullable"],
		"DateTime" => &["System", "DateTime"],
		"TimeSpan" => &["System", "TimeSpan"],
		"Guid" => &["System", "Guid"],
		"Uri" => &["System", "Uri"],
		"Random" => &["System", "Random"],
		"Math" => &["System", "Math"],
		"Convert" => &["System", "Convert"],
		"Environment" => &["System", "Environment"],
		"Console" => &["System", "Console"],
		"Tuple" => &["System", "Tuple"],
		"ValueTuple" => &["System", "ValueTuple"],
		"Span" => &["System", "Span"],
		"Memory" => &["System", "Memory"],
		"ReadOnlySpan" => &["System", "ReadOnlySpan"],
		"ReadOnlyMemory" => &["System", "ReadOnlyMemory"],
		"IDisposable" => &["System", "IDisposable"],
		"IComparable" => &["System", "IComparable"],
		"IEquatable" => &["System", "IEquatable"],
		"ICloneable" => &["System", "ICloneable"],
		"IFormattable" => &["System", "IFormattable"],
		"IServiceProvider" => &["System", "IServiceProvider"],
		"Task" => &["System", "Threading", "Tasks", "Task"],
		"ValueTask" => &["System", "Threading", "Tasks", "ValueTask"],
		"CancellationToken" => &["System", "Threading", "CancellationToken"],
		"CancellationTokenSource" => &["System", "Threading", "CancellationTokenSource"],
		"List" => &["System", "Collections", "Generic", "List"],
		"Dictionary" => &["System", "Collections", "Generic", "Dictionary"],
		"HashSet" => &["System", "Collections", "Generic", "HashSet"],
		"Queue" => &["System", "Collections", "Generic", "Queue"],
		"Stack" => &["System", "Collections", "Generic", "Stack"],
		"LinkedList" => &["System", "Collections", "Generic", "LinkedList"],
		"SortedDictionary" => &["System", "Collections", "Generic", "SortedDictionary"],
		"SortedSet" => &["System", "Collections", "Generic", "SortedSet"],
		"IEnumerable" => &["System", "Collections", "Generic", "IEnumerable"],
		"ICollection" => &["System", "Collections", "Generic", "ICollection"],
		"IList" => &["System", "Collections", "Generic", "IList"],
		"IDictionary" => &["System", "Collections", "Generic", "IDictionary"],
		"IReadOnlyList" => &["System", "Collections", "Generic", "IReadOnlyList"],
		"IReadOnlyCollection" => &["System", "Collections", "Generic", "IReadOnlyCollection"],
		"IReadOnlyDictionary" => &["System", "Collections", "Generic", "IReadOnlyDictionary"],
		"IAsyncEnumerable" => &["System", "Collections", "Generic", "IAsyncEnumerable"],
		"IAsyncEnumerator" => &["System", "Collections", "Generic", "IAsyncEnumerator"],
		"KeyValuePair" => &["System", "Collections", "Generic", "KeyValuePair"],
		"ConcurrentDictionary" => &[
			"System",
			"Collections",
			"Concurrent",
			"ConcurrentDictionary",
		],
		"ConcurrentBag" => &["System", "Collections", "Concurrent", "ConcurrentBag"],
		"ConcurrentQueue" => &["System", "Collections", "Concurrent", "ConcurrentQueue"],
		"ConcurrentStack" => &["System", "Collections", "Concurrent", "ConcurrentStack"],
		"Enumerable" => &["System", "Linq", "Enumerable"],
		"Queryable" => &["System", "Linq", "Queryable"],
		_ => return None,
	};
	Some(path)
}

fn build_module_target(project: &[u8], pieces: &[&str]) -> Moniker {
	if is_csharp_sdk_path(pieces) {
		return csharp_sdk_target(project, pieces);
	}
	let mut builder = MonikerBuilder::new();
	builder.project(project);
	if let Some((head, tail)) = pieces.split_first() {
		builder.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for piece in tail {
			builder.segment(kinds::PATH, piece.as_bytes());
		}
	}
	builder.build()
}

fn csharp_sdk_target(project: &[u8], pieces: &[&str]) -> Moniker {
	let mut builder = crate::lang::sdk::sdk_target_builder(project, b"cs");
	for piece in pieces {
		builder.segment(kinds::PATH, piece.as_bytes());
	}
	builder.build()
}

fn stdlib_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if is_csharp_sdk_path(pieces) {
		kinds::CONF_EXTERNAL
	} else {
		kinds::CONF_IMPORTED
	}
}

// A namespace beginning with `System` is not necessarily part of the target
// framework: System.Reactive and System.CommandLine are NuGet packages. Keep
// the static extractor conservative until target-framework reference packs
// become an indexed input.
fn is_csharp_sdk_path(pieces: &[&str]) -> bool {
	matches!(pieces, ["mscorlib", ..] | ["System"])
		|| pieces.last().is_some_and(|member| {
			clr_system_path(member.as_bytes()).is_some_and(|path| path == pieces)
		}) || is_csharp_bcl_namespace(pieces)
}

fn is_csharp_sdk_type_path(pieces: &[&str]) -> bool {
	is_csharp_sdk_path(pieces)
		|| pieces
			.split_last()
			.is_some_and(|(_, namespace)| is_csharp_bcl_namespace(namespace))
}

fn is_csharp_bcl_namespace(pieces: &[&str]) -> bool {
	matches!(
		pieces,
		["System", "Buffers"]
			| ["System", "Buffers", "Binary"]
			| ["System", "Collections"]
			| ["System", "Collections", "Concurrent"]
			| ["System", "Collections", "Generic"]
			| ["System", "Collections", "ObjectModel"]
			| ["System", "Collections", "Specialized"]
			| ["System", "ComponentModel"]
			| ["System", "Data"]
			| ["System", "Data", "Common"]
			| ["System", "Diagnostics"]
			| ["System", "Diagnostics", "CodeAnalysis"]
			| ["System", "Diagnostics", "Contracts"]
			| ["System", "Diagnostics", "Metrics"]
			| ["System", "Diagnostics", "Tracing"]
			| ["System", "Dynamic"]
			| ["System", "Formats", "Asn1"]
			| ["System", "Formats", "Tar"]
			| ["System", "Globalization"]
			| ["System", "IO"]
			| ["System", "IO", "Compression"]
			| ["System", "IO", "IsolatedStorage"]
			| ["System", "IO", "MemoryMappedFiles"]
			| ["System", "IO", "Pipes"]
			| ["System", "Linq"]
			| ["System", "Linq", "Expressions"]
			| ["System", "Net"]
			| ["System", "Net", "Http"]
			| ["System", "Net", "Http", "Headers"]
			| ["System", "Net", "Mail"]
			| ["System", "Net", "Mime"]
			| ["System", "Net", "NetworkInformation"]
			| ["System", "Net", "Quic"]
			| ["System", "Net", "Security"]
			| ["System", "Net", "Sockets"]
			| ["System", "Net", "WebSockets"]
			| ["System", "Numerics"]
			| ["System", "Reflection"]
			| ["System", "Reflection", "Emit"]
			| ["System", "Reflection", "Metadata"]
			| ["System", "Reflection", "PortableExecutable"]
			| ["System", "Resources"]
			| ["System", "Runtime"]
			| ["System", "Runtime", "CompilerServices"]
			| ["System", "Runtime", "ConstrainedExecution"]
			| ["System", "Runtime", "ExceptionServices"]
			| ["System", "Runtime", "InteropServices"]
			| ["System", "Runtime", "Intrinsics"]
			| ["System", "Runtime", "Loader"]
			| ["System", "Runtime", "Serialization"]
			| ["System", "Runtime", "Versioning"]
			| ["System", "Security"]
			| ["System", "Security", "Claims"]
			| ["System", "Security", "Cryptography"]
			| ["System", "Security", "Principal"]
			| ["System", "Text"]
			| ["System", "Text", "Encodings", "Web"]
			| ["System", "Text", "Json"]
			| ["System", "Text", "RegularExpressions"]
			| ["System", "Text", "Unicode"]
			| ["System", "Threading"]
			| ["System", "Threading", "Channels"]
			| ["System", "Threading", "Tasks"]
			| ["System", "Threading", "Tasks", "Sources"]
			| ["System", "Transactions"]
			| ["System", "Xml"]
			| ["System", "Xml", "Linq"]
			| ["System", "Xml", "Schema"]
			| ["System", "Xml", "Serialization"]
			| ["System", "Xml", "XPath"]
			| ["System", "Xml", "Xsl"]
	)
}
