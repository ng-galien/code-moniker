// code-moniker: ignore-file[smell-feature-envy-local, smell-long-parameter-list, smell-data-clumps-param-names, smell-clone-reflex, smell-harmonious-method-size]
// TODO(smell): split SQL discovery into definition walking, callable metadata, and reference emission units before enabling these guardrails here.
use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser, Tree};

use crate::core::code_graph::Position;
use crate::core::moniker::Moniker;
use crate::lang::ParsedDocument;
use crate::lang::sdk::{DiscoveredDef, Namespace, RefHints, ResolvedRef};
use crate::lang::tree_util::{find_descendant, find_named_child, node_position, node_slice};

use crate::lang::callable::{
	CallableSlot, extend_callable_slots, extend_segment_u32, join_bytes_with_comma,
	slot_signature_bytes,
};

use super::super::canonicalize::{extend_segment, maybe_schema};
use super::super::kinds;

use find_named_child as find_child;

pub(in crate::lang::sql) fn new_sql_parser() -> Parser {
	let mut parser = Parser::new();
	parser
		.set_language(&tree_sitter_postgres::LANGUAGE.into())
		.unwrap_or_else(|err| {
			panic!("failed to load tree-sitter-postgres SQL grammar: {err}");
		});
	parser
}

pub(in crate::lang::sql) fn parse(source: &str) -> Tree {
	parse_with(&mut new_sql_parser(), source)
}

pub(super) fn parse_with(parser: &mut Parser, source: &str) -> Tree {
	parser.parse(source, None).unwrap_or_else(|| {
		panic!("tree-sitter parse returned None on a non-cancelled call");
	})
}

pub(in crate::lang::sql) type CallableSearchPaths = HashMap<Moniker, Option<Vec<u8>>>;
type CallableMetadata = HashMap<Moniker, (Vec<u8>, Option<usize>)>;

pub(super) struct DiscoveredSqlFile {
	pub(super) root: Moniker,
	pub(super) defs: Vec<DiscoveredDef>,
	pub(super) refs: Vec<ResolvedRef>,
}

pub(super) struct SqlDiscover;

struct SqlSymbol<'src> {
	moniker: Moniker,
	kind: &'static [u8],
	signature: Vec<u8>,
	call_name: Vec<u8>,
	call_arity: Option<usize>,
	body: Option<Node<'src>>,
	position: Position,
}

enum SqlNodeShape<'src> {
	Annotation,
	Symbol(SqlSymbol<'src>),
	Skip,
	Recurse,
}

pub(in crate::lang::sql) struct SqlBuilder {
	root: Moniker,
	defs: Vec<DiscoveredDef>,
	refs: Vec<ResolvedRef>,
	seen_defs: HashSet<Moniker>,
}

fn namespace_for(kind: &[u8]) -> Namespace {
	match kind {
		b"schema" | b"module" => Namespace::Module,
		b"table" | b"view" => Namespace::Type,
		_ => Namespace::Value,
	}
}

impl SqlBuilder {
	fn new(root: Moniker) -> Self {
		Self {
			root,
			defs: Vec::new(),
			refs: Vec::new(),
			seen_defs: HashSet::new(),
		}
	}

	fn add_definition(
		&mut self,
		moniker: Moniker,
		kind: &'static [u8],
		signature: Vec<u8>,
		position: Position,
		scope: &Moniker,
	) -> bool {
		self.add_symbol(
			&SqlSymbol {
				moniker,
				kind,
				signature,
				call_name: Vec::new(),
				call_arity: None,
				body: None,
				position,
			},
			scope,
		)
	}

	fn add_symbol(&mut self, symbol: &SqlSymbol<'_>, scope: &Moniker) -> bool {
		if self.contains(&symbol.moniker) {
			return false;
		}
		let parent = symbol
			.moniker
			.parent()
			.filter(|parent| parent != scope && self.contains(parent))
			.unwrap_or_else(|| scope.clone());
		if !self.contains(&parent) || !parent.is_ancestor_of(&symbol.moniker) {
			return false;
		}
		self.seen_defs.insert(symbol.moniker.clone());
		let name = symbol
			.moniker
			.as_view()
			.segments()
			.last()
			.map(|segment| segment.name.to_vec())
			.unwrap_or_default();
		self.defs.push(DiscoveredDef {
			moniker: symbol.moniker.clone(),
			parent,
			namespace: namespace_for(symbol.kind),
			name,
			kind: symbol.kind,
			visibility: kinds::VIS_NONE,
			signature: symbol.signature.clone(),
			position: Some(symbol.position),
			call_name: symbol.call_name.clone(),
			call_arity: symbol.call_arity,
		});
		true
	}

	fn apply_callable_metadata(&mut self, metadata: &CallableMetadata) {
		for definition in &mut self.defs {
			if let Some((call_name, call_arity)) = metadata.get(&definition.moniker) {
				definition.call_name.clone_from(call_name);
				definition.call_arity = *call_arity;
			}
		}
	}

	fn add_comment(&mut self, scope: &Moniker, start: u32, end: u32) {
		let symbol = SqlSymbol {
			moniker: extend_segment_u32(scope, kinds::COMMENT, start),
			kind: kinds::COMMENT,
			signature: Vec::new(),
			call_name: Vec::new(),
			call_arity: None,
			body: None,
			position: (start, end),
		};
		let _ = self.add_symbol(&symbol, scope);
	}

	fn push_ref(&mut self, reference: ResolvedRef) {
		self.refs.push(reference);
	}

	fn contains(&self, moniker: &Moniker) -> bool {
		moniker == &self.root || self.seen_defs.contains(moniker)
	}

	fn finish(self) -> DiscoveredSqlFile {
		DiscoveredSqlFile {
			root: self.root,
			defs: self.defs,
			refs: self.refs,
		}
	}
}

fn resolved_ref(
	source: &Moniker,
	target: Moniker,
	kind: &'static [u8],
	position: Option<Position>,
	confidence: &'static [u8],
	call_name: &[u8],
	call_arity: Option<usize>,
) -> ResolvedRef {
	ResolvedRef {
		source: source.clone(),
		target,
		kind,
		position,
		confidence,
		hints: RefHints {
			receiver_hint: Vec::new(),
			alias: Vec::new(),
			namespace: None,
			call_name: call_name.to_vec(),
			call_arity,
		},
	}
}

struct PendingComment {
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

struct SqlWalker<'a> {
	module: &'a Moniker,
	source_str: &'a str,
	document: Option<&'a ParsedDocument>,
	emit_comments: bool,
	search_paths: &'a CallableSearchPaths,
}

