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

#[derive(Default)]
struct ImportBindings {
	by_name: RefCell<HashMap<Vec<u8>, ImportBinding>>,
}

#[derive(Clone)]
struct ImportBinding {
	confidence: &'static [u8],
	target: Moniker,
}

impl ImportBindings {
	fn confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.by_name
			.borrow()
			.get(name)
			.map(|binding| binding.confidence)
	}

	fn record_binding(&self, name: &[u8], confidence: &'static [u8], target: Moniker) {
		if name.is_empty() {
			return;
		}
		self.by_name
			.borrow_mut()
			.insert(name.to_vec(), ImportBinding { confidence, target });
	}

	fn target(&self, name: &[u8]) -> Option<Moniker> {
		self.by_name
			.borrow()
			.get(name)
			.map(|binding| binding.target.clone())
	}

	fn module_for(&self, name: &[u8]) -> Option<Moniker> {
		let target = self.target(name)?;
		let view = target.as_view();
		let last = view.segments().last()?;
		if last.kind == crate::lang::kinds::PATH {
			let count = view.segment_count() as usize;
			let mut b = MonikerBuilder::from_view(view);
			b.truncate(count - 1);
			Some(b.build())
		} else {
			Some(target)
		}
	}
}

#[derive(Default)]
struct TsImports {
	bindings: ImportBindings,
}

struct TsImportContext<'a, 'src> {
	module: &'a Moniker,
	anchor: &'a Moniker,
	source: &'src [u8],
	path_aliases: &'a [super::super::PathAlias],
}

struct ImportTargetResolver<'ctx, 'src> {
	ctx: &'ctx TsImportContext<'ctx, 'src>,
}

struct TsImportClauseContext<'a, 'ctx, 'src> {
	raw_spec: &'src str,
	confidence: &'static [u8],
	pos: Position,
	scope: &'a Moniker,
	import: &'a TsImportContext<'ctx, 'src>,
	resolver: &'a ImportTargetResolver<'ctx, 'src>,
}

impl<'ctx, 'src> ImportTargetResolver<'ctx, 'src> {
	fn new(ctx: &'ctx TsImportContext<'ctx, 'src>) -> Self {
		Self { ctx }
	}

	fn module_target(&self, raw_path: &str) -> Moniker {
		self.target(raw_path, None)
	}

	fn symbol_target(&self, raw_path: &str, name: &[u8]) -> Moniker {
		self.target(raw_path, Some(name))
	}

	fn target(&self, raw_path: &str, symbol: Option<&[u8]>) -> Moniker {
		let mut builder = if let Some(resolved) = self.resolve_path_alias(raw_path) {
			self.project_rooted_module_builder(&resolved)
		} else if is_relative_specifier(raw_path) {
			self.relative_module_builder(raw_path)
		} else {
			external_pkg_builder(self.ctx.module.as_view().project(), raw_path)
		};
		if let Some(symbol) = symbol {
			builder.segment(kinds::PATH, symbol);
		}
		builder.build()
	}

	fn resolve_path_alias(&self, spec: &str) -> Option<String> {
		for alias in self.ctx.path_aliases {
			if let Some(captured) = match_path_alias(&alias.pattern, spec) {
				return Some(apply_path_alias(&alias.substitution, captured));
			}
		}
		None
	}

	fn project_rooted_module_builder(&self, path: &str) -> MonikerBuilder {
		super::canonicalize::module_builder_for_path(self.ctx.anchor, path)
	}

	fn relative_module_builder(&self, raw_path: &str) -> MonikerBuilder {
		let importer_view = self.ctx.module.as_view();
		let mut builder = MonikerBuilder::from_view(importer_view);
		let mut depth = (importer_view.segment_count() as usize).saturating_sub(1);
		builder.truncate(depth);

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
			builder.truncate(depth);
			remainder = rest;
		}
		let remainder = strip_known_extension(remainder);
		append_module_segments(&mut builder, remainder);
		builder
	}
}

#[derive(Default)]
struct TsScopes {
	local: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
	nested_funcs: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
}

impl TsScopes {
	fn push(&self) {
		self.local.borrow_mut().push(HashMap::new());
		self.nested_funcs.borrow_mut().push(HashMap::new());
	}

	fn pop(&self) {
		self.local.borrow_mut().pop();
		self.nested_funcs.borrow_mut().pop();
	}

	fn bind_local(&self, name: &[u8], def: Moniker) {
		if let Some(top) = self.local.borrow_mut().last_mut() {
			top.insert(name.to_vec(), def);
		}
	}

	fn lookup_local(&self, name: &[u8]) -> Option<Moniker> {
		self.local
			.borrow()
			.iter()
			.rev()
			.find_map(|frame| frame.get(name).cloned())
	}

	fn lookup_nested_func(&self, name: &[u8]) -> Option<Moniker> {
		for frame in self.nested_funcs.borrow().iter().rev() {
			if let Some(target) = frame.get(name) {
				return Some(target.clone());
			}
		}
		None
	}

	fn hoist_nested_funcs(&self, body: Node<'_>, parent: &Moniker, source: &[u8]) {
		let mut cursor = body.walk();
		let mut nested_funcs = self.nested_funcs.borrow_mut();
		let Some(top) = nested_funcs.last_mut() else {
			return;
		};
		for child in body.named_children(&mut cursor) {
			if let Some((name, slots)) = function_decl_info(child, source) {
				let moniker = extend_callable_slots(parent, kinds::FUNCTION, name, &slots);
				top.insert(name.to_vec(), moniker);
			}
		}
	}
}

struct TsReferenceResolver<'a> {
	module: &'a Moniker,
	imports: &'a TsImports,
	type_table: &'a HashMap<Vec<u8>, Moniker>,
	callable_table: &'a HashMap<(Moniker, Vec<u8>), CallableEntry>,
	scopes: &'a TsScopes,
	deep: bool,
}

