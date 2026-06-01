// code-moniker: ignore-file[smell-feature-envy-local, smell-long-parameter-list, smell-data-clumps-param-names, smell-god-type-local-metrics, smell-harmonious-method-size, smell-large-type, smell-vertical-layout]
// TODO(smell): split TypeScript SDK discovery into classification, export/import handling, graph emission, callable/type resolution, and local-binding phases before enabling these guardrails here.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{DefAttrs, Position, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{extend_callable_slots, extend_segment, extend_segment_u32};
use crate::lang::sdk::{DiscoveredDef, RefHints, ResolvedRef};
use crate::lang::tree_util::{find_named_child, node_position, node_slice};

use super::super::build::external_pkg_builder;
use super::super::kinds;
use super::canonicalize::{
	anonymous_callback_name, append_module_segments, callable_param_slots, strip_known_extension,
};
use super::defs::{
	CallableEntry, callable_metadata, collect_callable_table, collect_export_ranges,
	collect_type_table, function_decl_info, namespace_for_kind, visibility_attr,
};
use super::refs::{
	confidence_attr, external_runtime_target, is_global_type, is_global_value, namespace_for_ref,
	ref_call_metadata,
};
use super::syntax::{
	apply_path_alias, class_member_visibility, collect_binding_names, first_identifier_text,
	generic_short, import_confidence, is_callable_kind, is_callable_scope, is_intrinsic_jsx_tag,
	is_relative_specifier, match_path_alias, nested_type_root, nested_type_short, receiver_hint,
	unquote_string_literal,
};

pub(super) struct DiscoveredTsFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
}

struct DecoratorCallee<'src> {
	name: &'src [u8],
	args: Option<Node<'src>>,
}

pub(super) struct TsDiscover<'src> {
	pub(super) module: Moniker,
	pub(super) anchor: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) presets: &'src super::super::Presets,
	pub(super) export_ranges: Vec<(u32, u32)>,
	pub(super) local_scope: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
	pub(super) imports: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
	pub(super) import_targets: RefCell<HashMap<Vec<u8>, Moniker>>,
	pub(super) type_table: HashMap<Vec<u8>, Moniker>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry>,
	pub(super) nested_funcs: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
}

enum NodeShape<'src> {
	Symbol(Symbol<'src>),
	Annotation { kind: &'static [u8] },
	Skip,
	Recurse,
}

struct Symbol<'src> {
	moniker: Moniker,
	kind: &'static [u8],
	visibility: &'static [u8],
	signature: Option<Vec<u8>>,
	body: Option<Node<'src>>,
	position: Position,
	annotated_by: Vec<RefSpec>,
}

struct RefSpec {
	kind: &'static [u8],
	target: Moniker,
	confidence: &'static [u8],
	position: Position,
	receiver_hint: &'static [u8],
	alias: &'static [u8],
}

struct SdkBuilder {
	root: Moniker,
	defs: Vec<DiscoveredDef>,
	refs: Vec<ResolvedRef>,
	seen_defs: HashSet<Moniker>,
}

impl SdkBuilder {
	fn new(root: Moniker) -> Self {
		Self {
			root,
			defs: Vec::new(),
			refs: Vec::new(),
			seen_defs: HashSet::new(),
		}
	}

	fn add_def(
		&mut self,
		moniker: Moniker,
		kind: &'static [u8],
		parent: &Moniker,
		position: Option<Position>,
	) -> Result<(), ()> {
		self.add_def_attrs(moniker, kind, parent, position, &DefAttrs::default())
	}

	fn add_def_attrs(
		&mut self,
		moniker: Moniker,
		kind: &'static [u8],
		parent: &Moniker,
		position: Option<Position>,
		attrs: &DefAttrs<'_>,
	) -> Result<(), ()> {
		if !self.seen_defs.insert(moniker.clone()) {
			return Err(());
		}
		let name = moniker
			.as_view()
			.segments()
			.last()
			.map(|segment| segment.name.to_vec())
			.unwrap_or_default();
		let (call_name, call_arity) = callable_metadata(kind, &name, attrs);
		self.defs.push(DiscoveredDef {
			moniker,
			parent: parent.clone(),
			namespace: namespace_for_kind(kind),
			name,
			kind,
			visibility: visibility_attr(attrs.visibility),
			signature: attrs.signature.to_vec(),
			position,
			call_name,
			call_arity,
		});
		Ok(())
	}

	fn add_ref_attrs(
		&mut self,
		source: &Moniker,
		target: Moniker,
		kind: &'static [u8],
		position: Option<Position>,
		attrs: &RefAttrs<'_>,
	) -> Result<(), ()> {
		let (call_name, call_arity) = ref_call_metadata(kind, &target, attrs);
		self.refs.push(ResolvedRef {
			source: source.clone(),
			target,
			kind,
			position,
			confidence: confidence_attr(attrs.confidence),
			hints: RefHints {
				receiver_hint: attrs.receiver_hint.to_vec(),
				alias: attrs.alias.to_vec(),
				namespace: Some(namespace_for_ref(kind)),
				call_name,
				call_arity,
			},
		});
		Ok(())
	}

	fn contains(&self, moniker: &Moniker) -> bool {
		moniker == &self.root || self.seen_defs.contains(moniker)
	}

	fn finish(self) -> DiscoveredTsFile {
		DiscoveredTsFile {
			root: self.root,
			defs: self.defs,
			refs: self.refs,
		}
	}
}

struct SdkWalker<'a> {
	discover: &'a TsDiscover<'a>,
	source: &'a [u8],
}

struct PendingAnnotation {
	kind: &'static [u8],
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

impl<'a> SdkWalker<'a> {
	fn new(discover: &'a TsDiscover<'a>, source: &'a [u8]) -> Self {
		Self { discover, source }
	}

	fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let mut cursor = node.walk();
		let mut pending: Option<PendingAnnotation> = None;
		for child in node.children(&mut cursor) {
			match self.discover.classify(child, scope, self.source, graph) {
				NodeShape::Annotation { kind } => {
					self.extend_or_flush(&mut pending, kind, child, scope, graph);
				}
				NodeShape::Symbol(sym) => {
					self.flush_pending(&mut pending, scope, graph);
					self.emit_symbol(child, scope, sym, graph);
				}
				NodeShape::Skip => self.flush_pending(&mut pending, scope, graph),
				NodeShape::Recurse => {
					self.flush_pending(&mut pending, scope, graph);
					self.walk(child, scope, graph);
				}
			}
		}
		self.flush_pending(&mut pending, scope, graph);
	}

	fn dispatch(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		match self.discover.classify(node, scope, self.source, graph) {
			NodeShape::Annotation { kind } => self.emit_annotation_range(
				kind,
				node.start_byte() as u32,
				node.end_byte() as u32,
				scope,
				graph,
			),
			NodeShape::Symbol(sym) => self.emit_symbol(node, scope, sym, graph),
			NodeShape::Skip => {}
			NodeShape::Recurse => self.walk(node, scope, graph),
		}
	}