impl SqlWalker<'_> {
	fn walk(&self, node: Node<'_>, scope: &Moniker, builder: &mut SqlBuilder) {
		let mut cursor = node.walk();
		let mut pending = None;
		for child in node.children(&mut cursor) {
			match self.classify(child, scope, builder) {
				SqlNodeShape::Annotation => {
					self.extend_or_flush(&mut pending, child, scope, builder)
				}
				SqlNodeShape::Symbol(symbol) => {
					self.flush_pending(&mut pending, scope, builder);
					self.emit_symbol(child, scope, symbol, builder);
				}
				SqlNodeShape::Skip => self.flush_pending(&mut pending, scope, builder),
				SqlNodeShape::Recurse => {
					self.flush_pending(&mut pending, scope, builder);
					self.walk(child, scope, builder);
				}
			}
		}
		self.flush_pending(&mut pending, scope, builder);
	}

	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		builder: &mut SqlBuilder,
	) -> SqlNodeShape<'src> {
		let source = self.source_str.as_bytes();
		match node.kind() {
			"comment" if self.emit_comments => SqlNodeShape::Annotation,
			"comment" => SqlNodeShape::Skip,
			"CreateSchemaStmt" => classify_schema(node, source, self.module),
			"CreateFunctionStmt" => classify_create_function(node, source, self.module),
			"DefineStmt" if find_descendant(node, "kw_type").is_some() => {
				classify_user_type(node, source, self.module)
			}
			"CreateDomainStmt" => classify_user_type(node, source, self.module),
			"CreateTrigStmt" => classify_trigger(node, source, self.module),
			"CreateStmt" => {
				classify_qualified_relation(node, source, self.module, kinds::TABLE, None)
			}
			"CreateAsStmt" => {
				emit_statement_write(node, source, scope, self.module, self.search_paths, builder);
				classify_qualified_relation(
					node,
					source,
					self.module,
					kinds::TABLE,
					find_child(node, "SelectStmt"),
				)
			}
			"ViewStmt" => classify_qualified_relation(
				node,
				source,
				self.module,
				kinds::VIEW,
				find_child(node, "SelectStmt"),
			),
			"InsertStmt" | "UpdateStmt" | "DeleteStmt" => {
				emit_statement_write(node, source, scope, self.module, self.search_paths, builder);
				emit_statement_reads(node, source, scope, self.module, self.search_paths, builder);
				SqlNodeShape::Recurse
			}
			"SelectStmt" => {
				emit_statement_reads(node, source, scope, self.module, self.search_paths, builder);
				SqlNodeShape::Recurse
			}
			"func_application" => {
				emit_call(node, source, scope, self.module, self.search_paths, builder);
				SqlNodeShape::Recurse
			}
			_ => SqlNodeShape::Recurse,
		}
	}

	fn emit_symbol(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		symbol: SqlSymbol<'_>,
		builder: &mut SqlBuilder,
	) {
		if !builder.add_symbol(&symbol, scope) {
			return;
		}
		if let Some(body) = symbol.body {
			if body.kind() == "SelectStmt" {
				emit_statement_reads(
					body,
					self.source_str.as_bytes(),
					&symbol.moniker,
					self.module,
					self.search_paths,
					builder,
				);
			}
			self.walk(body, &symbol.moniker, builder);
		}
		self.on_symbol_emitted(node, symbol.kind, &symbol.moniker, builder);
	}

	fn extend_or_flush(
		&self,
		pending: &mut Option<PendingComment>,
		child: Node<'_>,
		scope: &Moniker,
		builder: &mut SqlBuilder,
	) {
		let start_row = child.start_position().row;
		let end_row = child.end_position().row;
		let start_byte = child.start_byte() as u32;
		let end_byte = child.end_byte() as u32;
		if let Some(comment) = pending.as_mut() {
			if start_row <= comment.end_row + 1 {
				comment.end_byte = end_byte;
				comment.end_row = end_row;
				return;
			}
			builder.add_comment(scope, comment.start_byte, comment.end_byte);
		}
		*pending = Some(PendingComment {
			start_byte,
			end_byte,
			end_row,
		});
	}

	fn flush_pending(
		&self,
		pending: &mut Option<PendingComment>,
		scope: &Moniker,
		builder: &mut SqlBuilder,
	) {
		if let Some(comment) = pending.take() {
			builder.add_comment(scope, comment.start_byte, comment.end_byte);
		}
	}
}

impl SqlDiscover {
	pub(super) fn run(
		module: Moniker,
		source: &str,
		document: &ParsedDocument,
	) -> DiscoveredSqlFile {
		let root = document.primary().root_node();
		let (callable_metadata, search_paths) =
			collect_callable_metadata(root, source.as_bytes(), &module);
		let mut builder = SqlBuilder::new(module.clone());
		SqlWalker {
			module: &module,
			source_str: source,
			document: Some(document),
			emit_comments: true,
			search_paths: &search_paths,
		}
		.walk(root, &module, &mut builder);
		builder.apply_callable_metadata(&callable_metadata);
		builder.finish()
	}
}

impl SqlWalker<'_> {
	fn on_symbol_emitted(
		&self,
		node: Node<'_>,
		sym_kind: &[u8],
		sym_moniker: &Moniker,
		builder: &mut SqlBuilder,
	) {
		let source = self.source_str.as_bytes();
		if matches!(sym_kind, kinds::FUNCTION | kinds::PROCEDURE) {
			emit_function_type_refs(node, source, sym_moniker, self.module, builder);
			if let Some(injection) = self
				.document
				.and_then(|document| document.injection_within(node.start_byte()..node.end_byte()))
				&& let Some(body_text) = self.source_str.get(injection.content_byte_range())
			{
				if injection.language() == "plpgsql" {
					super::super::body::walk_plpgsql_body(
						body_text,
						injection.tree(),
						sym_moniker,
						self.module,
						self.search_paths,
						builder,
					);
				} else if injection.language() == "sql" {
					run_inner_sql_tree(
						injection.tree(),
						body_text,
						sym_moniker,
						self.module,
						self.search_paths,
						builder,
					);
				}
			}
		} else if sym_kind == kinds::TABLE {
			emit_table_members(node, source, sym_moniker, self.module, builder);
		} else if sym_kind == kinds::TRIGGER {
			emit_trigger_refs(
				node,
				source,
				sym_moniker,
				self.module,
				self.search_paths,
				builder,
			);
		}
	}
}

pub(in crate::lang::sql) fn run_inner_sql(
	parser: &mut Parser,
	source: &str,
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	let tree = parse_with(parser, source);
	run_inner_sql_tree(&tree, source, scope, module, search_paths, builder);
}