impl TsReferenceResolver<'_> {
	fn name_confidence(&self, name: &[u8]) -> Option<&'static [u8]> {
		if self.is_local_name(name) {
			return if self.deep {
				Some(kinds::CONF_LOCAL)
			} else {
				None
			};
		}
		Some(
			self.imports
				.confidence_for(name)
				.unwrap_or(kinds::CONF_NAME_MATCH),
		)
	}

	fn ref_confidence(&self, name: &[u8]) -> &'static [u8] {
		self.imports
			.confidence_for(name)
			.unwrap_or(kinds::CONF_NAME_MATCH)
	}

	fn call_identifier_target(
		&self,
		name: &[u8],
		scope: &Moniker,
		confidence: &'static [u8],
	) -> (Moniker, &'static [u8]) {
		if confidence == kinds::CONF_LOCAL {
			let target = self
				.lookup_local_binding(name)
				.unwrap_or_else(|| extend_segment(scope, kinds::LOCAL, name));
			(target, confidence)
		} else if confidence == kinds::CONF_NAME_MATCH {
			if let Some(target) = self.lookup_nested_func(name) {
				(target, kinds::CONF_RESOLVED)
			} else if let Some(target) = self.lookup_callable(name) {
				(target, kinds::CONF_RESOLVED)
			} else if is_global_value(name) {
				(
					external_runtime_target(self.module, kinds::FUNCTION, name),
					kinds::CONF_EXTERNAL,
				)
			} else {
				(
					extend_segment(self.module, kinds::FUNCTION, name),
					confidence,
				)
			}
		} else {
			let base = self
				.imports
				.module_for(name)
				.unwrap_or_else(|| self.module.clone());
			(extend_segment(&base, kinds::FUNCTION, name), confidence)
		}
	}

	fn type_query_value_target(&self, name: &[u8], graph: &SdkBuilder) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.lookup_local_binding(name) {
			return (target, kinds::CONF_LOCAL);
		}
		if let Some(target) = self.imports.target(name) {
			return (target, self.ref_confidence(name));
		}
		if is_global_value(name) {
			return (
				external_runtime_target(self.module, kinds::FUNCTION, name),
				kinds::CONF_EXTERNAL,
			);
		}
		if let Some(target) = self.lookup_callable(name) {
			return (target, kinds::CONF_RESOLVED);
		}
		let target = extend_segment(self.module, kinds::CONST, name);
		let confidence = if graph.contains(&target) {
			kinds::CONF_RESOLVED
		} else {
			self.ref_confidence(name)
		};
		(target, confidence)
	}

	fn read_target(
		&self,
		name: &[u8],
		scope: &Moniker,
		confidence: &'static [u8],
		graph: &SdkBuilder,
	) -> (Moniker, &'static [u8]) {
		if confidence == kinds::CONF_LOCAL {
			(
				self.lookup_local_binding(name)
					.unwrap_or_else(|| extend_segment(scope, kinds::LOCAL, name)),
				confidence,
			)
		} else if let Some(target) = self.imports.target(name) {
			(target, confidence)
		} else if is_global_value(name) {
			(
				external_runtime_target(self.module, kinds::FUNCTION, name),
				kinds::CONF_EXTERNAL,
			)
		} else if let Some(target) = self.lookup_callable(name) {
			(target, kinds::CONF_RESOLVED)
		} else if graph.contains(&extend_segment(self.module, kinds::CONST, name)) {
			(
				extend_segment(self.module, kinds::CONST, name),
				kinds::CONF_RESOLVED,
			)
		} else {
			(
				extend_segment(self.module, kinds::FUNCTION, name),
				confidence,
			)
		}
	}

	fn heritage_target(&self, name: &[u8], target_kind: &'static [u8]) -> (Moniker, &'static [u8]) {
		if let Some(target) = self.imports.target(name) {
			(target, self.ref_confidence(name))
		} else if let Some(target) = self.type_table.get(name) {
			(target.clone(), self.ref_confidence(name))
		} else {
			(
				external_runtime_target(self.module, target_kind, name),
				kinds::CONF_EXTERNAL,
			)
		}
	}

	fn lookup_local_binding(&self, name: &[u8]) -> Option<Moniker> {
		self.scopes.lookup_local(name)
	}

	fn is_local_name(&self, name: &[u8]) -> bool {
		self.lookup_local_binding(name).is_some()
	}

	fn lookup_callable(&self, name: &[u8]) -> Option<Moniker> {
		let entry = self
			.callable_table
			.get(&(self.module.clone(), name.to_vec()))?;
		Some(extend_segment(self.module, entry.kind, &entry.segment))
	}

	fn lookup_nested_func(&self, name: &[u8]) -> Option<Moniker> {
		self.scopes.lookup_nested_func(name)
	}
}