	fn extend_or_flush(
		&self,
		pending: &mut Option<PendingAnnotation>,
		kind: &'static [u8],
		child: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) {
		let start_row = child.start_position().row;
		let end_row = child.end_position().row;
		let start_byte = child.start_byte() as u32;
		let end_byte = child.end_byte() as u32;
		if let Some(p) = pending.as_mut() {
			if p.kind == kind && start_row <= p.end_row + 1 {
				p.end_byte = end_byte;
				p.end_row = end_row;
				return;
			}
			self.emit_annotation_range(p.kind, p.start_byte, p.end_byte, scope, graph);
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
		graph: &mut SdkBuilder,
	) {
		if let Some(p) = pending.take() {
			self.emit_annotation_range(p.kind, p.start_byte, p.end_byte, scope, graph);
		}
	}

	fn emit_symbol(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		sym: Symbol<'_>,
		graph: &mut SdkBuilder,
	) {
		let Symbol {
			moniker: m,
			kind,
			visibility,
			signature,
			body,
			position,
			annotated_by,
		} = sym;

		let attrs = DefAttrs {
			visibility,
			signature: signature.as_deref().unwrap_or_default(),
			..DefAttrs::default()
		};
		let parent = m
			.parent()
			.filter(|parent| parent != scope && graph.contains(parent))
			.unwrap_or_else(|| scope.clone());
		if graph
			.add_def_attrs(m.clone(), kind, &parent, Some(position), &attrs)
			.is_err()
		{
			return;
		}

		for r in annotated_by {
			let attrs = RefAttrs {
				confidence: r.confidence,
				receiver_hint: r.receiver_hint,
				alias: r.alias,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(&m, r.target, r.kind, Some(r.position), &attrs);
		}

		self.discover
			.before_body(node, kind, &m, self.source, graph);
		if let Some(body_node) = body {
			self.walk(body_node, &m, graph);
		}
		self.discover.after_body(kind, &m);
		self.discover
			.on_symbol_emitted(node, kind, &m, self.source, graph);
	}

	fn emit_annotation_range(
		&self,
		kind: &'static [u8],
		start_byte: u32,
		end_byte: u32,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) {
		let m = extend_segment_u32(scope, kind, start_byte);
		let _ = graph.add_def(m, kind, scope, Some((start_byte, end_byte)));
	}
}

impl<'a> TsDiscover<'a> {
	pub(super) fn run(
		module: Moniker,
		anchor: Moniker,
		source_bytes: &'a [u8],
		deep: bool,
		presets: &'a super::super::Presets,
		root: Node<'_>,
	) -> DiscoveredTsFile {
		let export_ranges = collect_export_ranges(root);
		let mut callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry> = HashMap::new();
		collect_callable_table(root, source_bytes, &module, &mut callable_table);
		let mut type_table: HashMap<Vec<u8>, Moniker> = HashMap::new();
		collect_type_table(root, source_bytes, &module, &mut type_table);
		let discover = Self {
			module: module.clone(),
			anchor,
			source_bytes,
			deep,
			presets,
			export_ranges,
			local_scope: RefCell::new(Vec::new()),
			imports: RefCell::new(HashMap::new()),
			import_targets: RefCell::new(HashMap::new()),
			type_table,
			callable_table,
			nested_funcs: RefCell::new(Vec::new()),
		};
		let mut builder = SdkBuilder::new(module.clone());
		SdkWalker::new(&discover, source_bytes).walk(root, &module, &mut builder);
		builder.finish()
	}

	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		match node.kind() {
			"comment" => NodeShape::Annotation {
				kind: kinds::COMMENT,
			},
			"import_statement" => {
				self.handle_import(node, scope, graph);
				NodeShape::Skip
			}
			"export_statement" => self.classify_export(node, scope, source, graph),
			"class_declaration" | "abstract_class_declaration" => {
				self.classify_class(node, scope, source, None, None)
			}
			"interface_declaration" => self.classify_interface(node, scope, source),
			"enum_declaration" => self.classify_enum(node, scope, source),
			"type_alias_declaration" => self.classify_type_alias(node, scope, source, graph),
			"function_declaration" | "generator_function_declaration" => {
				self.classify_function_decl(node, scope, source)
			}
			"method_definition" | "method_signature" => self.classify_method(node, scope, source),
			"public_field_definition" | "property_signature" => {
				self.classify_field(node, scope, source)
			}
			"lexical_declaration" | "variable_declaration" => {
				self.handle_lexical(node, scope, graph);
				NodeShape::Skip
			}
			"call_expression" => {
				self.handle_call(node, scope, graph);
				NodeShape::Skip
			}
			"new_expression" => {
				self.handle_new(node, scope, graph);
				NodeShape::Skip
			}
			"decorator" => {
				self.handle_decorator(node, scope, graph);
				NodeShape::Skip
			}
			"arrow_function" | "function_expression" => {
				self.classify_inline_callable(node, scope, source, graph)
			}
			"pair" => self.classify_pair(node, scope, source),
			"catch_clause" => {
				self.handle_catch_clause(node, scope, graph);
				NodeShape::Skip
			}
			"for_in_statement" | "for_of_statement" => {
				self.handle_for_in(node, scope, graph);
				NodeShape::Skip
			}
			"type_annotation"
			| "type_arguments"
			| "union_type"
			| "intersection_type"
			| "lookup_type"
			| "index_type_query"
			| "type_query"
			| "generic_type"
			| "nested_type_identifier" => {
				self.emit_uses_type_recursive(node, scope, graph);
				NodeShape::Skip
			}
			"return_statement"
			| "spread_element"
			| "parenthesized_expression"
			| "template_substitution"
			| "arguments"
			| "array" => {
				self.emit_reads_in_children(node, scope, graph);
				NodeShape::Skip
			}
			"binary_expression" | "assignment_expression" => {
				self.dispatch_fields(node, scope, graph, &["left", "right"]);
				NodeShape::Skip
			}
			"unary_expression" | "update_expression" => {
				self.dispatch_fields(node, scope, graph, &["argument"]);
				NodeShape::Skip
			}
			"ternary_expression" => {
				self.dispatch_fields(
					node,
					scope,
					graph,
					&["condition", "consequence", "alternative"],
				);
				NodeShape::Skip
			}
			"member_expression" | "subscript_expression" => {
				self.dispatch_fields(node, scope, graph, &["object"]);
				NodeShape::Skip
			}
			"shorthand_property_identifier" => {
				self.emit_read_at(node, scope, graph);
				NodeShape::Skip
			}
			"jsx_expression" => {
				self.emit_reads_in_children(node, scope, graph);
				NodeShape::Skip
			}
			"jsx_opening_element" | "jsx_self_closing_element" => {
				self.handle_jsx_element(node, scope, graph);
				NodeShape::Skip
			}
			_ => NodeShape::Recurse,
		}
	}

	fn before_body(
		&self,
		node: Node<'_>,
		kind: &[u8],
		moniker: &Moniker,
		_source: &[u8],
		graph: &mut SdkBuilder,
	) {
		if !is_callable_kind(kind) {
			return;
		}
		if let Some(rt) = node.child_by_field_name("return_type") {
			self.emit_uses_type_recursive(rt, moniker, graph);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			self.bind_and_emit_params(params, moniker, graph);
		}
		if let Some(p) = node.child_by_field_name("parameter") {
			self.bind_and_emit_param_leaf(p, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if is_callable_kind(kind) {
			self.pop_local_scope();
		}
	}

	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		_source: &[u8],
		graph: &mut SdkBuilder,
	) {
		if sym_kind == kinds::CLASS
			|| sym_kind == kinds::INTERFACE
			|| sym_kind == kinds::FUNCTION
			|| sym_kind == kinds::METHOD
			|| sym_kind == kinds::CONSTRUCTOR
			|| sym_kind == kinds::FIELD
		{
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				if c.kind() == "decorator" {
					self.walk_decorator_args(c, sym_moniker, graph);
				}
			}
		}
		if sym_kind == kinds::ENUM {
			self.emit_enum_constants(node, sym_moniker, graph);
		}
		if sym_kind == kinds::FIELD {
			if let Some(tp) = node.child_by_field_name("type") {
				self.emit_uses_type_recursive(tp, sym_moniker, graph);
			}
			if let Some(value) = node.child_by_field_name("value") {
				self.recurse_subtree(value, sym_moniker, graph);
			}
		}
	}
}

impl<'src_lang> TsDiscover<'src_lang> {
	fn classify_class<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		name_override: Option<&'static [u8]>,
		visibility_override: Option<&'static [u8]>,
	) -> NodeShape<'src> {
		let name: &[u8] = if let Some(n) = name_override {
			n
		} else {
			let Some(name_node) = node.child_by_field_name("name") else {
				return NodeShape::Recurse;
			};
			node_slice(name_node, source)
		};
		let moniker = extend_segment(scope, kinds::CLASS, name);
		if is_callable_scope(scope, &self.module) {
			self.bind_local(name, moniker.clone());
		}

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"decorator" => self.collect_decorator_ref(child, &mut annotated_by),
				"class_heritage" => {
					self.collect_heritage_refs_from_clauses(child, &mut annotated_by)
				}
				_ => {}
			}
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::CLASS,
			visibility: visibility_override.unwrap_or_else(|| self.module_visibility(node)),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_interface<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::INTERFACE, name);
		if is_callable_scope(scope, &self.module) {
			self.bind_local(name, moniker.clone());
		}

		let mut annotated_by: Vec<RefSpec> = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if matches!(child.kind(), "extends_type_clause" | "extends_clause") {
				self.emit_heritage_refs_collect(child, kinds::EXTENDS, &mut annotated_by);
			}
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::INTERFACE,
			visibility: self.module_visibility(node),
			signature: None,
			body: node.child_by_field_name("body"),
			position: node_position(node),
			annotated_by,
		})
	}

	fn classify_enum<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::ENUM, name);
		if is_callable_scope(scope, &self.module) {
			self.bind_local(name, moniker.clone());
		}
		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::ENUM,
			visibility: self.module_visibility(node),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_type_alias<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::TYPE, name);
		if is_callable_scope(scope, &self.module) {
			self.bind_local(name, moniker.clone());
		}
		if let Some(value) = node.child_by_field_name("value") {
			self.emit_uses_type_recursive(value, &moniker, graph);
		}
		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::TYPE,
			visibility: self.module_visibility(node),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_function_decl<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		self.callable_symbol(
			node,
			node,
			name,
			kinds::FUNCTION,
			scope,
			self.module_visibility(node),
		)
	}

	fn classify_method<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let kind: &'static [u8] = if name == b"constructor" {
			kinds::CONSTRUCTOR
		} else {
			kinds::METHOD
		};
		let vis = class_member_visibility(node, source);
		self.callable_symbol(node, node, name, kind, scope, vis)
	}

	fn classify_field<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		let Some(name_node) = node.child_by_field_name("name") else {
			return NodeShape::Recurse;
		};
		let name = node_slice(name_node, source);
		let moniker = extend_segment(scope, kinds::FIELD, name);

		NodeShape::Symbol(Symbol {
			moniker,
			kind: kinds::FIELD,
			visibility: class_member_visibility(node, source),
			signature: None,
			body: None,
			position: node_position(node),
			annotated_by: Vec::new(),
		})
	}

	fn classify_inline_callable<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		if self.deep && is_callable_scope(scope, &self.module) {
			let name = anonymous_callback_name(node);
			return self.callable_symbol(
				node,
				node,
				&name,
				kinds::FUNCTION,
				scope,
				kinds::VIS_NONE,
			);
		}
		self.push_local_scope();
		if let Some(params) = node.child_by_field_name("parameters") {
			self.bind_and_emit_params(params, scope, graph);
		}
		if let Some(p) = node.child_by_field_name("parameter") {
			self.bind_and_emit_param_leaf(p, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
		self.pop_local_scope();
		let _ = source;
		NodeShape::Skip
	}

	fn classify_pair<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
	) -> NodeShape<'src> {
		if self.deep && is_callable_scope(scope, &self.module) {
			let key = node.child_by_field_name("key");
			let value = node.child_by_field_name("value");
			if let (Some(k), Some(v)) = (key, value)
				&& k.kind() == "property_identifier"
				&& (v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				let name = node_slice(k, source);
				return self.callable_symbol(
					v,
					node,
					name,
					kinds::FUNCTION,
					scope,
					kinds::VIS_PUBLIC,
				);
			}
		}
		NodeShape::Recurse
	}

	fn classify_export<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		if node.child_by_field_name("source").is_some() {
			self.handle_reexport(node, scope, graph);
			return NodeShape::Skip;
		}
		self.handle_bare_reexport(node, scope, graph);
		let mut has_default = false;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "default" {
				has_default = true;
				break;
			}
		}
		if has_default {
			let mut cursor = node.walk();
			for c in node.children(&mut cursor) {
				match c.kind() {
					"function_expression" | "arrow_function" => {
						return self.callable_symbol(
							c,
							c,
							b"default",
							kinds::FUNCTION,
							scope,
							kinds::VIS_PUBLIC,
						);
					}
					"class" | "class_declaration" => {
						return self.classify_class(
							c,
							scope,
							source,
							Some(b"default"),
							Some(kinds::VIS_PUBLIC),
						);
					}
					_ => {}
				}
			}
		}
		NodeShape::Recurse
	}

	fn callable_symbol<'src>(
		&self,
		callable_node: Node<'src>,
		anchor_node: Node<'src>,
		name: &[u8],
		kind: &'static [u8],
		scope: &Moniker,
		visibility: &'static [u8],
	) -> NodeShape<'src> {
		let slots = callable_param_slots(callable_node, self.source_bytes);
		let moniker = extend_callable_slots(scope, kind, name, &slots);

		self.push_local_scope();
		let body = callable_node.child_by_field_name("body");
		if let Some(body) = body {
			self.hoist_nested_funcs(body, &moniker);
		}

		NodeShape::Symbol(Symbol {
			moniker,
			kind,
			visibility,
			signature: None,
			body,
			position: node_position(anchor_node),
			annotated_by: Vec::new(),
		})
	}

	fn emit_enum_constants(&self, enum_node: Node<'_>, parent: &Moniker, graph: &mut SdkBuilder) {
		let Some(body) = enum_node.child_by_field_name("body") else {
			return;
		};
		let mut cursor = body.walk();
		for member in body.named_children(&mut cursor) {
			if member.kind() != "enum_assignment" && member.kind() != "property_identifier" {
				continue;
			}
			let name_node = if member.kind() == "enum_assignment" {
				member.child_by_field_name("name").unwrap_or(member)
			} else {
				member
			};
			let name = node_slice(name_node, self.source_bytes);
			if name.is_empty() {
				continue;
			}
			let m = extend_segment(parent, kinds::ENUM_CONSTANT, name);
			let _ = graph.add_def(m, kinds::ENUM_CONSTANT, parent, Some(node_position(member)));
		}
	}

	fn bind_and_emit_params(&self, params: Node<'_>, callable: &Moniker, graph: &mut SdkBuilder) {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			match child.kind() {
				"required_parameter" | "optional_parameter" => {
					if let Some(p) = child.child_by_field_name("pattern") {
						self.bind_and_emit_param_leaf(p, callable, graph);
					}
					if let Some(t) = child.child_by_field_name("type") {
						self.emit_uses_type_recursive(t, callable, graph);
					}
				}
				"rest_pattern" => {
					self.bind_and_emit_param_leaf(child, callable, graph);
				}
				_ => {}
			}
		}
	}

	fn bind_and_emit_param_leaf(&self, pat: Node<'_>, callable: &Moniker, graph: &mut SdkBuilder) {
		for name in collect_binding_names(pat, self.source_bytes) {
			let m = extend_segment(callable, kinds::PARAM, &name);
			self.bind_local(&name, m.clone());
			if self.deep {
				let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
			}
		}
	}

	fn handle_lexical(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let inside_callable = is_callable_scope(scope, &self.module);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "variable_declarator" {
				continue;
			}
			self.handle_variable_declarator(child, scope, inside_callable, graph);
		}
	}

	fn handle_variable_declarator(
		&self,
		decl: Node<'_>,
		scope: &Moniker,
		inside_callable: bool,
		graph: &mut SdkBuilder,
	) {
		let Some(name_node) = decl.child_by_field_name("name") else {
			return;
		};
		let value = decl.child_by_field_name("value");
		let type_annot = decl.child_by_field_name("type");

		let names = collect_binding_names(name_node, self.source_bytes);

		let module_vis = self.module_visibility(decl);
		for name in &names {
			if inside_callable {
				self.bind_local(name, extend_segment(scope, kinds::LOCAL, name));
			}
			let (kind, emit) = if inside_callable {
				(kinds::LOCAL, self.deep)
			} else if let Some(v) =
				value.filter(|v| v.kind() == "arrow_function" || v.kind() == "function_expression")
			{
				let visibility = module_vis;
				let slots = callable_param_slots(v, self.source_bytes);
				let m = extend_callable_slots(scope, kinds::FUNCTION, name, &slots);
				let attrs = crate::core::code_graph::DefAttrs {
					visibility,
					..crate::core::code_graph::DefAttrs::default()
				};
				let _ = graph.add_def_attrs(
					m.clone(),
					kinds::FUNCTION,
					scope,
					Some(node_position(decl)),
					&attrs,
				);
				self.push_local_scope();
				if let Some(rt) = v.child_by_field_name("return_type") {
					self.emit_uses_type_recursive(rt, &m, graph);
				}
				if let Some(params) = v.child_by_field_name("parameters") {
					self.bind_and_emit_params(params, &m, graph);
				}
				if let Some(p) = v.child_by_field_name("parameter") {
					self.bind_and_emit_param_leaf(p, &m, graph);
				}
				if let Some(body) = v.child_by_field_name("body") {
					self.walk_children(body, &m, graph);
				}
				self.pop_local_scope();
				continue;
			} else {
				(kinds::CONST, true)
			};
			if emit {
				let m = extend_segment(scope, kind, name);
				let attrs = crate::core::code_graph::DefAttrs {
					visibility: if inside_callable {
						kinds::VIS_NONE
					} else {
						module_vis
					},
					..crate::core::code_graph::DefAttrs::default()
				};
				let _ =
					graph.add_def_attrs(m.clone(), kind, scope, Some(node_position(decl)), &attrs);
				if kind == kinds::CONST
					&& type_annot.is_none()
					&& let Some((target, confidence)) = self.value_call_target(value, scope)
				{
					let ref_attrs = RefAttrs {
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						&m,
						target,
						kinds::RETURNS_TYPE,
						Some(node_position(decl)),
						&ref_attrs,
					);
				}
			}
		}

		if let Some(tp) = type_annot {
			self.emit_uses_type_recursive(tp, scope, graph);
		}
		if let Some(v) = value {
			if v.kind() == "identifier" {
				self.emit_read_at(v, scope, graph);
			} else {
				self.recurse_subtree(v, scope, graph);
			}
		}
	}

	fn handle_catch_clause(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		if is_callable_scope(scope, &self.module)
			&& let Some(p) = node.child_by_field_name("parameter")
		{
			self.bind_and_emit_param_leaf(p, scope, graph);
		}
		if let Some(body) = node.child_by_field_name("body") {
			self.walk_children(body, scope, graph);
		}
	}

	fn handle_for_in(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		if is_callable_scope(scope, &self.module) {
			let mut cursor = node.walk();
			for c in node.named_children(&mut cursor) {
				let binding_node = match c.kind() {
					"identifier" | "array_pattern" | "object_pattern" => Some(c),
					"lexical_declaration" | "variable_declaration" => {
						let mut decl_cursor = c.walk();
						c.named_children(&mut decl_cursor)
							.find(|decl| decl.kind() == "variable_declarator")
							.and_then(|decl| decl.child_by_field_name("name"))
					}
					_ => None,
				};
				let Some(binding_node) = binding_node else {
					continue;
				};
				for name in collect_binding_names(binding_node, self.source_bytes) {
					let m = extend_segment(scope, kinds::LOCAL, &name);
					self.bind_local(&name, m.clone());
					if self.deep {
						let _ = graph.add_def(
							m,
							kinds::LOCAL,
							scope,
							Some(node_position(binding_node)),
						);
					}
				}
				break;
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn call_identifier_target(
		&self,
		name: &[u8],
		scope: &Moniker,
		confidence: &'static [u8],
	) -> (Moniker, &'static [u8]) {
		if confidence == kinds::CONF_LOCAL {
			let t = self
				.lookup_local_binding(name)
				.unwrap_or_else(|| extend_segment(scope, kinds::LOCAL, name));
			(t, confidence)
		} else if confidence == kinds::CONF_NAME_MATCH {
			if let Some(m) = self.lookup_nested_func(name) {
				(m, kinds::CONF_RESOLVED)
			} else if let Some(m) = self.lookup_callable(name) {
				(m, kinds::CONF_RESOLVED)
			} else if is_global_value(name) {
				(
					external_runtime_target(&self.module, kinds::FUNCTION, name),
					kinds::CONF_EXTERNAL,
				)
			} else {
				(
					extend_segment(&self.module, kinds::FUNCTION, name),
					confidence,
				)
			}
		} else {
			let base = self.import_or_local_module(name);
			(extend_segment(&base, kinds::FUNCTION, name), confidence)
		}
	}

	fn value_call_target(
		&self,
		value: Option<Node<'_>>,
		scope: &Moniker,
	) -> Option<(Moniker, &'static [u8])> {
		let call = value.filter(|v| v.kind() == "call_expression")?;
		let fn_node = call
			.child_by_field_name("function")
			.filter(|f| f.kind() == "identifier")?;
		let name = node_slice(fn_node, self.source_bytes);
		let confidence = self.name_confidence(name)?;
		Some(self.call_identifier_target(name, scope, confidence))
	}

	fn handle_call(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let pos = node_position(node);
		let Some(fn_node) = node.child_by_field_name("function") else {
			self.walk_children(node, scope, graph);
			return;
		};
		match fn_node.kind() {
			"identifier" => {
				let name = node_slice(fn_node, self.source_bytes);
				match self.name_confidence(name) {
					Some(confidence) => {
						let (target, confidence) =
							self.call_identifier_target(name, scope, confidence);
						let attrs = RefAttrs {
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
					None => {
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
				}
			}
			"member_expression" => {
				if let Some(prop) = fn_node.child_by_field_name("property") {
					let name = node_slice(prop, self.source_bytes);
					if !name.is_empty() {
						let obj_ident = fn_node
							.child_by_field_name("object")
							.filter(|o| o.kind() == "identifier")
							.map(|o| node_slice(o, self.source_bytes));
						let imported_target = obj_ident.and_then(|n| self.lookup_import_target(n));
						let target = imported_target
							.clone()
							.map(|m| extend_segment(&m, kinds::METHOD, name))
							.unwrap_or_else(|| {
								external_runtime_target(&self.module, kinds::METHOD, name)
							});
						let confidence = imported_target
							.as_ref()
							.map(|_| {
								obj_ident
									.map(|n| self.ref_confidence(n))
									.unwrap_or(kinds::CONF_EXTERNAL)
							})
							.unwrap_or(kinds::CONF_EXTERNAL);
						let attrs = RefAttrs {
							receiver_hint: receiver_hint(fn_node, self.source_bytes),
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(
							scope,
							target,
							kinds::METHOD_CALL,
							Some(pos),
							&attrs,
						);
						self.maybe_emit_di_register(node, name, scope, graph, pos);
					}
				}
				if let Some(obj) = fn_node.child_by_field_name("object") {
					if obj.kind() == "identifier" {
						self.emit_read_at(obj, scope, graph);
					} else {
						self.recurse_subtree(obj, scope, graph);
					}
				}
			}
			_ => {}
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.walk_children(args, scope, graph);
		}
	}

	fn handle_new(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let pos = node_position(node);
		if let Some(ctor) = node.child_by_field_name("constructor") {
			let name = match ctor.kind() {
				"identifier" | "type_identifier" => Some(node_slice(ctor, self.source_bytes)),
				"member_expression" => ctor
					.child_by_field_name("property")
					.map(|p| node_slice(p, self.source_bytes)),
				_ => None,
			};
			if let Some(n) = name
				&& !n.is_empty()
			{
				let confidence = match ctor.kind() {
					"identifier" | "type_identifier" => self.ref_confidence(n),
					"member_expression" => ctor
						.child_by_field_name("object")
						.filter(|o| o.kind() == "identifier")
						.map(|o| self.ref_confidence(node_slice(o, self.source_bytes)))
						.unwrap_or(kinds::CONF_NAME_MATCH),
					_ => kinds::CONF_NAME_MATCH,
				};
				let target =
					if confidence == kinds::CONF_IMPORTED || confidence == kinds::CONF_EXTERNAL {
						extend_segment(&self.import_or_local_module(n), kinds::CLASS, n)
					} else if let Some(m) = self.lookup_local_binding(n) {
						m
					} else if let Some(m) = self.type_table.get(n) {
						m.clone()
					} else {
						external_runtime_target(&self.module, kinds::CLASS, n)
					};
				let attrs = RefAttrs {
					confidence: if is_global_type(n) || !self.type_table.contains_key(n) {
						kinds::CONF_EXTERNAL
					} else if self.type_table.values().any(|m| m == &target)
						|| graph.contains(&target)
					{
						kinds::CONF_RESOLVED
					} else {
						confidence
					},
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::INSTANTIATES, Some(pos), &attrs);
			}
		}
		self.walk_children(node, scope, graph);
	}

	fn decorator_callees<'src>(&self, decorator: Node<'src>) -> Vec<DecoratorCallee<'src>>
	where
		'src_lang: 'src,
	{
		let mut out = Vec::new();
		let mut cursor = decorator.walk();
		for ch in decorator.children(&mut cursor) {
			match ch.kind() {
				"identifier" => {
					let name = node_slice(ch, self.source_bytes);
					if !name.is_empty() {
						out.push(DecoratorCallee { name, args: None });
					}
				}
				"call_expression" => {
					if let Some(fn_node) = ch.child_by_field_name("function")
						&& fn_node.kind() == "identifier"
					{
						let name = node_slice(fn_node, self.source_bytes);
						if !name.is_empty() {
							out.push(DecoratorCallee {
								name,
								args: ch.child_by_field_name("arguments"),
							});
						}
					}
				}
				_ => {}
			}
		}
		out
	}

	fn handle_decorator(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let pos = node_position(node);
		for callee in self.decorator_callees(node) {
			let target = extend_segment(&self.module, kinds::FUNCTION, callee.name);
			let attrs = RefAttrs {
				confidence: self.ref_confidence(callee.name),
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
			if let Some(args) = callee.args {
				self.walk_children(args, scope, graph);
			}
		}
	}

	fn walk_decorator_args(
		&self,
		decorator: Node<'_>,
		sym_moniker: &Moniker,
		graph: &mut SdkBuilder,
	) {
		for callee in self.decorator_callees(decorator) {
			if let Some(args) = callee.args {
				self.walk_children(args, sym_moniker, graph);
			}
		}
	}

	fn collect_decorator_ref(&self, decorator: Node<'_>, out: &mut Vec<RefSpec>) {
		let pos = node_position(decorator);
		for callee in self.decorator_callees(decorator) {
			let target = extend_segment(&self.module, kinds::FUNCTION, callee.name);
			out.push(RefSpec {
				kind: kinds::ANNOTATES,
				target,
				confidence: self.ref_confidence(callee.name),
				position: pos,
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn collect_heritage_refs_from_clauses(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			let edge: &'static [u8] = match child.kind() {
				"extends_clause" => kinds::EXTENDS,
				"implements_clause" => kinds::IMPLEMENTS,
				_ => continue,
			};
			self.emit_heritage_refs_collect(child, edge, out);
		}
	}

	fn emit_heritage_refs_collect(
		&self,
		clause: Node<'_>,
		edge: &'static [u8],
		out: &mut Vec<RefSpec>,
	) {
		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			let pos = node_position(c);
			let target_kind = if edge == kinds::IMPLEMENTS {
				kinds::INTERFACE
			} else {
				kinds::CLASS
			};
			let name: Option<&[u8]> = match c.kind() {
				"identifier" | "type_identifier" => Some(node_slice(c, self.source_bytes)),
				"member_expression" => c
					.child_by_field_name("property")
					.map(|p| node_slice(p, self.source_bytes)),
				"generic_type" => generic_short(c, self.source_bytes),
				"nested_type_identifier" => nested_type_short(c, self.source_bytes),
				_ => None,
			};
			let Some(name) = name.filter(|n| !n.is_empty()) else {
				continue;
			};
			let (target, confidence) = if let Some(m) = self.lookup_import_target(name) {
				(m, self.ref_confidence(name))
			} else if let Some(m) = self.type_table.get(name) {
				(m.clone(), self.ref_confidence(name))
			} else {
				(
					external_runtime_target(&self.module, target_kind, name),
					kinds::CONF_EXTERNAL,
				)
			};
			out.push(RefSpec {
				kind: edge,
				target,
				confidence,
				position: pos,
				receiver_hint: b"",
				alias: b"",
			});
		}
	}

	fn emit_uses_type_recursive(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		match node.kind() {
			"type_query" => self.emit_type_query_recursive(node, scope, graph),
			"type_identifier" => {
				let name = node_slice(node, self.source_bytes);
				if name.is_empty() {
					return;
				}
				let target = self
					.lookup_local_binding(name)
					.or_else(|| self.lookup_import_target(name))
					.or_else(|| self.type_table.get(name).cloned())
					.unwrap_or_else(|| external_runtime_target(&self.module, kinds::CLASS, name));
				let attrs = RefAttrs {
					confidence: if is_global_type(name) || !self.type_table.contains_key(name) {
						kinds::CONF_EXTERNAL
					} else {
						self.ref_confidence(name)
					},
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					scope,
					target,
					kinds::USES_TYPE,
					Some(node_position(node)),
					&attrs,
				);
			}
			"nested_type_identifier" => {
				if let Some(name) = nested_type_short(node, self.source_bytes) {
					let root = nested_type_root(node, self.source_bytes).unwrap_or(name);
					let target = self
						.lookup_import_target(root)
						.map(|m| extend_segment(&m, kinds::CLASS, name))
						.unwrap_or_else(|| {
							external_runtime_target(&self.module, kinds::CLASS, name)
						});
					let attrs = RefAttrs {
						confidence: if is_global_type(root) || !self.type_table.contains_key(root) {
							kinds::CONF_EXTERNAL
						} else {
							self.ref_confidence(root)
						},
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&attrs,
					);
				}
			}
			"generic_type" => {
				if let Some(name) = generic_short(node, self.source_bytes) {
					let target = self
						.lookup_import_target(name)
						.or_else(|| self.type_table.get(name).cloned())
						.unwrap_or_else(|| {
							external_runtime_target(&self.module, kinds::CLASS, name)
						});
					let attrs = RefAttrs {
						confidence: if is_global_type(name) || !self.type_table.contains_key(name) {
							kinds::CONF_EXTERNAL
						} else {
							self.ref_confidence(name)
						},
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::USES_TYPE,
						Some(node_position(node)),
						&attrs,
					);
				}
				if let Some(args) = node.child_by_field_name("type_arguments") {
					self.emit_uses_type_recursive(args, scope, graph);
				}
			}
			_ => {
				let mut cursor = node.walk();
				for c in node.children(&mut cursor) {
					self.emit_uses_type_recursive(c, scope, graph);
				}
			}
		}
	}

	fn emit_type_query_recursive(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let Some(identifier) = Self::first_type_query_identifier(node) else {
			return;
		};
		let name = node_slice(identifier, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let (target, confidence) = self.type_query_value_target(name, graph);
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::USES_TYPE,
			Some(node_position(identifier)),
			&attrs,
		);
	}

	fn first_type_query_identifier(node: Node<'_>) -> Option<Node<'_>> {
		if node.kind() == "identifier" {
			return Some(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if let Some(identifier) = Self::first_type_query_identifier(child) {
				return Some(identifier);
			}
		}
		None
	}

	fn type_query_value_target(&self, name: &[u8], graph: &SdkBuilder) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.lookup_local_binding(name) {
			return (target, kinds::CONF_LOCAL);
		}
		if let Some(target) = self.lookup_import_target(name) {
			return (target, self.ref_confidence(name));
		}
		if is_global_value(name) {
			return (
				external_runtime_target(&self.module, kinds::FUNCTION, name),
				kinds::CONF_EXTERNAL,
			);
		}
		if let Some(target) = self.lookup_callable(name) {
			return (target, kinds::CONF_RESOLVED);
		}
		let target = extend_segment(&self.module, kinds::CONST, name);
		let confidence = if graph.contains(&target) {
			kinds::CONF_RESOLVED
		} else {
			self.ref_confidence(name)
		};
		(target, confidence)
	}

	fn emit_reads_in_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() == "identifier" {
				self.emit_read_at(c, scope, graph);
			} else {
				self.recurse_subtree(c, scope, graph);
			}
		}
	}

	fn emit_read_at(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let name = node_slice(node, self.source_bytes);
		if name.is_empty() {
			return;
		}
		let Some(confidence) = self.name_confidence(name) else {
			return;
		};
		let (target, confidence) = if confidence == kinds::CONF_LOCAL {
			(
				self.lookup_local_binding(name)
					.unwrap_or_else(|| extend_segment(scope, kinds::LOCAL, name)),
				confidence,
			)
		} else if let Some(target) = self.lookup_import_target(name) {
			(target, confidence)
		} else if is_global_value(name) {
			(
				external_runtime_target(&self.module, kinds::FUNCTION, name),
				kinds::CONF_EXTERNAL,
			)
		} else if let Some(target) = self.lookup_callable(name) {
			(target, kinds::CONF_RESOLVED)
		} else if graph.contains(&extend_segment(&self.module, kinds::CONST, name)) {
			(
				extend_segment(&self.module, kinds::CONST, name),
				kinds::CONF_RESOLVED,
			)
		} else {
			(
				extend_segment(&self.module, kinds::FUNCTION, name),
				confidence,
			)
		};
		let attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			scope,
			target,
			kinds::READS,
			Some(node_position(node)),
			&attrs,
		);
	}

	fn dispatch_fields(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		fields: &[&str],
	) {
		for f in fields {
			let Some(c) = node.child_by_field_name(f) else {
				continue;
			};
			if c.kind() == "identifier" {
				self.emit_read_at(c, scope, graph);
			} else {
				self.recurse_subtree(c, scope, graph);
			}
		}
	}

	fn handle_jsx_element(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		if let Some(name) = node.child_by_field_name("name")
			&& name.kind() == "identifier"
			&& !is_intrinsic_jsx_tag(node_slice(name, self.source_bytes))
		{
			self.emit_read_at(name, scope, graph);
		}
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"jsx_attribute" => {
					if let Some(v) = c.child_by_field_name("value") {
						self.recurse_subtree(v, scope, graph);
					}
				}
				"jsx_text" => {}
				_ => self.recurse_subtree(c, scope, graph),
			}
		}
	}

	fn handle_import(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let confidence = import_confidence(raw_spec);
		let Some(clause) = find_named_child(node, "import_clause") else {
			let target = self.import_module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		};

		let mut cursor = clause.walk();
		for c in clause.children(&mut cursor) {
			match c.kind() {
				"identifier" => {
					let local_name = node_slice(c, self.source_bytes);
					self.record_import(local_name, confidence);
					let target = self.import_symbol_target(raw_spec, b"default");
					self.record_import_target(local_name, &target);
					let attrs = RefAttrs {
						alias: local_name,
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_SYMBOL,
						Some(pos),
						&attrs,
					);
				}
				"namespace_import" => {
					let alias = first_identifier_text(c, self.source_bytes);
					self.record_import(alias, confidence);
					let target = self.import_module_target(raw_spec);
					self.record_import_target(alias, &target);
					let attrs = RefAttrs {
						alias,
						confidence,
						..RefAttrs::default()
					};
					let _ = graph.add_ref_attrs(
						scope,
						target,
						kinds::IMPORTS_MODULE,
						Some(pos),
						&attrs,
					);
				}
				"named_imports" => {
					let mut nc = c.walk();
					for spec in c.children(&mut nc) {
						if spec.kind() != "import_specifier" {
							continue;
						}
						let Some(name) = spec
							.child_by_field_name("name")
							.map(|n| node_slice(n, self.source_bytes))
							.filter(|n| !n.is_empty())
						else {
							continue;
						};
						let alias = spec
							.child_by_field_name("alias")
							.map(|n| node_slice(n, self.source_bytes))
							.unwrap_or(b"");
						let local = if alias.is_empty() { name } else { alias };
						self.record_import(local, confidence);
						let target = self.import_symbol_target(raw_spec, name);
						self.record_import_target(local, &target);
						let attrs = RefAttrs {
							alias,
							confidence,
							..RefAttrs::default()
						};
						let _ = graph.add_ref_attrs(
							scope,
							target,
							kinds::IMPORTS_SYMBOL,
							Some(pos),
							&attrs,
						);
					}
				}
				_ => {}
			}
		}
	}

	fn handle_reexport(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, self.source_bytes);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);

		let mut has_star = false;
		let mut export_clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			match c.kind() {
				"*" => has_star = true,
				"export_clause" => export_clause = Some(c),
				_ => {}
			}
		}

		let confidence = import_confidence(raw_spec);
		if has_star {
			let target = self.import_module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
			return;
		}

		let Some(clause) = export_clause else { return };
		let mut nc = clause.walk();
		for spec in clause.children(&mut nc) {
			if spec.kind() != "export_specifier" {
				continue;
			}
			let Some(name) = spec
				.child_by_field_name("name")
				.map(|n| node_slice(n, self.source_bytes))
				.filter(|n| !n.is_empty())
			else {
				continue;
			};
			let alias = spec
				.child_by_field_name("alias")
				.map(|n| node_slice(n, self.source_bytes))
				.unwrap_or(b"");
			let target = self.import_symbol_target(raw_spec, name);
			let attrs = RefAttrs {
				alias,
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
		}
	}

	fn handle_bare_reexport(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		for c in node.children(&mut cursor) {
			if c.kind() != "export_clause" {
				continue;
			}
			let mut nc = c.walk();
			for spec in c.children(&mut nc) {
				if spec.kind() != "export_specifier" {
					continue;
				}
				let Some(local) = spec
					.child_by_field_name("name")
					.map(|n| node_slice(n, self.source_bytes))
					.filter(|n| !n.is_empty())
				else {
					continue;
				};
				let Some(target) = self.lookup_import_target(local) else {
					continue;
				};
				let alias = spec
					.child_by_field_name("alias")
					.map(|n| node_slice(n, self.source_bytes))
					.unwrap_or(b"");
				let attrs = RefAttrs {
					alias,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
			}
		}
	}

	fn import_module_target(&self, raw_path: &str) -> Moniker {
		self.import_target(raw_path, None)
	}

	fn import_symbol_target(&self, raw_path: &str, name: &[u8]) -> Moniker {
		self.import_target(raw_path, Some(name))
	}

	fn import_target(&self, raw_path: &str, symbol: Option<&[u8]>) -> Moniker {
		let mut b = if let Some(resolved) = self.resolve_path_alias(raw_path) {
			self.project_rooted_module_builder(&resolved)
		} else if is_relative_specifier(raw_path) {
			self.relative_module_builder(raw_path)
		} else {
			external_pkg_builder(self.module.as_view().project(), raw_path)
		};
		if let Some(sym) = symbol {
			b.segment(kinds::PATH, sym);
		}
		b.build()
	}

	fn resolve_path_alias(&self, spec: &str) -> Option<String> {
		for alias in &self.presets.path_aliases {
			if let Some(captured) = match_path_alias(&alias.pattern, spec) {
				return Some(apply_path_alias(&alias.substitution, captured));
			}
		}
		None
	}

	fn project_rooted_module_builder(&self, path: &str) -> MonikerBuilder {
		super::canonicalize::module_builder_for_path(&self.anchor, path)
	}

	fn relative_module_builder(&self, raw_path: &str) -> MonikerBuilder {
		let importer_view = self.module.as_view();
		let mut b = MonikerBuilder::from_view(importer_view);
		let mut depth = (importer_view.segment_count() as usize).saturating_sub(1);
		b.truncate(depth);

		let mut remainder = match raw_path {
			"." => "./",
			".." => "../",
			other => other,
		};
		while let Some(rest) = remainder.strip_prefix("./") {
			remainder = rest;
		}
		while let Some(rest) = remainder.strip_prefix("../") {
			depth = depth.saturating_sub(1);
			b.truncate(depth);
			remainder = rest;
		}
		let remainder = strip_known_extension(remainder);
		append_module_segments(&mut b, remainder);
		b
	}

	fn maybe_emit_di_register(
		&self,
		call: Node<'_>,
		callee_name: &[u8],
		scope: &Moniker,
		graph: &mut SdkBuilder,
		pos: (u32, u32),
	) {
		if self.presets.di_register_callees.is_empty() {
			return;
		}
		let callee_str = match std::str::from_utf8(callee_name) {
			Ok(s) => s,
			Err(_) => return,
		};
		if !self
			.presets
			.di_register_callees
			.iter()
			.any(|p| p == callee_str)
		{
			return;
		}
		let Some(args) = call.child_by_field_name("arguments") else {
			return;
		};
		let mut cursor = args.walk();
		for c in args.children(&mut cursor) {
			if !c.is_named() {
				continue;
			}
			if let Some(name) = self.find_di_factory(c) {
				let target = extend_segment(&self.module, kinds::CLASS, name);
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::DI_REGISTER, Some(pos), &attrs);
			}
		}
	}

	fn find_di_factory<'a>(&'a self, node: Node<'a>) -> Option<&'a [u8]> {
		match node.kind() {
			"identifier" => {
				let name = node_slice(node, self.source_bytes);
				(!name.is_empty()).then_some(name)
			}
			"call_expression" => {
				let fn_node = node.child_by_field_name("function")?;
				match fn_node.kind() {
					"member_expression" => fn_node
						.child_by_field_name("object")
						.and_then(|obj| self.find_di_factory(obj)),
					"identifier" => {
						let inner_args = node.child_by_field_name("arguments")?;
						let mut cur = inner_args.walk();
						for c in inner_args.children(&mut cur) {
							if !c.is_named() {
								continue;
							}
							if let Some(name) = self.find_di_factory(c) {
								return Some(name);
							}
						}
						None
					}
					_ => None,
				}
			}
			_ => None,
		}
	}

	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let walker = SdkWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}

	fn walk_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let walker = SdkWalker::new(self, self.source_bytes);
		walker.walk(node, scope, graph);
	}

	fn push_local_scope(&self) {
		self.local_scope.borrow_mut().push(HashMap::new());
		self.nested_funcs.borrow_mut().push(HashMap::new());
	}

	fn pop_local_scope(&self) {
		self.local_scope.borrow_mut().pop();
		self.nested_funcs.borrow_mut().pop();
	}

	fn hoist_nested_funcs(&self, body: Node<'_>, parent: &Moniker) {
		let mut cursor = body.walk();
		let mut nf = self.nested_funcs.borrow_mut();
		let Some(top) = nf.last_mut() else {
			return;
		};
		for child in body.named_children(&mut cursor) {
			if let Some((name, slots)) = function_decl_info(child, self.source_bytes) {
				let m = extend_callable_slots(parent, kinds::FUNCTION, name, &slots);
				top.insert(name.to_vec(), m);
			}
		}
	}

	fn bind_local(&self, name: &[u8], def: Moniker) {
		if let Some(top) = self.local_scope.borrow_mut().last_mut() {
			top.insert(name.to_vec(), def);
		}
	}

	fn lookup_local_binding(&self, name: &[u8]) -> Option<Moniker> {
		self.local_scope
			.borrow()
			.iter()
			.rev()
			.find_map(|frame| frame.get(name).cloned())
	}

	fn is_local_name(&self, name: &[u8]) -> bool {
		self.lookup_local_binding(name).is_some()
	}

	fn name_confidence(&self, name: &[u8]) -> Option<&'static [u8]> {
		if self.is_local_name(name) {
			return if self.deep {
				Some(kinds::CONF_LOCAL)
			} else {
				None
			};
		}
		Some(
			self.import_confidence_for(name)
				.unwrap_or(kinds::CONF_NAME_MATCH),
		)
	}

	fn import_confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.imports.borrow().get(name).copied()
	}

	fn record_import(&self, name: &[u8], confidence: &'static [u8]) {
		if name.is_empty() {
			return;
		}
		self.imports.borrow_mut().insert(name.to_vec(), confidence);
	}

	fn record_import_target(&self, name: &[u8], target: &Moniker) {
		if name.is_empty() {
			return;
		}
		self.import_targets
			.borrow_mut()
			.insert(name.to_vec(), target.clone());
	}

	fn lookup_import_target(&self, name: &[u8]) -> Option<Moniker> {
		self.import_targets.borrow().get(name).cloned()
	}

	fn lookup_import_module(&self, name: &[u8]) -> Option<Moniker> {
		let target = self.import_targets.borrow().get(name).cloned()?;
		let view = target.as_view();
		let last = view.segments().last()?;
		if last.kind == crate::lang::kinds::PATH {
			let count = view.segment_count() as usize;
			let mut b = crate::core::moniker::MonikerBuilder::from_view(view);
			b.truncate(count - 1);
			Some(b.build())
		} else {
			Some(target)
		}
	}

	fn import_or_local_module(&self, name: &[u8]) -> Moniker {
		self.lookup_import_module(name)
			.unwrap_or_else(|| self.module.clone())
	}

	fn lookup_callable(&self, name: &[u8]) -> Option<Moniker> {
		let entry = self
			.callable_table
			.get(&(self.module.clone(), name.to_vec()))?;
		Some(extend_segment(&self.module, entry.kind, &entry.segment))
	}

	fn lookup_nested_func(&self, name: &[u8]) -> Option<Moniker> {
		for frame in self.nested_funcs.borrow().iter().rev() {
			if let Some(m) = frame.get(name) {
				return Some(m.clone());
			}
		}
		None
	}

	fn ref_confidence(&self, name: &[u8]) -> &'static [u8] {
		self.import_confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH)
	}

	fn module_visibility(&self, node: Node<'_>) -> &'static [u8] {
		let start = node.start_byte() as u32;
		if self
			.export_ranges
			.iter()
			.any(|(a, b)| *a <= start && start < *b)
		{
			kinds::VIS_PUBLIC
		} else {
			kinds::VIS_MODULE
		}
	}
}