fn run_inner_sql_tree(
	tree: &Tree,
	source: &str,
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	SqlWalker {
		module,
		source_str: source,
		document: None,
		emit_comments: false,
		search_paths,
	}
	.walk(tree.root_node(), scope, builder);
}

fn classify_create_function<'src>(
	node: Node<'src>,
	source: &[u8],
	module: &Moniker,
) -> SqlNodeShape<'src> {
	let Some(func_name) = find_child(node, "func_name") else {
		return SqlNodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(func_name, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() {
		return SqlNodeShape::Recurse;
	}
	let params = find_child(node, "func_args_with_defaults");
	let slots = params
		.map(|p| collect_param_slots(p, source))
		.unwrap_or_default();
	let parent = maybe_schema(module, &schema);
	let kind = routine_kind(node);
	let moniker = extend_callable_slots(&parent, kind, &name, &slots);
	let signature =
		join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
	SqlNodeShape::Symbol(SqlSymbol {
		moniker,
		kind,
		signature,
		call_name: name,
		call_arity: Some(params.map(required_input_arity).unwrap_or(0)),
		body: None,
		position: node_position(node),
	})
}

fn classify_schema<'src>(node: Node<'src>, source: &[u8], module: &Moniker) -> SqlNodeShape<'src> {
	let name_node = find_child(node, "ColId").or_else(|| {
		find_child(node, "opt_single_name").and_then(|name| find_descendant(name, "ColId"))
	});
	let Some(name_node) = name_node else {
		return SqlNodeShape::Recurse;
	};
	let name = canonical_identifier(node_slice(name_node, source));
	if name.is_empty() {
		return SqlNodeShape::Recurse;
	}
	SqlNodeShape::Symbol(SqlSymbol {
		moniker: extend_segment(module, kinds::SCHEMA, &name),
		kind: kinds::SCHEMA,
		signature: Vec::new(),
		call_name: Vec::new(),
		call_arity: None,
		body: None,
		position: node_position(node),
	})
}

fn classify_qualified_relation<'src>(
	node: Node<'src>,
	source: &[u8],
	module: &Moniker,
	kind: &'static [u8],
	body: Option<Node<'src>>,
) -> SqlNodeShape<'src> {
	let qualified_name = find_child(node, "qualified_name").or_else(|| {
		find_child(node, "create_as_target")
			.and_then(|target| find_descendant(target, "qualified_name"))
	});
	let Some(q) = qualified_name else {
		return SqlNodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(q, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() {
		return SqlNodeShape::Recurse;
	}
	let parent = maybe_schema(module, &schema);
	let moniker = extend_segment(&parent, kind, &name);
	SqlNodeShape::Symbol(SqlSymbol {
		moniker,
		kind,
		signature: Vec::new(),
		call_name: Vec::new(),
		call_arity: None,
		body,
		position: node_position(node),
	})
}

fn classify_user_type<'src>(
	node: Node<'src>,
	source: &[u8],
	module: &Moniker,
) -> SqlNodeShape<'src> {
	let Some(name_node) =
		find_child(node, "any_name").or_else(|| find_child(node, "qualified_name"))
	else {
		return SqlNodeShape::Recurse;
	};
	let (schema, name) = split_qualified_name(name_node, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() {
		return SqlNodeShape::Recurse;
	}
	let parent = maybe_schema(module, &schema);
	SqlNodeShape::Symbol(SqlSymbol {
		moniker: extend_segment(&parent, kinds::TYPE, &name),
		kind: kinds::TYPE,
		signature: Vec::new(),
		call_name: Vec::new(),
		call_arity: None,
		body: None,
		position: node_position(node),
	})
}

fn classify_trigger<'src>(node: Node<'src>, source: &[u8], module: &Moniker) -> SqlNodeShape<'src> {
	let Some(name_node) = find_child(node, "name") else {
		return SqlNodeShape::Recurse;
	};
	let name = canonical_identifier(node_slice(name_node, source));
	let Some(table_name) = find_child(node, "qualified_name") else {
		return SqlNodeShape::Recurse;
	};
	let Some(table) = relation_target_kind(
		table_name,
		source,
		module,
		module,
		&CallableSearchPaths::new(),
		trigger_relation_kind(node),
	) else {
		return SqlNodeShape::Recurse;
	};
	if name.is_empty() {
		return SqlNodeShape::Recurse;
	}
	SqlNodeShape::Symbol(SqlSymbol {
		moniker: extend_segment(&table, kinds::TRIGGER, &name),
		kind: kinds::TRIGGER,
		signature: Vec::new(),
		call_name: Vec::new(),
		call_arity: None,
		body: None,
		position: node_position(node),
	})
}