impl TsImports {
	fn confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.bindings.confidence_for(name)
	}

	fn target(&self, name: &[u8]) -> Option<Moniker> {
		self.bindings.target(name)
	}

	fn module_for(&self, name: &[u8]) -> Option<Moniker> {
		self.bindings.module_for(name)
	}

	fn handle_import(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		ctx: TsImportContext<'_, '_>,
	) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, ctx.source);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);
		let confidence = import_confidence(raw_spec);
		let resolver = ImportTargetResolver::new(&ctx);
		let Some(clause) = find_named_child(node, "import_clause") else {
			let target = resolver.module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::IMPORTS_MODULE, Some(pos), &attrs);
			return;
		};

		let mut cursor = clause.walk();
		let clause_ctx = TsImportClauseContext {
			raw_spec,
			confidence,
			pos,
			scope,
			import: &ctx,
			resolver: &resolver,
		};
		for child in clause.children(&mut cursor) {
			self.handle_import_clause_child(child, graph, &clause_ctx);
		}
	}

	fn handle_import_clause_child(
		&self,
		child: Node<'_>,
		graph: &mut SdkBuilder,
		ctx: &TsImportClauseContext<'_, '_, '_>,
	) {
		match child.kind() {
			"identifier" => {
				let local_name = node_slice(child, ctx.import.source);
				let target = ctx.resolver.symbol_target(ctx.raw_spec, b"default");
				self.bindings
					.record_binding(local_name, ctx.confidence, target.clone());
				let attrs = RefAttrs {
					alias: local_name,
					confidence: ctx.confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					ctx.scope,
					target,
					kinds::IMPORTS_SYMBOL,
					Some(ctx.pos),
					&attrs,
				);
			}
			"namespace_import" => {
				let alias = first_identifier_text(child, ctx.import.source);
				let target = ctx.resolver.module_target(ctx.raw_spec);
				self.bindings
					.record_binding(alias, ctx.confidence, target.clone());
				let attrs = RefAttrs {
					alias,
					confidence: ctx.confidence,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(
					ctx.scope,
					target,
					kinds::IMPORTS_MODULE,
					Some(ctx.pos),
					&attrs,
				);
			}
			"named_imports" => {
				self.handle_named_imports(child, graph, ctx);
			}
			_ => {}
		}
	}

	fn handle_named_imports(
		&self,
		node: Node<'_>,
		graph: &mut SdkBuilder,
		ctx: &TsImportClauseContext<'_, '_, '_>,
	) {
		let mut cursor = node.walk();
		for spec in node.children(&mut cursor) {
			if spec.kind() != "import_specifier" {
				continue;
			}
			let Some(name) = spec
				.child_by_field_name("name")
				.map(|n| node_slice(n, ctx.import.source))
				.filter(|n| !n.is_empty())
			else {
				continue;
			};
			let alias = spec
				.child_by_field_name("alias")
				.map(|n| node_slice(n, ctx.import.source))
				.unwrap_or(b"");
			let local = if alias.is_empty() { name } else { alias };
			let target = ctx.resolver.symbol_target(ctx.raw_spec, name);
			self.bindings
				.record_binding(local, ctx.confidence, target.clone());
			let attrs = RefAttrs {
				alias,
				confidence: ctx.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				ctx.scope,
				target,
				kinds::IMPORTS_SYMBOL,
				Some(ctx.pos),
				&attrs,
			);
		}
	}

	fn handle_reexport(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		ctx: TsImportContext<'_, '_>,
	) {
		let Some(src_node) = node.child_by_field_name("source") else {
			return;
		};
		let raw_spec = unquote_string_literal(src_node, ctx.source);
		if raw_spec.is_empty() {
			return;
		}
		let pos = node_position(node);
		let resolver = ImportTargetResolver::new(&ctx);
		let mut has_star = false;
		let mut export_clause: Option<Node<'_>> = None;
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			match child.kind() {
				"*" => has_star = true,
				"export_clause" => export_clause = Some(child),
				_ => {}
			}
		}

		let confidence = import_confidence(raw_spec);
		if has_star {
			let target = resolver.module_target(raw_spec);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
			return;
		}

		let Some(clause) = export_clause else { return };
		let mut cursor = clause.walk();
		for spec in clause.children(&mut cursor) {
			if spec.kind() != "export_specifier" {
				continue;
			}
			let Some(name) = spec
				.child_by_field_name("name")
				.map(|n| node_slice(n, ctx.source))
				.filter(|n| !n.is_empty())
			else {
				continue;
			};
			let alias = spec
				.child_by_field_name("alias")
				.map(|n| node_slice(n, ctx.source))
				.unwrap_or(b"");
			let target = resolver.symbol_target(raw_spec, name);
			let attrs = RefAttrs {
				alias,
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
		}
	}

	fn handle_bare_reexport(
		&self,
		node: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		ctx: TsImportContext<'_, '_>,
	) {
		let pos = node_position(node);
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() != "export_clause" {
				continue;
			}
			let mut export_cursor = child.walk();
			for spec in child.children(&mut export_cursor) {
				if spec.kind() != "export_specifier" {
					continue;
				}
				let Some(local) = spec
					.child_by_field_name("name")
					.map(|n| node_slice(n, ctx.source))
					.filter(|n| !n.is_empty())
				else {
					continue;
				};
				let Some(target) = self.target(local) else {
					continue;
				};
				let alias = spec
					.child_by_field_name("alias")
					.map(|n| node_slice(n, ctx.source))
					.unwrap_or(b"");
				let attrs = RefAttrs {
					alias,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(pos), &attrs);
			}
		}
	}
}

struct TsTypeRefs<'a, 'src> {
	source: &'src [u8],
	module: &'a Moniker,
	imports: &'a TsImports,
	type_table: &'a HashMap<Vec<u8>, Moniker>,
	refs: TsReferenceResolver<'a>,
}