fn trigger_relation_kind(node: Node<'_>) -> &'static [u8] {
	if find_descendant(node, "kw_instead").is_some() {
		kinds::VIEW
	} else {
		kinds::TABLE
	}
}

fn collect_callable_metadata(
	root: Node<'_>,
	source: &[u8],
	module: &Moniker,
) -> (CallableMetadata, CallableSearchPaths) {
	let mut metadata = CallableMetadata::new();
	let mut search_paths = CallableSearchPaths::new();
	visit(root, &mut |n| {
		if n.kind() != "CreateFunctionStmt" {
			return;
		}
		let Some(func_name) = find_child(n, "func_name") else {
			return;
		};
		let (schema, name) = split_qualified_name(func_name, source);
		let schema = canonical_identifier(schema);
		let name = canonical_identifier(name);
		if name.is_empty() {
			return;
		}
		let params = find_child(n, "func_args_with_defaults");
		let slots = params
			.map(|p| collect_param_slots(p, source))
			.unwrap_or_default();
		let parent = maybe_schema(module, &schema);
		let kind = routine_kind(n);
		let m = extend_callable_slots(&parent, kind, &name, &slots);
		let call_arity = Some(params.map(required_input_arity).unwrap_or(0));
		metadata.insert(m.clone(), (name, call_arity));
		search_paths
			.entry(m)
			.or_insert_with(|| static_search_schema(n, source));
	});
	(metadata, search_paths)
}

fn required_input_arity(params: Node<'_>) -> usize {
	let mut required = 0;
	visit(params, &mut |node| {
		if node.kind() != "func_arg_with_default" {
			return;
		}
		let Some(argument) = find_child(node, "func_arg") else {
			return;
		};
		if find_descendant(argument, "kw_out").is_none()
			&& find_descendant(argument, "kw_variadic").is_none()
			&& !has_default_marker(node)
		{
			required += 1;
		}
	});
	required
}

fn has_default_marker(node: Node<'_>) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.any(|child| matches!(child.kind(), "kw_default" | "="))
}

fn routine_kind(node: Node<'_>) -> &'static [u8] {
	if find_descendant(node, "kw_procedure").is_some() {
		kinds::PROCEDURE
	} else {
		kinds::FUNCTION
	}
}

fn emit_call(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	let Some(name_node) = find_child(node, "func_name") else {
		return;
	};
	let (schema, name) = split_qualified_name(name_node, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() || is_non_callable_keyword(&name) {
		return;
	}
	let argument_slots = call_argument_slots(node, source, scope);
	let builtin_name = name.as_slice();
	let procedure_call = inside_call_statement(node);
	let callable_kind = if procedure_call {
		kinds::PROCEDURE
	} else {
		kinds::FUNCTION
	};
	let confidence =
		if schema == b"pg_catalog" || (schema.is_empty() && is_builtin_function(builtin_name)) {
			kinds::CONF_EXTERNAL
		} else {
			kinds::CONF_NAME_MATCH
		};
	let target = if confidence == kinds::CONF_EXTERNAL {
		let mut b = crate::lang::sdk::sdk_target_builder(module.as_view().project(), b"sql");
		b.segment(kinds::PATH, b"pg_catalog");
		b.segment(kinds::PATH, builtin_name);
		b.build()
	} else {
		let inferred_schema = schema
			.is_empty()
			.then(|| search_paths.get(scope))
			.flatten()
			.and_then(Option::as_deref)
			.unwrap_or(&schema);
		let parent = maybe_schema(module, inferred_schema);
		extend_callable_slots(&parent, callable_kind, &name, &argument_slots)
	};
	let s = node.start_byte() as u32;
	builder.push_ref(resolved_ref(
		scope,
		target,
		kinds::REF_CALLS,
		Some((s, s)),
		confidence,
		&name,
		Some(argument_slots.len()),
	));
}

fn inside_call_statement(mut node: Node<'_>) -> bool {
	while let Some(parent) = node.parent() {
		if parent.kind() == "CallStmt" {
			return true;
		}
		node = parent;
	}
	false
}

fn call_argument_slots(node: Node<'_>, source: &[u8], scope: &Moniker) -> Vec<CallableSlot> {
	let Some(arguments) = find_child(node, "func_arg_list") else {
		return Vec::new();
	};
	let mut nodes = Vec::new();
	collect_call_arguments(arguments, &mut nodes);
	nodes
		.into_iter()
		.map(|argument| {
			let raw = trim_ascii(node_slice(argument, source));
			let (name, expression) = named_argument_parts(raw);
			let inferred = infer_argument_type(expression, scope);
			CallableSlot {
				name: name.map(canonical_identifier).unwrap_or_default(),
				r#type: inferred.unwrap_or_else(|| {
					if name.is_some() {
						b"_".to_vec()
					} else {
						Vec::new()
					}
				}),
			}
		})
		.collect()
}

fn collect_call_arguments<'tree>(node: Node<'tree>, out: &mut Vec<Node<'tree>>) {
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		match child.kind() {
			"func_arg_expr" => out.push(child),
			"func_arg_list" => collect_call_arguments(child, out),
			_ => {}
		}
	}
}

fn infer_argument_type(raw: &[u8], scope: &Moniker) -> Option<Vec<u8>> {
	if let Some(cast) = explicit_cast_type(raw) {
		return Some(cast);
	}
	if raw.eq_ignore_ascii_case(b"true") || raw.eq_ignore_ascii_case(b"false") {
		return Some(b"bool".to_vec());
	}
	if let Some(number) = numeric_literal_type(raw) {
		return Some(number);
	}
	if is_identifier(raw) {
		return scope_parameter_type(scope, raw);
	}
	None
}

fn explicit_cast_type(raw: &[u8]) -> Option<Vec<u8>> {
	let cast_at = raw.windows(2).rposition(|window| window == b"::")?;
	let operand = trim_ascii(&raw[..cast_at]);
	if !is_atomic_cast_operand(operand) && explicit_cast_type(operand).is_none() {
		return None;
	}
	let candidate = trim_ascii(&raw[cast_at + 2..]);
	if candidate.is_empty()
		|| candidate.iter().any(|byte| {
			!matches!(
				byte,
				b'a'..=b'z'
					| b'A'..=b'Z'
					| b'0'..=b'9'
					| b'_'
					| b'.'
					| b'"'
					| b' '
					| b'\t'
					| b'['
					| b']'
					| b'('
					| b')'
					| b','
			)
		}) {
		return None;
	}
	Some(normalize_type(candidate))
}

fn is_atomic_cast_operand(value: &[u8]) -> bool {
	let value = trim_ascii(value);
	if value.is_empty() {
		return false;
	}
	if is_identifier(value)
		|| numeric_literal_type(value).is_some()
		|| value.eq_ignore_ascii_case(b"true")
		|| value.eq_ignore_ascii_case(b"false")
		|| value.eq_ignore_ascii_case(b"null")
		|| (value.starts_with(b"'") && value.ends_with(b"'"))
	{
		return true;
	}
	if delimiters_enclose(value, b'(', b')') || delimiters_enclose(value, b'[', b']') {
		return true;
	}
	for (open, close) in [(b'(', b')'), (b'[', b']')] {
		let Some(position) = value.iter().position(|byte| *byte == open) else {
			continue;
		};
		if is_qualified_identifier(trim_ascii(&value[..position]))
			&& delimiters_enclose(&value[position..], open, close)
		{
			return true;
		}
	}
	false
}

fn delimiters_enclose(value: &[u8], open: u8, close: u8) -> bool {
	if value.first() != Some(&open) || value.last() != Some(&close) {
		return false;
	}
	let mut depth = 0_u32;
	let mut single_quoted = false;
	let mut double_quoted = false;
	for (index, byte) in value.iter().copied().enumerate() {
		match byte {
			b'\'' if !double_quoted => single_quoted = !single_quoted,
			b'"' if !single_quoted => double_quoted = !double_quoted,
			_ if single_quoted || double_quoted => {}
			_ if byte == open => depth += 1,
			_ if byte == close => {
				depth = depth.saturating_sub(1);
				if depth == 0 && index + 1 != value.len() {
					return false;
				}
			}
			_ => {}
		}
	}
	depth == 0 && !single_quoted && !double_quoted
}

fn is_qualified_identifier(value: &[u8]) -> bool {
	!value.is_empty()
		&& value
			.iter()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'"' | b'.'))
}

fn named_argument_parts(raw: &[u8]) -> (Option<&[u8]>, &[u8]) {
	if let Some(position) = raw
		.windows(2)
		.position(|window| window == b"=>" || window == b":=")
	{
		let name = trim_ascii(&raw[..position]);
		let expression = trim_ascii(&raw[position + 2..]);
		return ((!name.is_empty()).then_some(name), expression);
	}
	(None, raw)
}

fn numeric_literal_type(raw: &[u8]) -> Option<Vec<u8>> {
	let unsigned = raw
		.strip_prefix(b"-")
		.or_else(|| raw.strip_prefix(b"+"))
		.unwrap_or(raw);
	if unsigned.is_empty() {
		return None;
	}
	if unsigned.iter().all(u8::is_ascii_digit) {
		let value = std::str::from_utf8(raw).ok()?.parse::<i64>().ok()?;
		return Some(if i32::try_from(value).is_ok() {
			b"int4".to_vec()
		} else {
			b"int8".to_vec()
		});
	}
	let mut decimal_point = false;
	if unsigned.iter().all(|byte| {
		if *byte == b'.' && !decimal_point {
			decimal_point = true;
			true
		} else {
			byte.is_ascii_digit()
		}
	}) && decimal_point
	{
		return Some(b"numeric".to_vec());
	}
	None
}

fn scope_parameter_type(scope: &Moniker, argument: &[u8]) -> Option<Vec<u8>> {
	let callable = scope.as_view().segments().last()?;
	let name = callable.name;
	let open = name.iter().position(|byte| *byte == b'(')?;
	let close = name.iter().rposition(|byte| *byte == b')')?;
	if close <= open {
		return None;
	}
	for slot in split_top_level(&name[open + 1..close], b',') {
		let Some(colon) = top_level_byte(slot, b':') else {
			continue;
		};
		let parameter = trim_ascii(&slot[..colon]);
		if canonical_identifier(parameter) == canonical_identifier(argument) {
			return Some(normalize_type(trim_ascii(&slot[colon + 1..])));
		}
	}
	None
}

fn split_top_level(value: &[u8], separator: u8) -> Vec<&[u8]> {
	let mut out = Vec::new();
	let mut start = 0;
	let mut depth = 0_u32;
	let mut quoted = false;
	for (index, byte) in value.iter().copied().enumerate() {
		match byte {
			b'"' => quoted = !quoted,
			b'(' | b'[' if !quoted => depth += 1,
			b')' | b']' if !quoted => depth = depth.saturating_sub(1),
			_ if byte == separator && !quoted && depth == 0 => {
				out.push(&value[start..index]);
				start = index + 1;
			}
			_ => {}
		}
	}
	out.push(&value[start..]);
	out
}

fn top_level_byte(value: &[u8], needle: u8) -> Option<usize> {
	let mut depth = 0_u32;
	let mut quoted = false;
	for (index, byte) in value.iter().copied().enumerate() {
		match byte {
			b'"' => quoted = !quoted,
			b'(' | b'[' if !quoted => depth += 1,
			b')' | b']' if !quoted => depth = depth.saturating_sub(1),
			_ if byte == needle && !quoted && depth == 0 => return Some(index),
			_ => {}
		}
	}
	None
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
	while value.first().is_some_and(u8::is_ascii_whitespace) {
		value = &value[1..];
	}
	while value.last().is_some_and(u8::is_ascii_whitespace) {
		value = &value[..value.len() - 1];
	}
	value
}

fn is_identifier(value: &[u8]) -> bool {
	!value.is_empty()
		&& value
			.iter()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'"'))
}

fn is_non_callable_keyword(name: &[u8]) -> bool {
	matches!(name, b"any" | b"as" | b"distinct" | b"from" | b"is")
}

fn static_search_schema(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	let header = node_slice(node, source);
	let header = header
		.windows(2)
		.position(|window| window == b"$$")
		.map(|body| &header[..body])
		.unwrap_or(header);
	let text = std::str::from_utf8(header).ok()?;
	let lower = text.to_ascii_lowercase();
	let marker = "set search_path";
	let start = lower.find(marker)? + marker.len();
	let mut clause = text.get(start..)?.trim_start();
	if let Some(rest) = clause.strip_prefix('=') {
		clause = rest.trim_start();
	} else if clause
		.get(..2)
		.is_some_and(|prefix| prefix.eq_ignore_ascii_case("to"))
	{
		clause = clause.get(2..)?.trim_start();
	} else {
		return None;
	}
	let end = clause.find(['\n', '\r', ';']).unwrap_or(clause.len());
	let clause = clause[..end]
		.split_once(" AS ")
		.map(|(path, _)| path)
		.unwrap_or(&clause[..end]);
	let schemas = clause
		.split(',')
		.map(str::trim)
		.filter(|schema| !schema.is_empty())
		.map(|schema| canonical_identifier(schema.as_bytes()))
		.collect::<Vec<_>>();
	if schemas
		.iter()
		.any(|schema| matches!(schema.as_slice(), b"pg_catalog" | b"$user"))
	{
		return None;
	}
	let static_schemas = schemas
		.into_iter()
		.filter(|schema| schema.as_slice() != b"pg_temp")
		.collect::<Vec<_>>();
	(static_schemas.len() == 1).then(|| static_schemas[0].clone())
}

fn canonical_identifier(value: &[u8]) -> Vec<u8> {
	if value.first() != Some(&b'"') || value.last() != Some(&b'"') {
		return value.iter().map(u8::to_ascii_lowercase).collect();
	}
	let mut canonical = Vec::with_capacity(value.len().saturating_sub(2));
	let mut bytes = value[1..value.len() - 1].iter().copied().peekable();
	while let Some(byte) = bytes.next() {
		canonical.push(byte);
		if byte == b'"' && bytes.peek() == Some(&b'"') {
			bytes.next();
		}
	}
	canonical
}