impl TsTypeRefs<'_, '_> {
	fn emit_recursive(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		match node.kind() {
			"type_query" => self.emit_type_query_recursive(node, scope, graph),
			"type_identifier" => self.emit_type_identifier(node, scope, graph),
			"nested_type_identifier" => self.emit_nested_type_identifier(node, scope, graph),
			"generic_type" => self.emit_generic_type(node, scope, graph),
			_ => {
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					self.emit_recursive(child, scope, graph);
				}
			}
		}
	}

	fn emit_type_identifier(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let name = node_slice(node, self.source);
		if name.is_empty() {
			return;
		}
		let target = self
			.refs
			.lookup_local_binding(name)
			.or_else(|| self.imports.target(name))
			.or_else(|| self.type_table.get(name).cloned())
			.unwrap_or_else(|| external_runtime_target(self.module, kinds::CLASS, name));
		let attrs = RefAttrs {
			confidence: self.type_confidence(name),
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

	fn emit_nested_type_identifier(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let Some(name) = nested_type_short(node, self.source) else {
			return;
		};
		let root = nested_type_root(node, self.source).unwrap_or(name);
		let target = self
			.imports
			.target(root)
			.map(|module| extend_segment(&module, kinds::CLASS, name))
			.unwrap_or_else(|| external_runtime_target(self.module, kinds::CLASS, name));
		let attrs = RefAttrs {
			confidence: self.type_confidence(root),
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

	fn emit_generic_type(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		if let Some(name) = generic_short(node, self.source) {
			let target = self
				.imports
				.target(name)
				.or_else(|| self.type_table.get(name).cloned())
				.unwrap_or_else(|| external_runtime_target(self.module, kinds::CLASS, name));
			let attrs = RefAttrs {
				confidence: self.type_confidence(name),
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
			self.emit_recursive(args, scope, graph);
		}
	}

	fn emit_type_query_recursive(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let Some(identifier) = Self::first_type_query_identifier(node) else {
			return;
		};
		let name = node_slice(identifier, self.source);
		if name.is_empty() {
			return;
		}
		let (target, confidence) = self.refs.type_query_value_target(name, graph);
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

	fn type_confidence(&self, name: &[u8]) -> &'static [u8] {
		if is_global_type(name) || !self.type_table.contains_key(name) {
			kinds::CONF_EXTERNAL
		} else {
			self.refs.ref_confidence(name)
		}
	}
}

struct TsCalls<'a, 'src> {
	source: &'src [u8],
	module: &'a Moniker,
	imports: &'a TsImports,
	refs: TsReferenceResolver<'a>,
	di_register_callees: &'a [String],
}

struct CallFollowup<'src> {
	read_nodes: Vec<Node<'src>>,
	recurse_nodes: Vec<Node<'src>>,
	walk_nodes: Vec<Node<'src>>,
}

impl<'src> CallFollowup<'src> {
	fn new() -> Self {
		Self {
			read_nodes: Vec::new(),
			recurse_nodes: Vec::new(),
			walk_nodes: Vec::new(),
		}
	}
}

impl<'src> TsCalls<'_, 'src> {
	fn handle_call(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) -> CallFollowup<'src> {
		let mut followup = CallFollowup::new();
		let pos = node_position(node);
		let Some(fn_node) = node.child_by_field_name("function") else {
			followup.walk_nodes.push(node);
			return followup;
		};
		match fn_node.kind() {
			"identifier" => self.handle_identifier_call(node, fn_node, scope, graph, pos),
			"member_expression" => {
				self.handle_member_call(node, fn_node, scope, graph, pos, &mut followup);
			}
			_ => {}
		}
		if let Some(args) = node.child_by_field_name("arguments") {
			followup.walk_nodes.push(args);
		}
		followup
	}

	fn handle_identifier_call(
		&self,
		node: Node<'_>,
		fn_node: Node<'_>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		pos: Position,
	) {
		let name = node_slice(fn_node, self.source);
		if let Some(confidence) = self.refs.name_confidence(name) {
			let (target, confidence) = self.refs.call_identifier_target(name, scope, confidence);
			let attrs = RefAttrs {
				confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(scope, target, kinds::CALLS, Some(pos), &attrs);
		}
		self.maybe_emit_di_register(node, name, scope, graph, pos);
	}

	fn handle_member_call(
		&self,
		node: Node<'_>,
		fn_node: Node<'src>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
		pos: Position,
		followup: &mut CallFollowup<'src>,
	) {
		if let Some(prop) = fn_node.child_by_field_name("property") {
			let name = node_slice(prop, self.source);
			if !name.is_empty() {
				self.emit_member_call_ref(node, fn_node, name, scope, graph, pos);
			}
		}
		if let Some(obj) = fn_node.child_by_field_name("object") {
			if obj.kind() == "identifier" {
				followup.read_nodes.push(obj);
			} else {
				followup.recurse_nodes.push(obj);
			}
		}
	}

	fn emit_member_call_ref(
		&self,
		node: Node<'_>,
		fn_node: Node<'_>,
		name: &[u8],
		scope: &Moniker,
		graph: &mut SdkBuilder,
		pos: Position,
	) {
		let obj_ident = fn_node
			.child_by_field_name("object")
			.filter(|object| object.kind() == "identifier")
			.map(|object| node_slice(object, self.source));
		let imported_target = obj_ident.and_then(|name| self.imports.target(name));
		let target = imported_target
			.clone()
			.map(|module| extend_segment(&module, kinds::METHOD, name))
			.unwrap_or_else(|| external_runtime_target(self.module, kinds::METHOD, name));
		let confidence = imported_target
			.as_ref()
			.map(|_| {
				obj_ident
					.map(|name| self.refs.ref_confidence(name))
					.unwrap_or(kinds::CONF_EXTERNAL)
			})
			.unwrap_or(kinds::CONF_EXTERNAL);
		let attrs = RefAttrs {
			receiver_hint: receiver_hint(fn_node, self.source),
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::METHOD_CALL, Some(pos), &attrs);
		self.maybe_emit_di_register(node, name, scope, graph, pos);
	}

	fn maybe_emit_di_register(
		&self,
		call: Node<'_>,
		callee_name: &[u8],
		scope: &Moniker,
		graph: &mut SdkBuilder,
		pos: Position,
	) {
		if self.di_register_callees.is_empty() {
			return;
		}
		let callee_str = match std::str::from_utf8(callee_name) {
			Ok(value) => value,
			Err(_) => return,
		};
		if !self
			.di_register_callees
			.iter()
			.any(|pattern| pattern == callee_str)
		{
			return;
		}
		let Some(args) = call.child_by_field_name("arguments") else {
			return;
		};
		let mut cursor = args.walk();
		for child in args.children(&mut cursor) {
			if !child.is_named() {
				continue;
			}
			if let Some(name) = self.find_di_factory(child) {
				let target = extend_segment(self.module, kinds::CLASS, name);
				let attrs = RefAttrs {
					confidence: kinds::CONF_NAME_MATCH,
					..RefAttrs::default()
				};
				let _ = graph.add_ref_attrs(scope, target, kinds::DI_REGISTER, Some(pos), &attrs);
			}
		}
	}

	fn find_di_factory<'a>(&'a self, node: Node<'a>) -> Option<&'a [u8]>
	where
		'src: 'a,
	{
		match node.kind() {
			"identifier" => {
				let name = node_slice(node, self.source);
				(!name.is_empty()).then_some(name)
			}
			"call_expression" => {
				let fn_node = node.child_by_field_name("function")?;
				match fn_node.kind() {
					"member_expression" => fn_node
						.child_by_field_name("object")
						.and_then(|object| self.find_di_factory(object)),
					"identifier" => {
						let inner_args = node.child_by_field_name("arguments")?;
						let mut cursor = inner_args.walk();
						for child in inner_args.children(&mut cursor) {
							if !child.is_named() {
								continue;
							}
							if let Some(name) = self.find_di_factory(child) {
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
}

struct TsClassifier<'a, 'src> {
	discover: &'a TsDiscover<'src>,
}

impl<'a, 'src_lang> TsClassifier<'a, 'src_lang> {
	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		if let Some(shape) = self.classify_module_or_declaration(node, scope, source, graph) {
			return shape;
		}
		if let Some(shape) = self.classify_runtime(node, scope, source, graph) {
			return shape;
		}
		if let Some(shape) = self.classify_type_surface(node, scope, graph) {
			return shape;
		}
		if let Some(shape) = self.classify_expression(node, scope, graph) {
			return shape;
		}
		NodeShape::Recurse
	}

	fn classify_module_or_declaration<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> Option<NodeShape<'src>> {
		match node.kind() {
			"comment" => Some(NodeShape::Annotation {
				kind: kinds::COMMENT,
			}),
			"import_statement" => {
				self.discover.imports.handle_import(
					node,
					scope,
					graph,
					self.discover.import_context(),
				);
				Some(NodeShape::Skip)
			}
			"export_statement" => Some(classify_export(self.discover, node, scope, source, graph)),
			"class_declaration" | "abstract_class_declaration" => Some(classify_class(
				self.discover,
				node,
				scope,
				source,
				None,
				None,
			)),
			"interface_declaration" => Some(classify_interface(self.discover, node, scope, source)),
			"enum_declaration" => Some(classify_enum(self.discover, node, scope, source)),
			"type_alias_declaration" => Some(classify_type_alias(
				self.discover,
				node,
				scope,
				source,
				graph,
			)),
			"function_declaration" | "generator_function_declaration" => {
				Some(classify_function_decl(self.discover, node, scope, source))
			}
			"method_definition" | "method_signature" => {
				Some(classify_method(self.discover, node, scope, source))
			}
			"public_field_definition" | "property_signature" => {
				Some(classify_field(self.discover, node, scope, source))
			}
			_ => None,
		}
	}

	fn classify_runtime<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> Option<NodeShape<'src>> {
		match node.kind() {
			"lexical_declaration" | "variable_declaration" => {
				handle_lexical(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"call_expression" => {
				handle_call(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"new_expression" => {
				handle_new(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"decorator" => {
				handle_decorator(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"arrow_function" | "function_expression" => Some(classify_inline_callable(
				self.discover,
				node,
				scope,
				source,
				graph,
			)),
			"pair" => Some(classify_pair(self.discover, node, scope, source)),
			"catch_clause" => {
				handle_catch_clause(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"for_in_statement" | "for_of_statement" => {
				handle_for_in(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			_ => None,
		}
	}

	fn classify_type_surface<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) -> Option<NodeShape<'src>> {
		match node.kind() {
			"type_annotation"
			| "type_arguments"
			| "union_type"
			| "intersection_type"
			| "lookup_type"
			| "index_type_query"
			| "type_query"
			| "generic_type"
			| "nested_type_identifier" => {
				emit_uses_type_recursive(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			_ => None,
		}
	}

	fn classify_expression<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) -> Option<NodeShape<'src>> {
		match node.kind() {
			"return_statement"
			| "spread_element"
			| "parenthesized_expression"
			| "template_substitution"
			| "arguments"
			| "array" => {
				emit_reads_in_children(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"binary_expression" | "assignment_expression" => {
				dispatch_fields(self.discover, node, scope, graph, &["left", "right"]);
				Some(NodeShape::Skip)
			}
			"unary_expression" | "update_expression" => {
				dispatch_fields(self.discover, node, scope, graph, &["argument"]);
				Some(NodeShape::Skip)
			}
			"ternary_expression" => {
				dispatch_fields(
					self.discover,
					node,
					scope,
					graph,
					&["condition", "consequence", "alternative"],
				);
				Some(NodeShape::Skip)
			}
			"member_expression" | "subscript_expression" => {
				dispatch_fields(self.discover, node, scope, graph, &["object"]);
				Some(NodeShape::Skip)
			}
			"shorthand_property_identifier" => {
				emit_read_at(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"jsx_expression" => {
				emit_reads_in_children(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			"jsx_opening_element" | "jsx_self_closing_element" => {
				handle_jsx_element(self.discover, node, scope, graph);
				Some(NodeShape::Skip)
			}
			_ => None,
		}
	}
}

pub(super) struct TsDiscover<'src> {
	pub(super) module: Moniker,
	pub(super) anchor: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) presets: &'src super::super::Presets,
	pub(super) export_ranges: Vec<(u32, u32)>,
	scopes: TsScopes,
	imports: TsImports,
	pub(super) type_table: HashMap<Vec<u8>, Moniker>,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry>,
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

struct VariableBinding {
	kind: &'static [u8],
	visibility: &'static [u8],
	emit: bool,
	infer_return_type: bool,
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
			scopes: TsScopes::default(),
			imports: TsImports::default(),
			type_table,
			callable_table,
		};
		let mut builder = SdkBuilder::new(module.clone());
		SdkWalker::new(&discover, source_bytes).walk(root, &module, &mut builder);
		builder.finish()
	}

	fn import_context(&self) -> TsImportContext<'_, 'a> {
		TsImportContext {
			module: &self.module,
			anchor: &self.anchor,
			source: self.source_bytes,
			path_aliases: &self.presets.path_aliases,
		}
	}

	fn refs(&self) -> TsReferenceResolver<'_> {
		TsReferenceResolver {
			module: &self.module,
			imports: &self.imports,
			type_table: &self.type_table,
			callable_table: &self.callable_table,
			scopes: &self.scopes,
			deep: self.deep,
		}
	}

	fn type_refs(&self) -> TsTypeRefs<'_, 'a> {
		TsTypeRefs {
			source: self.source_bytes,
			module: &self.module,
			imports: &self.imports,
			type_table: &self.type_table,
			refs: self.refs(),
		}
	}

	fn calls(&self) -> TsCalls<'_, 'a> {
		TsCalls {
			source: self.source_bytes,
			module: &self.module,
			imports: &self.imports,
			refs: self.refs(),
			di_register_callees: &self.presets.di_register_callees,
		}
	}

	fn classifier(&self) -> TsClassifier<'_, 'a> {
		TsClassifier { discover: self }
	}

	fn classify<'src>(
		&self,
		node: Node<'src>,
		scope: &Moniker,
		source: &'src [u8],
		graph: &mut SdkBuilder,
	) -> NodeShape<'src> {
		self.classifier().classify(node, scope, source, graph)
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
			emit_uses_type_recursive(self, rt, moniker, graph);
		}
		if let Some(params) = node.child_by_field_name("parameters") {
			bind_and_emit_params(self, params, moniker, graph);
		}
		if let Some(p) = node.child_by_field_name("parameter") {
			bind_and_emit_param_leaf(self, p, moniker, graph);
		}
	}

	fn after_body(&self, kind: &[u8], _moniker: &Moniker) {
		if is_callable_kind(kind) {
			self.scopes.pop();
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
					walk_decorator_args(self, c, sym_moniker, graph);
				}
			}
		}
		if sym_kind == kinds::ENUM {
			emit_enum_constants(self, node, sym_moniker, graph);
		}
		if sym_kind == kinds::FIELD {
			if let Some(tp) = node.child_by_field_name("type") {
				emit_uses_type_recursive(self, tp, sym_moniker, graph);
			}
			if let Some(value) = node.child_by_field_name("value") {
				self.recurse_subtree(value, sym_moniker, graph);
			}
		}
	}
}

fn classify_class<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
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
	if is_callable_scope(scope, &discover.module) {
		discover.scopes.bind_local(name, moniker.clone());
	}

	let mut annotated_by: Vec<RefSpec> = Vec::new();
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		match child.kind() {
			"decorator" => collect_decorator_ref(discover, child, &mut annotated_by),
			"class_heritage" => {
				collect_heritage_refs_from_clauses(discover, child, &mut annotated_by)
			}
			_ => {}
		}
	}

	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::CLASS,
		visibility: visibility_override.unwrap_or_else(|| discover.module_visibility(node)),
		signature: None,
		body: node.child_by_field_name("body"),
		position: node_position(node),
		annotated_by,
	})
}

fn classify_interface<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
) -> NodeShape<'src> {
	let Some(name_node) = node.child_by_field_name("name") else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, source);
	let moniker = extend_segment(scope, kinds::INTERFACE, name);
	if is_callable_scope(scope, &discover.module) {
		discover.scopes.bind_local(name, moniker.clone());
	}

	let mut annotated_by: Vec<RefSpec> = Vec::new();
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if matches!(child.kind(), "extends_type_clause" | "extends_clause") {
			emit_heritage_refs_collect(discover, child, kinds::EXTENDS, &mut annotated_by);
		}
	}

	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::INTERFACE,
		visibility: discover.module_visibility(node),
		signature: None,
		body: node.child_by_field_name("body"),
		position: node_position(node),
		annotated_by,
	})
}

fn classify_enum<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
) -> NodeShape<'src> {
	let Some(name_node) = node.child_by_field_name("name") else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, source);
	let moniker = extend_segment(scope, kinds::ENUM, name);
	if is_callable_scope(scope, &discover.module) {
		discover.scopes.bind_local(name, moniker.clone());
	}
	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::ENUM,
		visibility: discover.module_visibility(node),
		signature: None,
		body: None,
		position: node_position(node),
		annotated_by: Vec::new(),
	})
}