fn is_builtin_function(name: &[u8]) -> bool {
	is_builtin_catalog_function(name)
		|| matches!(
			name,
			b"coalesce"
				| b"nullif" | b"greatest"
				| b"least" | b"length"
				| b"char_length"
				| b"character_length"
				| b"octet_length"
				| b"lower" | b"upper"
				| b"initcap" | b"substring"
				| b"substr" | b"trim"
				| b"ltrim" | b"rtrim"
				| b"btrim" | b"replace"
				| b"translate"
				| b"position"
				| b"strpos" | b"concat"
				| b"concat_ws"
				| b"split_part"
				| b"string_agg"
				| b"array_agg"
				| b"array_length"
				| b"array_lower"
				| b"array_upper"
				| b"array_to_string"
				| b"string_to_array"
				| b"array_append"
				| b"array_prepend"
				| b"array_cat"
				| b"cardinality"
				| b"unnest" | b"generate_series"
				| b"jsonb_build_object"
				| b"json_build_object"
				| b"jsonb_build_array"
				| b"json_build_array"
				| b"jsonb_object_keys"
				| b"json_object_keys"
				| b"jsonb_extract_path"
				| b"jsonb_extract_path_text"
				| b"json_extract_path"
				| b"json_extract_path_text"
				| b"jsonb_to_recordset"
				| b"jsonb_populate_record"
				| b"jsonb_each"
				| b"jsonb_each_text"
				| b"jsonb_typeof"
				| b"jsonb_pretty"
				| b"json_array_length"
				| b"jsonb_array_length"
				| b"to_json" | b"to_jsonb"
				| b"row_to_json"
				| b"to_char" | b"date_part"
				| b"age" | b"regexp_match"
				| b"enum_range"
				| b"num_nonnulls"
				| b"abs" | b"floor"
				| b"ceil" | b"ceiling"
				| b"round" | b"trunc"
				| b"mod" | b"power"
				| b"sqrt" | b"random"
				| b"count" | b"sum"
				| b"avg" | b"bool_and"
				| b"json_agg"
				| b"jsonb_agg"
				| b"min" | b"max"
				| b"row_number"
				| b"rank" | b"dense_rank"
				| b"percent_rank"
				| b"cume_dist"
				| b"ntile" | b"lag"
				| b"lead" | b"first_value"
				| b"last_value"
				| b"nth_value"
				| b"gen_random_uuid"
				| b"nextval" | b"currval"
				| b"setval" | b"pg_typeof"
				| b"pg_size_pretty"
				| b"pg_tablespace_location"
				| b"txid_current"
				| b"quote_ident"
				| b"quote_literal"
				| b"quote_nullable"
				| b"to_tsquery"
				| b"plainto_tsquery"
				| b"phraseto_tsquery"
				| b"websearch_to_tsquery"
				| b"to_tsvector"
				| b"ts_rank" | b"ts_rank_cd"
				| b"inet_client_addr"
				| b"inet_client_port"
				| b"inet_server_addr"
				| b"inet_server_port"
		)
}

fn is_builtin_catalog_function(name: &[u8]) -> bool {
	matches!(
		name,
		b"format"
			| b"chr" | b"format_type"
			| b"to_regtype"
			| b"to_regtypemod"
			| b"to_regclass"
			| b"to_regproc"
			| b"current_setting"
			| b"set_config"
			| b"current_database"
			| b"current_schema"
			| b"current_user"
			| b"session_user"
			| b"version"
			| b"now" | b"clock_timestamp"
			| b"transaction_timestamp"
			| b"statement_timestamp"
			| b"timeofday"
	)
}

pub(super) fn visit<F: FnMut(Node)>(node: Node, f: &mut F) {
	f(node);
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		visit(c, f);
	}
}

pub(super) fn split_qualified_name<'src>(
	node: Node<'src>,
	src: &'src [u8],
) -> (&'src [u8], &'src [u8]) {
	let mut parts: Vec<&'src [u8]> = Vec::new();
	collect_qualified_parts(node, src, &mut parts);
	match parts.len() {
		0 => (&[], &[]),
		1 => (&[], parts[0]),
		_ => (parts[0], parts[parts.len() - 1]),
	}
}

fn collect_qualified_parts<'src>(node: Node<'src>, src: &'src [u8], out: &mut Vec<&'src [u8]>) {
	let mut cur = node.walk();
	for c in node.named_children(&mut cur) {
		match c.kind() {
			"ColId" | "ColLabel" | "type_function_name" | "attr_name" => {
				if let Some(id) = find_descendant(c, "identifier")
					.or_else(|| find_descendant(c, "quoted_identifier"))
				{
					out.push(node_slice(id, src));
				} else {
					out.push(node_slice(c, src));
				}
			}
			"indirection" | "indirection_el" => collect_qualified_parts(c, src, out),
			"identifier" | "quoted_identifier" => out.push(node_slice(c, src)),
			_ => collect_qualified_parts(c, src, out),
		}
	}
}