fn classify_type_alias<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
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
	if is_callable_scope(scope, &discover.module) {
		discover.scopes.bind_local(name, moniker.clone());
	}
	if let Some(value) = node.child_by_field_name("value") {
		emit_uses_type_recursive(discover, value, &moniker, graph);
	}
	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::TYPE,
		visibility: discover.module_visibility(node),
		signature: None,
		body: None,
		position: node_position(node),
		annotated_by: Vec::new(),
	})
}

fn classify_function_decl<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
) -> NodeShape<'src> {
	let Some(name_node) = node.child_by_field_name("name") else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, source);
	discover.callable_symbol(
		node,
		node,
		name,
		kinds::FUNCTION,
		scope,
		discover.module_visibility(node),
	)
}

fn classify_method<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
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
	discover.callable_symbol(node, node, name, kind, scope, vis)
}

fn classify_field<'src, 'src_lang>(
	_discover: &TsDiscover<'src_lang>,
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

fn classify_inline_callable<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
	graph: &mut SdkBuilder,
) -> NodeShape<'src> {
	if discover.deep && is_callable_scope(scope, &discover.module) {
		let name = anonymous_callback_name(node);
		return discover.callable_symbol(
			node,
			node,
			&name,
			kinds::FUNCTION,
			scope,
			kinds::VIS_NONE,
		);
	}
	discover.scopes.push();
	if let Some(params) = node.child_by_field_name("parameters") {
		bind_and_emit_params(discover, params, scope, graph);
	}
	if let Some(p) = node.child_by_field_name("parameter") {
		bind_and_emit_param_leaf(discover, p, scope, graph);
	}
	if let Some(body) = node.child_by_field_name("body") {
		discover.walk_children(body, scope, graph);
	}
	discover.scopes.pop();
	let _ = source;
	NodeShape::Skip
}

fn classify_pair<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
) -> NodeShape<'src> {
	if discover.deep && is_callable_scope(scope, &discover.module) {
		let key = node.child_by_field_name("key");
		let value = node.child_by_field_name("value");
		if let (Some(k), Some(v)) = (key, value)
			&& k.kind() == "property_identifier"
			&& (v.kind() == "arrow_function" || v.kind() == "function_expression")
		{
			let name = node_slice(k, source);
			return discover.callable_symbol(
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

fn classify_export<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
	graph: &mut SdkBuilder,
) -> NodeShape<'src> {
	if node.child_by_field_name("source").is_some() {
		discover
			.imports
			.handle_reexport(node, scope, graph, discover.import_context());
		return NodeShape::Skip;
	}
	discover
		.imports
		.handle_bare_reexport(node, scope, graph, discover.import_context());
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
					return discover.callable_symbol(
						c,
						c,
						b"default",
						kinds::FUNCTION,
						scope,
						kinds::VIS_PUBLIC,
					);
				}
				"class" | "class_declaration" => {
					return classify_class(
						discover,
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

fn handle_lexical<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let inside_callable = is_callable_scope(scope, &discover.module);
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() != "variable_declarator" {
			continue;
		}
		handle_variable_declarator(discover, child, scope, inside_callable, graph);
	}
}

fn handle_variable_declarator<'src_lang>(
	discover: &TsDiscover<'src_lang>,
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

	let names = collect_binding_names(name_node, discover.source_bytes);
	let module_vis = discover.module_visibility(decl);
	for name in &names {
		if inside_callable {
			discover
				.scopes
				.bind_local(name, extend_segment(scope, kinds::LOCAL, name));
		}
		if !inside_callable
			&& let Some(value) =
				value.filter(|node| matches!(node.kind(), "arrow_function" | "function_expression"))
		{
			emit_assigned_function(discover, decl, value, scope, name, module_vis, graph);
			continue;
		}
		emit_variable_binding(
			discover,
			decl,
			scope,
			name,
			VariableBinding {
				kind: if inside_callable {
					kinds::LOCAL
				} else {
					kinds::CONST
				},
				visibility: if inside_callable {
					kinds::VIS_NONE
				} else {
					module_vis
				},
				emit: !inside_callable || discover.deep,
				infer_return_type: !inside_callable && type_annot.is_none(),
			},
			value,
			graph,
		);
	}

	emit_variable_type_and_value(discover, type_annot, value, scope, graph);
}