fn collect_param_slots(params: Node, src: &[u8]) -> Vec<CallableSlot> {
	let mut out = Vec::new();
	visit(params, &mut |n| {
		if n.kind() != "func_arg" {
			return;
		}
		if find_descendant(n, "kw_out").is_some() {
			return;
		}
		let mut r#type = find_child(n, "func_type")
			.map(|ft| normalize_type(node_slice(ft, src)))
			.unwrap_or_default();
		if find_descendant(n, "kw_variadic").is_some() {
			r#type.extend_from_slice(b"...");
		}
		let name = find_child(n, "param_name")
			.map(|pn| canonical_identifier(node_slice(pn, src)))
			.unwrap_or_default();
		out.push(CallableSlot { name, r#type });
	});
	out
}

fn normalize_type(raw: &[u8]) -> Vec<u8> {
	let s = std::str::from_utf8(raw).unwrap_or("");
	let mut collapsed = String::new();
	for w in s.split_whitespace() {
		if !collapsed.is_empty() {
			collapsed.push(' ');
		}
		collapsed.push_str(w);
	}
	let mut canonical = Vec::with_capacity(collapsed.len());
	let mut quoted = false;
	for mut byte in collapsed.bytes() {
		if byte == b'"' {
			quoted = !quoted;
		} else if !quoted {
			byte.make_ascii_lowercase();
		}
		canonical.push(byte);
	}
	match canonical.as_slice() {
		b"int" | b"integer" => b"int4".to_vec(),
		b"bigint" => b"int8".to_vec(),
		b"smallint" => b"int2".to_vec(),
		b"real" => b"float4".to_vec(),
		b"double precision" => b"float8".to_vec(),
		_ => canonical,
	}
}

fn emit_function_type_refs(
	node: Node<'_>,
	source: &[u8],
	source_moniker: &Moniker,
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	if let Some(params) = find_child(node, "func_args_with_defaults") {
		visit(params, &mut |n| {
			if n.kind() != "func_arg" {
				return;
			}
			if let Some(ft) = find_child(n, "func_type") {
				emit_uses_type(ft, source, source_moniker, module, builder);
			}
		});
	}
	if let Some(ft) = find_descendant(node, "func_return")
		&& let Some(t) = find_descendant(ft, "func_type")
	{
		emit_uses_type(t, source, source_moniker, module, builder);
	}
}

fn emit_table_members(
	node: Node<'_>,
	source: &[u8],
	table_moniker: &Moniker,
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	let mut columns = Vec::new();
	let mut table_constraints = Vec::new();
	collect_nodes(node, "columnDef", &mut columns);
	collect_nodes(node, "TableConstraint", &mut table_constraints);
	for column in columns {
		let Some(name_node) = find_child(column, "ColId") else {
			continue;
		};
		let name = canonical_identifier(node_slice(name_node, source));
		if name.is_empty() {
			continue;
		}
		let column_moniker = extend_segment(table_moniker, kinds::COLUMN, &name);
		let type_node = find_child(column, "Typename");
		let signature = type_node
			.map(|r#type| normalize_type(node_slice(r#type, source)))
			.unwrap_or_default();
		if !builder.add_definition(
			column_moniker.clone(),
			kinds::COLUMN,
			signature,
			node_position(column),
			table_moniker,
		) {
			continue;
		}
		if let Some(r#type) = type_node {
			emit_uses_type(r#type, source, &column_moniker, module, builder);
		}
		let mut column_constraints = Vec::new();
		collect_nodes(column, "ColConstraint", &mut column_constraints);
		for constraint in column_constraints {
			if find_descendant(constraint, "ColConstraintElem").is_some() {
				emit_constraint(constraint, source, table_moniker, module, builder);
			}
		}
	}
	for constraint in table_constraints {
		emit_constraint(constraint, source, table_moniker, module, builder);
	}
}

fn emit_statement_write(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	let container_kind = match node.kind() {
		"InsertStmt" => "insert_target",
		"UpdateStmt" | "DeleteStmt" => "relation_expr_opt_alias",
		"CreateAsStmt" => "create_as_target",
		_ => return,
	};
	let Some(container) = find_child(node, container_kind) else {
		return;
	};
	let Some(name) = find_descendant(container, "qualified_name") else {
		return;
	};
	let Some(target) = relation_target(name, source, scope, module, search_paths) else {
		return;
	};
	builder.push_ref(resolved_ref(
		scope,
		target,
		kinds::REF_WRITES,
		Some(node_position(name)),
		kinds::CONF_NAME_MATCH,
		&[],
		None,
	));
}

fn emit_statement_reads(
	statement: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	let cte_names = visible_cte_names(statement, source);
	let mut relations = Vec::new();
	collect_nodes(statement, "relation_expr", &mut relations);
	for relation in relations {
		if !nearest_statement(relation).is_some_and(|owner| owner == statement)
			|| !is_read_relation(relation, statement)
		{
			continue;
		}
		let Some(name_node) = find_descendant(relation, "qualified_name") else {
			continue;
		};
		let (schema, name) = split_qualified_name(name_node, source);
		let schema = canonical_identifier(schema);
		let name = canonical_identifier(name);
		if name.is_empty() || (schema.is_empty() && cte_names.contains(&name)) {
			continue;
		}
		let Some(target) = relation_target(name_node, source, scope, module, search_paths) else {
			continue;
		};
		builder.push_ref(resolved_ref(
			scope,
			target,
			kinds::REF_READS,
			Some(node_position(name_node)),
			kinds::CONF_NAME_MATCH,
			&[],
			None,
		));
	}
}

fn relation_target(
	name_node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
) -> Option<Moniker> {
	relation_target_kind(name_node, source, scope, module, search_paths, kinds::TABLE)
}

fn relation_target_kind(
	name_node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	kind: &[u8],
) -> Option<Moniker> {
	let (schema, name) = split_qualified_name(name_node, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() {
		return None;
	}
	let inferred_schema = if schema.is_empty() {
		search_paths
			.get(scope)
			.and_then(Option::as_deref)
			.unwrap_or_default()
	} else {
		&schema
	};
	Some(extend_segment(
		&maybe_schema(module, inferred_schema),
		kind,
		&name,
	))
}

fn nearest_statement(mut node: Node<'_>) -> Option<Node<'_>> {
	while let Some(parent) = node.parent() {
		if is_relational_statement(parent.kind()) {
			return Some(parent);
		}
		node = parent;
	}
	None
}

fn is_relational_statement(kind: &str) -> bool {
	matches!(
		kind,
		"SelectStmt" | "InsertStmt" | "UpdateStmt" | "DeleteStmt" | "CreateAsStmt"
	)
}

fn is_read_relation(mut node: Node<'_>, statement: Node<'_>) -> bool {
	while let Some(parent) = node.parent() {
		if parent == statement {
			return parent.kind() == "SelectStmt" && find_descendant(parent, "kw_table").is_some();
		}
		if parent.kind() == "table_ref" {
			return true;
		}
		if is_relational_statement(parent.kind()) {
			return false;
		}
		node = parent;
	}
	false
}

fn visible_cte_names(statement: Node<'_>, source: &[u8]) -> HashSet<Vec<u8>> {
	let mut names = HashSet::new();
	let mut current = Some(statement);
	while let Some(owner) = current {
		visit(owner, &mut |node| {
			if node.kind() != "common_table_expr"
				|| !nearest_statement(node).is_some_and(|statement| statement == owner)
			{
				return;
			}
			if let Some(name) = find_child(node, "name")
				.and_then(|name| find_descendant(name, "ColId"))
				.map(|name| canonical_identifier(node_slice(name, source)))
				.filter(|name| !name.is_empty())
			{
				names.insert(name);
			}
		});
		current = nearest_statement(owner);
	}
	names
}

fn emit_constraint(
	node: Node<'_>,
	source: &[u8],
	table_moniker: &Moniker,
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	let explicit_name = find_child(node, "name")
		.map(|name| canonical_identifier(node_slice(name, source)))
		.filter(|name| !name.is_empty());
	let moniker = explicit_name
		.as_deref()
		.map(|name| extend_segment(table_moniker, kinds::CONSTRAINT, name))
		.unwrap_or_else(|| {
			extend_segment_u32(table_moniker, kinds::CONSTRAINT, node.start_byte() as u32)
		});
	if !builder.add_definition(
		moniker.clone(),
		kinds::CONSTRAINT,
		constraint_signature(node),
		node_position(node),
		table_moniker,
	) {
		return;
	}
	emit_foreign_key_refs(node, source, &moniker, module, builder);
}

fn constraint_signature(node: Node<'_>) -> Vec<u8> {
	for (keyword, signature) in [
		("kw_foreign", b"foreign key".as_slice()),
		("kw_primary", b"primary key"),
		("kw_unique", b"unique"),
		("kw_check", b"check"),
		("kw_references", b"foreign key"),
		("kw_not", b"not null"),
		("kw_null", b"null"),
		("kw_default", b"default"),
		("kw_generated", b"generated"),
	] {
		if find_descendant(node, keyword).is_some() {
			return signature.to_vec();
		}
	}
	Vec::new()
}

fn emit_foreign_key_refs(
	node: Node<'_>,
	source: &[u8],
	constraint_moniker: &Moniker,
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	if find_descendant(node, "kw_references").is_none() {
		return;
	}
	let Some(target_name) = find_descendant(node, "qualified_name") else {
		return;
	};
	let Some(target_table) = relation_target(
		target_name,
		source,
		constraint_moniker,
		module,
		&CallableSearchPaths::new(),
	) else {
		return;
	};
	builder.push_ref(resolved_ref(
		constraint_moniker,
		target_table.clone(),
		kinds::REF_REFERENCES,
		Some(node_position(target_name)),
		kinds::CONF_NAME_MATCH,
		&[],
		None,
	));
	let mut target_columns = Vec::new();
	collect_nodes(node, "columnElem", &mut target_columns);
	for column in target_columns {
		if column.start_byte() < target_name.end_byte() {
			continue;
		}
		let Some(name_node) = find_descendant(column, "ColId") else {
			continue;
		};
		let name = canonical_identifier(node_slice(name_node, source));
		if name.is_empty() {
			continue;
		}
		builder.push_ref(resolved_ref(
			constraint_moniker,
			extend_segment(&target_table, kinds::COLUMN, &name),
			kinds::REF_REFERENCES,
			Some(node_position(name_node)),
			kinds::CONF_NAME_MATCH,
			&[],
			None,
		));
	}
}

fn emit_trigger_refs(
	node: Node<'_>,
	source: &[u8],
	trigger_moniker: &Moniker,
	module: &Moniker,
	search_paths: &CallableSearchPaths,
	builder: &mut SqlBuilder,
) {
	let relation_kind = trigger_relation_kind(node);
	if let Some(table_name) = find_child(node, "qualified_name")
		&& let Some(table) = relation_target_kind(
			table_name,
			source,
			trigger_moniker,
			module,
			search_paths,
			relation_kind,
		) {
		builder.push_ref(resolved_ref(
			trigger_moniker,
			table,
			kinds::REF_REFERENCES,
			Some(node_position(table_name)),
			kinds::CONF_NAME_MATCH,
			&[],
			None,
		));
	}
	if let Some(from_table) = find_child(node, "OptConstrFromTable")
		.and_then(|from| find_descendant(from, "qualified_name"))
		&& let Some(table) =
			relation_target(from_table, source, trigger_moniker, module, search_paths)
	{
		builder.push_ref(resolved_ref(
			trigger_moniker,
			table,
			kinds::REF_REFERENCES,
			Some(node_position(from_table)),
			kinds::CONF_NAME_MATCH,
			&[],
			None,
		));
	}
	let Some(function_name) = find_child(node, "func_name") else {
		return;
	};
	let (schema, name) = split_qualified_name(function_name, source);
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	if name.is_empty() {
		return;
	}
	let inferred_schema = schema
		.is_empty()
		.then(|| search_paths.get(trigger_moniker))
		.flatten()
		.and_then(Option::as_deref)
		.unwrap_or(&schema);
	let parent = maybe_schema(module, inferred_schema);
	let target = extend_callable_slots(&parent, kinds::FUNCTION, &name, &[]);
	builder.push_ref(resolved_ref(
		trigger_moniker,
		target,
		kinds::REF_CALLS,
		Some(node_position(function_name)),
		kinds::CONF_NAME_MATCH,
		&name,
		Some(0),
	));
}

fn collect_nodes<'tree>(node: Node<'tree>, kind: &str, out: &mut Vec<Node<'tree>>) {
	if node.kind() == kind {
		out.push(node);
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		collect_nodes(child, kind, out);
	}
}

fn emit_uses_type(
	type_node: Node<'_>,
	source: &[u8],
	source_moniker: &Moniker,
	module: &Moniker,
	builder: &mut SqlBuilder,
) {
	let raw = node_slice(type_node, source);
	let canonical = normalize_type(raw);
	if canonical.is_empty() {
		return;
	}
	let (target, confidence) = type_target(&canonical, module);
	builder.push_ref(resolved_ref(
		source_moniker,
		target,
		kinds::USES_TYPE,
		Some(node_position(type_node)),
		confidence,
		&[],
		None,
	));
}

fn type_target(canonical: &[u8], module: &Moniker) -> (Moniker, &'static [u8]) {
	let base = canonical_type_base(canonical);
	if is_builtin_type(base) {
		let mut b = crate::lang::sdk::sdk_target_builder(module.as_view().project(), b"sql");
		b.segment(kinds::PATH, b"pg_catalog");
		b.segment(kinds::PATH, canonical);
		return (b.build(), kinds::CONF_EXTERNAL);
	}
	let (schema, name) = base
		.iter()
		.rposition(|byte| *byte == b'.')
		.map(|dot| (&base[..dot], &base[dot + 1..]))
		.unwrap_or((&[][..], base));
	let schema = canonical_identifier(schema);
	let name = canonical_identifier(name);
	let target = extend_segment(&maybe_schema(module, &schema), kinds::TYPE, &name);
	(target, kinds::CONF_NAME_MATCH)
}

fn canonical_type_base(name: &[u8]) -> &[u8] {
	let name = name.strip_prefix(b"setof ").unwrap_or(name);
	let end = name
		.iter()
		.position(|byte| matches!(byte, b'(' | b'['))
		.unwrap_or(name.len());
	&name[..end]
}

fn is_builtin_type(name: &[u8]) -> bool {
	matches!(
		name,
		b"int"
			| b"integer"
			| b"int2" | b"int4"
			| b"int8" | b"bigint"
			| b"smallint"
			| b"float4"
			| b"float8"
			| b"numeric"
			| b"decimal"
			| b"money"
			| b"text" | b"varchar"
			| b"bpchar"
			| b"char" | b"\"char\""
			| b"bool" | b"boolean"
			| b"date" | b"time"
			| b"timestamp"
			| b"timestamp with time zone"
			| b"timestamp without time zone"
			| b"time with time zone"
			| b"time without time zone"
			| b"timestamptz"
			| b"interval"
			| b"uuid" | b"json"
			| b"jsonb"
			| b"bytea"
			| b"serial"
			| b"bigserial"
			| b"smallserial"
			| b"oid" | b"regclass"
			| b"refcursor"
			| b"regproc"
			| b"regprocedure"
			| b"regtype"
			| b"cstring"
			| b"xml" | b"inet"
			| b"cidr" | b"macaddr"
			| b"macaddr8"
			| b"bit" | b"varbit"
			| b"tsvector"
			| b"tsquery"
			| b"name" | b"void"
			| b"trigger"
			| b"record"
			| b"any" | b"anyelement"
			| b"anyarray"
			| b"anynonarray"
			| b"anyenum"
			| b"anyrange"
	)
}