fn emit_assigned_function<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	decl: Node<'_>,
	value: Node<'_>,
	scope: &Moniker,
	name: &[u8],
	visibility: &'static [u8],
	graph: &mut SdkBuilder,
) {
	let slots = callable_param_slots(value, discover.source_bytes);
	let moniker = extend_callable_slots(scope, kinds::FUNCTION, name, &slots);
	let attrs = crate::core::code_graph::DefAttrs {
		visibility,
		..crate::core::code_graph::DefAttrs::default()
	};
	let _ = graph.add_def_attrs(
		moniker.clone(),
		kinds::FUNCTION,
		scope,
		Some(node_position(decl)),
		&attrs,
	);
	discover.scopes.push();
	if let Some(return_type) = value.child_by_field_name("return_type") {
		emit_uses_type_recursive(discover, return_type, &moniker, graph);
	}
	if let Some(params) = value.child_by_field_name("parameters") {
		bind_and_emit_params(discover, params, &moniker, graph);
	}
	if let Some(param) = value.child_by_field_name("parameter") {
		bind_and_emit_param_leaf(discover, param, &moniker, graph);
	}
	if let Some(body) = value.child_by_field_name("body") {
		discover.walk_children(body, &moniker, graph);
	}
	discover.scopes.pop();
}

fn emit_variable_binding<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	decl: Node<'_>,
	scope: &Moniker,
	name: &[u8],
	binding: VariableBinding,
	value: Option<Node<'_>>,
	graph: &mut SdkBuilder,
) {
	if !binding.emit {
		return;
	}
	let moniker = extend_segment(scope, binding.kind, name);
	let attrs = crate::core::code_graph::DefAttrs {
		visibility: binding.visibility,
		..crate::core::code_graph::DefAttrs::default()
	};
	let _ = graph.add_def_attrs(
		moniker.clone(),
		binding.kind,
		scope,
		Some(node_position(decl)),
		&attrs,
	);
	if binding.infer_return_type
		&& let Some((target, confidence)) = value_call_target(discover, value, scope)
	{
		let ref_attrs = RefAttrs {
			confidence,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			&moniker,
			target,
			kinds::RETURNS_TYPE,
			Some(node_position(decl)),
			&ref_attrs,
		);
	}
}

fn emit_variable_type_and_value<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	type_annot: Option<Node<'_>>,
	value: Option<Node<'_>>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if let Some(tp) = type_annot {
		emit_uses_type_recursive(discover, tp, scope, graph);
	}
	if let Some(v) = value {
		if v.kind() == "identifier" {
			emit_read_at(discover, v, scope, graph);
		} else {
			discover.recurse_subtree(v, scope, graph);
		}
	}
}

fn handle_catch_clause<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if is_callable_scope(scope, &discover.module)
		&& let Some(p) = node.child_by_field_name("parameter")
	{
		bind_and_emit_param_leaf(discover, p, scope, graph);
	}
	if let Some(body) = node.child_by_field_name("body") {
		discover.walk_children(body, scope, graph);
	}
}

fn handle_for_in<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if is_callable_scope(scope, &discover.module) {
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
			for name in collect_binding_names(binding_node, discover.source_bytes) {
				let m = extend_segment(scope, kinds::LOCAL, &name);
				discover.scopes.bind_local(&name, m.clone());
				if discover.deep {
					let _ =
						graph.add_def(m, kinds::LOCAL, scope, Some(node_position(binding_node)));
				}
			}
			break;
		}
	}
	discover.walk_children(node, scope, graph);
}

fn value_call_target<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	value: Option<Node<'_>>,
	scope: &Moniker,
) -> Option<(Moniker, &'static [u8])> {
	let call = value.filter(|v| v.kind() == "call_expression")?;
	let fn_node = call
		.child_by_field_name("function")
		.filter(|f| f.kind() == "identifier")?;
	let name = node_slice(fn_node, discover.source_bytes);
	let refs = discover.refs();
	let confidence = refs.name_confidence(name)?;
	Some(refs.call_identifier_target(name, scope, confidence))
}

fn handle_call<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let followup = discover.calls().handle_call(node, scope, graph);
	for read_node in followup.read_nodes {
		emit_read_at(discover, read_node, scope, graph);
	}
	for recurse_node in followup.recurse_nodes {
		discover.recurse_subtree(recurse_node, scope, graph);
	}
	for walk_node in followup.walk_nodes {
		discover.walk_children(walk_node, scope, graph);
	}
}

fn handle_new<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let pos = node_position(node);
	if let Some(ctor) = node.child_by_field_name("constructor") {
		let name = match ctor.kind() {
			"identifier" | "type_identifier" => Some(node_slice(ctor, discover.source_bytes)),
			"member_expression" => ctor
				.child_by_field_name("property")
				.map(|p| node_slice(p, discover.source_bytes)),
			_ => None,
		};
		if let Some(n) = name
			&& !n.is_empty()
		{
			let confidence = match ctor.kind() {
				"identifier" | "type_identifier" => discover.refs().ref_confidence(n),
				"member_expression" => ctor
					.child_by_field_name("object")
					.filter(|o| o.kind() == "identifier")
					.map(|o| {
						discover
							.refs()
							.ref_confidence(node_slice(o, discover.source_bytes))
					})
					.unwrap_or(kinds::CONF_NAME_MATCH),
				_ => kinds::CONF_NAME_MATCH,
			};
			let target = if confidence == kinds::CONF_IMPORTED || confidence == kinds::CONF_EXTERNAL
			{
				let base = discover
					.imports
					.module_for(n)
					.unwrap_or_else(|| discover.module.clone());
				extend_segment(&base, kinds::CLASS, n)
			} else if let Some(m) = discover.refs().lookup_local_binding(n) {
				m
			} else if let Some(m) = discover.type_table.get(n) {
				m.clone()
			} else {
				external_runtime_target(&discover.module, kinds::CLASS, n)
			};
			let attrs = RefAttrs {
				confidence: if is_global_type(n) || !discover.type_table.contains_key(n) {
					kinds::CONF_EXTERNAL
				} else if discover.type_table.values().any(|m| m == &target)
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
	discover.walk_children(node, scope, graph);
}

fn decorator_callees<'src, 'src_lang>(
	discover: &TsDiscover<'src_lang>,
	decorator: Node<'src>,
) -> Vec<DecoratorCallee<'src>>
where
	'src_lang: 'src,
{
	let mut out = Vec::new();
	let mut cursor = decorator.walk();
	for ch in decorator.children(&mut cursor) {
		match ch.kind() {
			"identifier" => {
				let name = node_slice(ch, discover.source_bytes);
				if !name.is_empty() {
					out.push(DecoratorCallee { name, args: None });
				}
			}
			"call_expression" => {
				if let Some(fn_node) = ch.child_by_field_name("function")
					&& fn_node.kind() == "identifier"
				{
					let name = node_slice(fn_node, discover.source_bytes);
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

fn handle_decorator<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let pos = node_position(node);
	for callee in decorator_callees(discover, node) {
		let target = extend_segment(&discover.module, kinds::FUNCTION, callee.name);
		let attrs = RefAttrs {
			confidence: discover.refs().ref_confidence(callee.name),
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::ANNOTATES, Some(pos), &attrs);
		if let Some(args) = callee.args {
			discover.walk_children(args, scope, graph);
		}
	}
}

fn walk_decorator_args<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	decorator: Node<'_>,
	sym_moniker: &Moniker,
	graph: &mut SdkBuilder,
) {
	for callee in decorator_callees(discover, decorator) {
		if let Some(args) = callee.args {
			discover.walk_children(args, sym_moniker, graph);
		}
	}
}

fn collect_decorator_ref<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	decorator: Node<'_>,
	out: &mut Vec<RefSpec>,
) {
	let pos = node_position(decorator);
	for callee in decorator_callees(discover, decorator) {
		let target = extend_segment(&discover.module, kinds::FUNCTION, callee.name);
		out.push(RefSpec {
			kind: kinds::ANNOTATES,
			target,
			confidence: discover.refs().ref_confidence(callee.name),
			position: pos,
			receiver_hint: b"",
			alias: b"",
		});
	}
}

fn collect_heritage_refs_from_clauses<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	out: &mut Vec<RefSpec>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let edge: &'static [u8] = match child.kind() {
			"extends_clause" => kinds::EXTENDS,
			"implements_clause" => kinds::IMPLEMENTS,
			_ => continue,
		};
		emit_heritage_refs_collect(discover, child, edge, out);
	}
}

fn emit_heritage_refs_collect<'src_lang>(
	discover: &TsDiscover<'src_lang>,
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
			"identifier" | "type_identifier" => Some(node_slice(c, discover.source_bytes)),
			"member_expression" => c
				.child_by_field_name("property")
				.map(|p| node_slice(p, discover.source_bytes)),
			"generic_type" => generic_short(c, discover.source_bytes),
			"nested_type_identifier" => nested_type_short(c, discover.source_bytes),
			_ => None,
		};
		let Some(name) = name.filter(|n| !n.is_empty()) else {
			continue;
		};
		let (target, confidence) = discover.refs().heritage_target(name, target_kind);
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

fn emit_uses_type_recursive<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	discover.type_refs().emit_recursive(node, scope, graph);
}

fn emit_reads_in_children<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		if c.kind() == "identifier" {
			emit_read_at(discover, c, scope, graph);
		} else {
			discover.recurse_subtree(c, scope, graph);
		}
	}
}

fn emit_read_at<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let name = node_slice(node, discover.source_bytes);
	if name.is_empty() {
		return;
	}
	let refs = discover.refs();
	let Some(confidence) = refs.name_confidence(name) else {
		return;
	};
	let (target, confidence) = refs.read_target(name, scope, confidence, graph);
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

fn dispatch_fields<'src_lang>(
	discover: &TsDiscover<'src_lang>,
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
			emit_read_at(discover, c, scope, graph);
		} else {
			discover.recurse_subtree(c, scope, graph);
		}
	}
}

fn handle_jsx_element<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if let Some(name) = node.child_by_field_name("name")
		&& name.kind() == "identifier"
		&& !is_intrinsic_jsx_tag(node_slice(name, discover.source_bytes))
	{
		emit_read_at(discover, name, scope, graph);
	}
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		match c.kind() {
			"jsx_attribute" => {
				if let Some(v) = c.child_by_field_name("value") {
					discover.recurse_subtree(v, scope, graph);
				}
			}
			"jsx_text" => {}
			_ => discover.recurse_subtree(c, scope, graph),
		}
	}
}
fn emit_enum_constants<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	enum_node: Node<'_>,
	parent: &Moniker,
	graph: &mut SdkBuilder,
) {
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
		let name = node_slice(name_node, discover.source_bytes);
		if name.is_empty() {
			continue;
		}
		let m = extend_segment(parent, kinds::ENUM_CONSTANT, name);
		let _ = graph.add_def(m, kinds::ENUM_CONSTANT, parent, Some(node_position(member)));
	}
}

fn bind_and_emit_params<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	params: Node<'_>,
	callable: &Moniker,
	graph: &mut SdkBuilder,
) {
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		match child.kind() {
			"required_parameter" | "optional_parameter" => {
				if let Some(p) = child.child_by_field_name("pattern") {
					bind_and_emit_param_leaf(discover, p, callable, graph);
				}
				if let Some(t) = child.child_by_field_name("type") {
					emit_uses_type_recursive(discover, t, callable, graph);
				}
			}
			"rest_pattern" => {
				bind_and_emit_param_leaf(discover, child, callable, graph);
			}
			_ => {}
		}
	}
}

fn bind_and_emit_param_leaf<'src_lang>(
	discover: &TsDiscover<'src_lang>,
	pat: Node<'_>,
	callable: &Moniker,
	graph: &mut SdkBuilder,
) {
	for name in collect_binding_names(pat, discover.source_bytes) {
		let m = extend_segment(callable, kinds::PARAM, &name);
		discover.scopes.bind_local(&name, m.clone());
		if discover.deep {
			let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
		}
	}
}

impl<'src_lang> TsDiscover<'src_lang> {
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

		self.scopes.push();
		let body = callable_node.child_by_field_name("body");
		if let Some(body) = body {
			self.scopes
				.hoist_nested_funcs(body, &moniker, self.source_bytes);
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
	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let walker = SdkWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}

	fn walk_children(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let walker = SdkWalker::new(self, self.source_bytes);
		walker.walk(node, scope, graph);
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
