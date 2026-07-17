// code-moniker: ignore-file[smell-feature-envy-local, smell-long-parameter-list, smell-data-clumps-param-names, smell-god-type-local-metrics, smell-harmonious-method-size, smell-large-type, smell-vertical-layout]
// TODO(smell): split Python Strategy into classification, import/type/call resolution, local-scope tracking, and graph emission phases before enabling these guardrails here.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::{DefAttrs, Position, RefAttrs};
use crate::core::moniker::{Moniker, MonikerBuilder};

use crate::lang::callable::{
	CallableSlot, callable_segment_slots, extend_callable_slots, extend_segment,
	join_bytes_with_comma, slot_signature_bytes,
};
use crate::lang::sdk::{DiscoveredDef, Namespace, RefHints, ResolvedRef};
use crate::lang::tree_util::{node_position, node_slice};

use super::kinds;
use super::local_types::LocalTypeSet;

pub(super) struct PyDiscover<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) imports: PyImportBindings,
	pub(super) locals: PyLocalScopes,
	pub(super) instance_attr_types: RefCell<HashMap<(Moniker, Vec<u8>), Moniker>>,
	pub(super) declared_instance_attr_types: HashSet<(Moniker, Vec<u8>)>,
	pub(super) ambiguous_instance_attr_types: RefCell<HashSet<(Moniker, Vec<u8>)>>,
	pub(super) type_table: TypeTable,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry>,
}

pub(super) type TypeTable = HashMap<Vec<u8>, Vec<Moniker>>;

#[derive(Clone)]
pub(super) struct CallableEntry {
	pub(super) kind: &'static [u8],
	pub(super) segment: Vec<u8>,
	pub(super) return_type_names: Vec<Vec<u8>>,
	pub(super) return_type_dynamic: bool,
	pub(super) return_type_ambiguous: bool,
	pub(super) is_async: bool,
}

pub(super) struct DiscoveredPythonFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
}

type ScopedBindings<T> = HashMap<Moniker, HashMap<Vec<u8>, T>>;

pub(super) struct PyImportBindings {
	confidences: RefCell<ScopedBindings<&'static [u8]>>,
	targets: RefCell<ScopedBindings<Moniker>>,
	runtime_bindings: RefCell<HashMap<Moniker, HashSet<Vec<u8>>>>,
}

impl PyImportBindings {
	fn new() -> Self {
		Self {
			confidences: RefCell::new(HashMap::new()),
			targets: RefCell::new(HashMap::new()),
			runtime_bindings: RefCell::new(HashMap::new()),
		}
	}

	fn bind(&self, scope: &Moniker, name: &[u8], confidence: &'static [u8], conditional: bool) {
		self.confidences
			.borrow_mut()
			.entry(scope.clone())
			.or_default()
			.insert(name.to_vec(), confidence);
		let mut runtime_bindings = self.runtime_bindings.borrow_mut();
		let bindings = runtime_bindings.entry(scope.clone()).or_default();
		if conditional {
			bindings.insert(name.to_vec());
		} else {
			bindings.remove(name);
		}
	}

	fn bind_target(&self, scope: &Moniker, name: &[u8], target: &Moniker) {
		if name.is_empty() {
			return;
		}
		self.targets
			.borrow_mut()
			.entry(scope.clone())
			.or_default()
			.insert(name.to_vec(), target.clone());
	}

	fn confidence_for(
		&self,
		scope: &Moniker,
		module: &Moniker,
		name: &[u8],
	) -> Option<&'static [u8]> {
		let confidences = self.confidences.borrow();
		confidences
			.get(scope)
			.and_then(|bindings| bindings.get(name))
			.or_else(|| {
				confidences
					.get(module)
					.and_then(|bindings| bindings.get(name))
			})
			.copied()
	}

	fn target_for(&self, scope: &Moniker, module: &Moniker, name: &[u8]) -> Option<Moniker> {
		let targets = self.targets.borrow();
		targets
			.get(scope)
			.and_then(|bindings| bindings.get(name))
			.or_else(|| targets.get(module).and_then(|bindings| bindings.get(name)))
			.cloned()
	}

	fn is_runtime_binding(&self, scope: &Moniker, module: &Moniker, name: &[u8]) -> bool {
		let confidences = self.confidences.borrow();
		let binding_scope = if confidences
			.get(scope)
			.is_some_and(|bindings| bindings.contains_key(name))
		{
			scope
		} else {
			module
		};
		let runtime = self.runtime_bindings.borrow();
		runtime
			.get(binding_scope)
			.is_some_and(|bindings| bindings.contains(name))
	}
}

pub(super) struct PyLocalScopes {
	names: RefCell<Vec<HashSet<Vec<u8>>>>,
	types: RefCell<Vec<HashMap<Vec<u8>, LocalTypeSet>>>,
	element_types: RefCell<Vec<HashMap<Vec<u8>, LocalTypeSet>>>,
	type_bindings: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
}

impl PyLocalScopes {
	fn new() -> Self {
		Self {
			names: RefCell::new(Vec::new()),
			types: RefCell::new(Vec::new()),
			element_types: RefCell::new(Vec::new()),
			type_bindings: RefCell::new(Vec::new()),
		}
	}

	fn push(&self) {
		self.names.borrow_mut().push(HashSet::new());
		self.types.borrow_mut().push(HashMap::new());
		self.element_types.borrow_mut().push(HashMap::new());
		self.type_bindings.borrow_mut().push(HashMap::new());
	}

	fn pop(&self) {
		self.names.borrow_mut().pop();
		self.types.borrow_mut().pop();
		self.element_types.borrow_mut().pop();
		self.type_bindings.borrow_mut().pop();
	}

	fn record_name(&self, name: &[u8]) {
		if let Some(top) = self.names.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn is_name(&self, name: &[u8]) -> bool {
		self.names.borrow().iter().any(|frame| frame.contains(name))
	}

	fn record_type_set(&self, name: &[u8], targets: LocalTypeSet) {
		if let Some(types) = self.types.borrow_mut().last_mut() {
			types.entry(name.to_vec()).or_default().union_with(targets);
		}
	}

	fn record_type_binding(&self, name: &[u8], target: Moniker) {
		if let Some(bindings) = self.type_bindings.borrow_mut().last_mut() {
			bindings.insert(name.to_vec(), target);
		}
	}

	fn record_element_type_set(&self, name: &[u8], targets: LocalTypeSet) {
		if let Some(types) = self.element_types.borrow_mut().last_mut() {
			types.entry(name.to_vec()).or_default().union_with(targets);
		}
	}

	fn record_unknown_element_type(&self, name: &[u8]) {
		if let Some(types) = self.element_types.borrow_mut().last_mut() {
			types.entry(name.to_vec()).or_default().mark_dynamic();
		}
	}

	fn record_unknown_type(&self, name: &[u8]) {
		if let Some(types) = self.types.borrow_mut().last_mut() {
			types.entry(name.to_vec()).or_default().mark_dynamic();
		}
	}

	fn lookup_type(&self, name: &[u8]) -> Option<Moniker> {
		self.lookup_type_set(name)?.unique()
	}

	fn lookup_type_set(&self, name: &[u8]) -> Option<LocalTypeSet> {
		let names = self.names.borrow();
		let types = self.types.borrow();
		for idx in (0..names.len()).rev() {
			if names[idx].contains(name) {
				return types[idx].get(name).cloned();
			}
		}
		None
	}

	fn lookup_element_type_set(&self, name: &[u8]) -> Option<LocalTypeSet> {
		let names = self.names.borrow();
		let element_types = self.element_types.borrow();
		for idx in (0..names.len()).rev() {
			if names[idx].contains(name) {
				return element_types[idx].get(name).cloned();
			}
		}
		None
	}

	fn current_type_sets(&self) -> Vec<(Vec<u8>, LocalTypeSet)> {
		let mut current = self
			.types
			.borrow()
			.last()
			.into_iter()
			.flat_map(|types| types.iter())
			.map(|(name, types)| (name.clone(), types.clone()))
			.collect::<Vec<_>>();
		current.sort_by(|left, right| left.0.cmp(&right.0));
		current
	}

	fn lookup_type_binding(&self, name: &[u8]) -> Option<Moniker> {
		let names = self.names.borrow();
		let bindings = self.type_bindings.borrow();
		for idx in (0..names.len()).rev() {
			if names[idx].contains(name) {
				return bindings[idx].get(name).cloned();
			}
		}
		None
	}
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
	call_name: Vec<u8>,
	call_arity: Option<usize>,
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

struct CallResolution {
	target: Moniker,
	kind: &'static [u8],
	confidence: &'static [u8],
	receiver_hint: Vec<u8>,
	call_name: Vec<u8>,
	call_arity: Option<usize>,
}

struct CallableTarget {
	moniker: Moniker,
	confidence: &'static [u8],
}

struct PyCallResolver<'a, 'src> {
	discover: &'a PyDiscover<'src>,
	scope: &'a Moniker,
	graph: &'a mut SdkBuilder,
}

impl<'a, 'src> PyCallResolver<'a, 'src> {
	fn new(discover: &'a PyDiscover<'src>, scope: &'a Moniker, graph: &'a mut SdkBuilder) -> Self {
		Self {
			discover,
			scope,
			graph,
		}
	}

	fn emit_call(&mut self, node: Node<'_>) {
		let pos = node_position(node);
		let Some(callee) = node.child_by_field_name("function") else {
			self.discover.recurse_subtree(node, self.scope, self.graph);
			return;
		};

		match callee.kind() {
			"identifier" => self.emit_identifier_call(node, callee, pos),
			"attribute" => self.emit_attribute_call(node, callee, pos),
			_ => self
				.discover
				.recurse_subtree(callee, self.scope, self.graph),
		}

		if let Some(args) = node.child_by_field_name("arguments") {
			self.discover.recurse_subtree(args, self.scope, self.graph);
		}
	}

	fn emit_identifier_call(&mut self, call: Node<'_>, callee: Node<'_>, pos: Position) {
		let name = node_slice(callee, self.discover.source_bytes);
		if name.is_empty() {
			return;
		}
		let Some(resolution) = self.resolve_identifier_call(call, name) else {
			return;
		};
		self.emit_resolution(resolution, pos);
	}

	fn resolve_identifier_call(&self, call: Node<'_>, name: &[u8]) -> Option<CallResolution> {
		let arity = call_argument_count(call);
		if let Some(target) =
			self.discover
				.imports
				.target_for(self.scope, &self.discover.module, name)
		{
			let confidence = self
				.discover
				.imports
				.confidence_for(self.scope, &self.discover.module, name)
				.unwrap_or(kinds::CONF_IMPORTED);
			return Some(CallResolution {
				target,
				kind: kinds::CALLS,
				confidence,
				receiver_hint: if self.discover.imports.is_runtime_binding(
					self.scope,
					&self.discover.module,
					name,
				) {
					kinds::HINT_PY_CONDITIONAL_IMPORT.to_vec()
				} else {
					Vec::new()
				},
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		if self.discover.locals.is_name(name) {
			if let Some(target) = lookup_function_local_type(self.discover, name) {
				return Some(CallResolution {
					target,
					kind: kinds::INSTANTIATES,
					confidence: kinds::CONF_RESOLVED,
					receiver_hint: Vec::new(),
					call_name: name.to_vec(),
					call_arity: Some(arity),
				});
			}
			let confidence = name_confidence(self.discover, name)?;
			return Some(CallResolution {
				target: extend_segment(self.scope, kinds::LOCAL, name),
				kind: kinds::CALLS,
				confidence,
				receiver_hint: Vec::new(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		if let Some(target) = lookup_discovered_type(self.discover, self.scope, name) {
			return Some(CallResolution {
				target,
				kind: kinds::INSTANTIATES,
				confidence: kinds::CONF_RESOLVED,
				receiver_hint: Vec::new(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		if is_python_builtin(name) {
			return Some(CallResolution {
				target: builtin_external_target(&self.discover.module, name),
				kind: kinds::CALLS,
				confidence: kinds::CONF_EXTERNAL,
				receiver_hint: Vec::new(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		let confidence = name_confidence(self.discover, name)?;
		Some(CallResolution {
			target: lookup_callable(self.discover, self.scope, name),
			kind: kinds::CALLS,
			confidence,
			receiver_hint: Vec::new(),
			call_name: name.to_vec(),
			call_arity: Some(arity),
		})
	}

	fn emit_attribute_call(&mut self, call: Node<'_>, callee: Node<'_>, pos: Position) {
		let name = last_attribute(callee, self.discover.source_bytes);
		if !name.is_empty()
			&& let Some(resolution) = self.resolve_attribute_call(call, callee, name.as_bytes())
		{
			self.emit_resolution(resolution, pos);
		}
		if let Some(obj) = callee.child_by_field_name("object") {
			self.discover.recurse_subtree(obj, self.scope, self.graph);
		}
	}

	fn resolve_attribute_call(
		&self,
		call: Node<'_>,
		callee: Node<'_>,
		name: &[u8],
	) -> Option<CallResolution> {
		let arity = call_argument_count(call);
		let receiver = callee.child_by_field_name("object");
		let hint = receiver
			.map(|r| receiver_hint(r, self.discover.source_bytes))
			.unwrap_or(b"");
		if let Some(resolution) = self.imported_member_call(receiver, name, hint, arity) {
			return Some(resolution);
		}
		if let Some(receiver) = receiver
			&& let Some(target) =
				lookup_method_on_typed_receiver(self.discover, self.scope, receiver, name)
		{
			return Some(CallResolution {
				target: target.moniker,
				kind: kinds::METHOD_CALL,
				confidence: target.confidence,
				receiver_hint: hint.to_vec(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		if matches!(hint, b"self" | b"cls") {
			return Some(self.self_or_class_member_call(name, hint, arity));
		}
		Some(CallResolution {
			target: extend_segment(&self.discover.module, kinds::METHOD, name),
			kind: kinds::METHOD_CALL,
			confidence: kinds::CONF_UNRESOLVED,
			receiver_hint: hint.to_vec(),
			call_name: name.to_vec(),
			call_arity: Some(arity),
		})
	}

	fn imported_member_call(
		&self,
		receiver: Option<Node<'_>>,
		name: &[u8],
		hint: &[u8],
		arity: usize,
	) -> Option<CallResolution> {
		let receiver = receiver?;
		if receiver.kind() != "identifier" {
			return None;
		}
		let receiver_name = node_slice(receiver, self.discover.source_bytes);
		let import_target =
			self.discover
				.imports
				.target_for(self.scope, &self.discover.module, receiver_name)?;
		let receiver_hint = if self.discover.imports.is_runtime_binding(
			self.scope,
			&self.discover.module,
			receiver_name,
		) {
			kinds::HINT_PY_CONDITIONAL_IMPORT
		} else {
			hint
		};
		Some(CallResolution {
			target: extend_segment(&import_target, kinds::FUNCTION, name),
			kind: kinds::CALLS,
			confidence: self
				.discover
				.imports
				.confidence_for(self.scope, &self.discover.module, receiver_name)
				.unwrap_or(kinds::CONF_NAME_MATCH),
			receiver_hint: receiver_hint.to_vec(),
			call_name: name.to_vec(),
			call_arity: Some(arity),
		})
	}

	fn self_or_class_member_call(&self, name: &[u8], hint: &[u8], arity: usize) -> CallResolution {
		if let Some(target) = lookup_self_named_attr_type(self.discover, self.scope, name) {
			return CallResolution {
				target,
				kind: kinds::CALLS,
				confidence: kinds::CONF_RESOLVED,
				receiver_hint: hint.to_vec(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			};
		}
		let target = lookup_callable_in_scope(self.discover, self.scope, name, kinds::METHOD)
			.unwrap_or_else(|| extend_segment(&self.discover.module, kinds::METHOD, name));
		CallResolution {
			target,
			kind: kinds::METHOD_CALL,
			confidence: kinds::CONF_RESOLVED,
			receiver_hint: hint.to_vec(),
			call_name: name.to_vec(),
			call_arity: Some(arity),
		}
	}

	fn emit_resolution(&mut self, resolution: CallResolution, pos: Position) {
		let attrs = RefAttrs {
			receiver_hint: &resolution.receiver_hint,
			confidence: resolution.confidence,
			call_name: &resolution.call_name,
			call_arity: resolution.call_arity,
			..RefAttrs::default()
		};
		let _ = self.graph.add_ref_attrs(
			self.scope,
			resolution.target,
			resolution.kind,
			Some(pos),
			&attrs,
		);
	}
}

struct PyTypeRefs<'a, 'src> {
	discover: &'a PyDiscover<'src>,
	scope: &'a Moniker,
}

impl<'a, 'src> PyTypeRefs<'a, 'src> {
	fn new(discover: &'a PyDiscover<'src>, scope: &'a Moniker) -> Self {
		Self { discover, scope }
	}

	fn collect(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		if type_ref_container(node.kind()) {
			self.collect_children(node, out);
			return;
		}
		if let Some(ref_spec) = self.ref_spec_for_type_node(node) {
			out.push(ref_spec);
		}
	}

	fn emit(&self, node: Node<'_>, graph: &mut SdkBuilder) {
		if node.kind() == "subscript" {
			self.emit_subscript(node, graph);
			return;
		}
		if type_ref_container(node.kind()) {
			self.emit_children(node, graph);
			return;
		}
		if let Some(ref_spec) = self.ref_spec_for_type_node(node) {
			let attrs = RefAttrs {
				confidence: ref_spec.confidence,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				self.scope,
				ref_spec.target,
				kinds::USES_TYPE,
				Some(ref_spec.position),
				&attrs,
			);
		}
	}

	fn collect_children(&self, node: Node<'_>, out: &mut Vec<RefSpec>) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			self.collect(child, out);
		}
	}

	fn emit_children(&self, node: Node<'_>, graph: &mut SdkBuilder) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			self.emit(child, graph);
		}
	}

	fn emit_subscript(&self, node: Node<'_>, graph: &mut SdkBuilder) {
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			if child.kind() != "slice" {
				self.emit(child, graph);
			}
		}
	}

	fn ref_spec_for_type_node(&self, node: Node<'_>) -> Option<RefSpec> {
		if node.kind() == "attribute"
			&& qualified_nonconcrete_typing(self.discover, node, self.scope)
		{
			return None;
		}
		let (name, position) = match node.kind() {
			"identifier" => (
				node_slice(node, self.discover.source_bytes).to_vec(),
				node_position(node),
			),
			"attribute" => (
				last_attribute(node, self.discover.source_bytes)
					.as_bytes()
					.to_vec(),
				node_position(node),
			),
			_ => return None,
		};
		if should_skip_type_name(&name) {
			return None;
		}
		let (target, confidence) =
			resolve_type_target(self.discover, self.scope, &name, kinds::CLASS);
		Some(RefSpec {
			kind: kinds::USES_TYPE,
			target,
			confidence,
			position,
			receiver_hint: b"",
			alias: b"",
		})
	}
}

struct PyImportEmitter<'a, 'src> {
	discover: &'a PyDiscover<'src>,
	scope: &'a Moniker,
	graph: &'a mut SdkBuilder,
}

impl<'a, 'src> PyImportEmitter<'a, 'src> {
	fn new(discover: &'a PyDiscover<'src>, scope: &'a Moniker, graph: &'a mut SdkBuilder) -> Self {
		Self {
			discover,
			scope,
			graph,
		}
	}

	fn emit_import_statement(&mut self, node: Node<'_>) {
		let pos = node_position(node);
		let conditional = is_conditionally_executed(node);
		let mut cursor = node.walk();
		let targets: Vec<_> = node
			.children(&mut cursor)
			.filter(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
			.collect();
		for target in targets {
			self.emit_import_module(target, pos, conditional);
		}
	}

	fn emit_import_from_statement(&mut self, node: Node<'_>) {
		let pos = node_position(node);
		let Some(module_node) = node.child_by_field_name("module_name") else {
			return;
		};
		let Some(module_import) = ModuleImport::from_node(module_node, self.discover.source_bytes)
		else {
			return;
		};
		let confidence = module_import.confidence();
		let module_target = module_import.module_target(&self.discover.module);
		let receiver_hint = if is_conditionally_executed(node) {
			kinds::HINT_PY_CONDITIONAL_IMPORT
		} else {
			b""
		};

		if has_wildcard_import(node) {
			self.emit_ref(
				module_target,
				kinds::IMPORTS_MODULE,
				confidence,
				b"*",
				receiver_hint,
				pos,
			);
			return;
		}

		for (name, alias) in collect_from_import_names(node, self.discover.source_bytes) {
			self.emit_imported_symbol(&module_import, name, alias, confidence, receiver_hint, pos);
		}
	}

	fn emit_import_module(&mut self, node: Node<'_>, pos: Position, conditional: bool) {
		let Some((path_node, alias)) =
			import_module_path_and_alias(node, self.discover.source_bytes)
		else {
			return;
		};
		let pieces = dotted_pieces(path_node, self.discover.source_bytes);
		if pieces.is_empty() {
			return;
		}
		let confidence = external_or_imported(&pieces);
		let bind = if !alias.is_empty() { alias } else { pieces[0] };
		self.discover
			.imports
			.bind(self.scope, bind.as_bytes(), confidence, conditional);

		let target = build_module_target(&self.discover.module, &pieces, 0, confidence);
		self.discover
			.imports
			.bind_target(self.scope, bind.as_bytes(), &target);
		self.emit_ref(
			target,
			kinds::IMPORTS_MODULE,
			confidence,
			alias.as_bytes(),
			if conditional {
				kinds::HINT_PY_CONDITIONAL_IMPORT
			} else {
				b""
			},
			pos,
		);
	}

	fn emit_imported_symbol(
		&mut self,
		module_import: &ModuleImport<'_>,
		name: &str,
		alias: &str,
		confidence: &'static [u8],
		receiver_hint: &'static [u8],
		pos: Position,
	) {
		let bind = if !alias.is_empty() { alias } else { name };
		self.discover.imports.bind(
			self.scope,
			bind.as_bytes(),
			confidence,
			receiver_hint == kinds::HINT_PY_CONDITIONAL_IMPORT,
		);
		let target = build_imported_symbol_target(
			&self.discover.module,
			&module_import.pieces,
			module_import.leading_dots,
			name.as_bytes(),
			confidence,
		);
		self.discover
			.imports
			.bind_target(self.scope, bind.as_bytes(), &target);
		self.emit_ref(
			target,
			kinds::IMPORTS_SYMBOL,
			confidence,
			alias.as_bytes(),
			receiver_hint,
			pos,
		);
	}

	fn emit_ref(
		&mut self,
		target: Moniker,
		kind: &'static [u8],
		confidence: &'static [u8],
		alias: &[u8],
		receiver_hint: &[u8],
		pos: Position,
	) {
		let attrs = RefAttrs {
			confidence,
			alias,
			receiver_hint,
			..RefAttrs::default()
		};
		let _ = self
			.graph
			.add_ref_attrs(self.scope, target, kind, Some(pos), &attrs);
	}
}

fn is_conditionally_executed(node: Node<'_>) -> bool {
	let mut parent = node.parent();
	while let Some(current) = parent {
		match current.kind() {
			"module" | "function_definition" | "class_definition" => return false,
			"if_statement" | "try_statement" | "for_statement" | "while_statement"
			| "with_statement" | "match_statement" => return true,
			_ => parent = current.parent(),
		}
	}
	false
}

struct ModuleImport<'src> {
	pieces: Vec<&'src str>,
	leading_dots: usize,
}

impl<'src> ModuleImport<'src> {
	fn from_node(node: Node<'_>, source: &'src [u8]) -> Option<Self> {
		match node.kind() {
			"relative_import" => {
				let (pieces, leading_dots) = relative_import_pieces(node, source);
				Some(Self {
					pieces,
					leading_dots,
				})
			}
			"dotted_name" => Some(Self {
				pieces: dotted_pieces(node, source),
				leading_dots: 0,
			}),
			_ => None,
		}
	}

	fn confidence(&self) -> &'static [u8] {
		if self.leading_dots > 0 {
			kinds::CONF_IMPORTED
		} else {
			external_or_imported(&self.pieces)
		}
	}

	fn module_target(&self, module: &Moniker) -> Moniker {
		build_module_target(module, &self.pieces, self.leading_dots, self.confidence())
	}
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
		let (call_name, call_arity) = def_call_metadata(kind, &name, attrs);
		self.defs.push(DiscoveredDef {
			moniker,
			parent: parent.clone(),
			namespace: namespace_for_kind(kind),
			name,
			kind,
			visibility: static_visibility(attrs.visibility),
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
			confidence: static_confidence(attrs.confidence),
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

	fn finish(self) -> DiscoveredPythonFile {
		DiscoveredPythonFile {
			root: self.root,
			defs: self.defs,
			refs: self.refs,
		}
	}
}

struct PyWalker<'a> {
	discover: &'a PyDiscover<'a>,
	source: &'a [u8],
}

struct PendingAnnotation {
	kind: &'static [u8],
	start_byte: u32,
	end_byte: u32,
	end_row: usize,
}

impl<'a> PyWalker<'a> {
	fn new(discover: &'a PyDiscover<'a>, source: &'a [u8]) -> Self {
		Self { discover, source }
	}

	fn walk(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let mut cursor = node.walk();
		let mut pending: Option<PendingAnnotation> = None;
		for child in node.children(&mut cursor) {
			match classify_node(self.discover, child, scope, self.source, graph) {
				NodeShape::Annotation { kind } => {
					self.extend_or_flush(&mut pending, kind, child, scope, graph)
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
		match classify_node(self.discover, node, scope, self.source, graph) {
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
			moniker,
			kind,
			visibility,
			signature,
			call_name,
			call_arity,
			body,
			position,
			annotated_by,
		} = sym;
		let attrs = DefAttrs {
			visibility,
			signature: signature.as_deref().unwrap_or_default(),
			call_name: &call_name,
			call_arity,
			..DefAttrs::default()
		};
		let parent = moniker
			.parent()
			.filter(|parent| parent != scope && graph.contains(parent))
			.unwrap_or_else(|| scope.clone());
		if graph
			.add_def_attrs(moniker.clone(), kind, &parent, Some(position), &attrs)
			.is_err()
		{
			return;
		}
		for reference in annotated_by {
			let attrs = RefAttrs {
				confidence: reference.confidence,
				receiver_hint: reference.receiver_hint,
				alias: reference.alias,
				..RefAttrs::default()
			};
			let _ = graph.add_ref_attrs(
				&moniker,
				reference.target,
				reference.kind,
				Some(reference.position),
				&attrs,
			);
		}
		before_symbol_body(self.discover, node, kind, &moniker, self.source, graph);
		if let Some(body_node) = body {
			self.walk(body_node, &moniker, graph);
		}
		after_symbol_body(self.discover, kind, &moniker, graph);
		on_symbol_emitted(self.discover, node, kind, &moniker, graph);
	}

	fn emit_annotation_range(
		&self,
		kind: &'static [u8],
		start_byte: u32,
		end_byte: u32,
		scope: &Moniker,
		graph: &mut SdkBuilder,
	) {
		let moniker = crate::lang::callable::extend_segment_u32(scope, kind, start_byte);
		let _ = graph.add_def(moniker, kind, scope, Some((start_byte, end_byte)));
	}
}

impl<'a> PyDiscover<'a> {
	pub(super) fn run(
		module: Moniker,
		source_bytes: &'a [u8],
		deep: bool,
		root: Node<'_>,
	) -> DiscoveredPythonFile {
		let mut type_table: TypeTable = HashMap::new();
		collect_type_table(root, source_bytes, &module, false, &mut type_table);
		let mut callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry> = HashMap::new();
		collect_callable_table(root, source_bytes, &module, false, &mut callable_table);
		let mut instance_attr_types: HashMap<(Moniker, Vec<u8>), Moniker> = HashMap::new();
		collect_instance_attr_types(
			root,
			source_bytes,
			&module,
			false,
			&type_table,
			&mut instance_attr_types,
		);
		let discover = Self {
			module: module.clone(),
			source_bytes,
			deep,
			imports: PyImportBindings::new(),
			locals: PyLocalScopes::new(),
			declared_instance_attr_types: instance_attr_types.keys().cloned().collect(),
			instance_attr_types: RefCell::new(instance_attr_types),
			ambiguous_instance_attr_types: RefCell::new(HashSet::new()),
			type_table,
			callable_table,
		};
		let mut builder = SdkBuilder::new(module.clone());
		PyWalker::new(&discover, source_bytes).walk(root, &module, &mut builder);
		if let Some(docstring) = first_docstring(root) {
			emit_docstring_def(docstring, &module, &mut builder);
		}
		builder.finish()
	}
}

fn classify_node<'src>(
	discover: &PyDiscover<'_>,
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
			PyImportEmitter::new(discover, scope, graph).emit_import_statement(node);
			NodeShape::Skip
		}
		"import_from_statement" => {
			PyImportEmitter::new(discover, scope, graph).emit_import_from_statement(node);
			NodeShape::Skip
		}
		"future_import_statement" => NodeShape::Skip,
		"decorated_definition" => classify_decorated(discover, node, scope, source, graph),
		"class_definition" => classify_class(discover, node, scope, source, graph, &[]),
		"type_alias_statement" => classify_type_alias(discover, node, scope),
		"function_definition" => classify_function(discover, node, scope, source, graph, &[]),
		"call" => {
			PyCallResolver::new(discover, scope, graph).emit_call(node);
			NodeShape::Skip
		}
		"assignment" => {
			if let Some(symbol) = classify_type_alias_assignment(discover, node, scope, graph) {
				NodeShape::Symbol(symbol)
			} else {
				handle_assignment(discover, node, scope, graph);
				NodeShape::Skip
			}
		}
		"augmented_assignment" if is_module_all_assignment(discover, node, scope) => {
			handle_all_augmented_assignment(discover, node, scope, graph);
			NodeShape::Skip
		}
		"keyword_argument" => {
			handle_keyword_argument(discover, node, scope, graph);
			NodeShape::Skip
		}
		"attribute" => {
			handle_attribute(discover, node, scope, graph);
			NodeShape::Skip
		}
		"identifier" => {
			handle_identifier(discover, node, scope, graph);
			NodeShape::Skip
		}
		"lambda" => {
			handle_lambda(discover, node, scope, graph);
			NodeShape::Skip
		}
		"for_statement" => {
			handle_for(discover, node, scope, graph);
			NodeShape::Skip
		}
		"for_in_clause" => {
			handle_for(discover, node, scope, graph);
			NodeShape::Skip
		}
		"with_statement" => {
			handle_with(discover, node, scope, graph);
			NodeShape::Skip
		}
		"except_clause" => {
			handle_except(discover, node, scope, graph);
			NodeShape::Skip
		}
		_ => NodeShape::Recurse,
	}
}

fn before_symbol_body(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	kind: &[u8],
	moniker: &Moniker,
	source: &[u8],
	graph: &mut SdkBuilder,
) {
	if kind != kinds::FUNCTION && kind != kinds::ASYNC_FUNCTION && kind != kinds::METHOD {
		return;
	}
	let node = effective_definition_node(node);
	if let Some(rt) = node.child_by_field_name("return_type") {
		PyTypeRefs::new(discover, moniker).emit(rt, graph);
		if !is_async_function(node) && !callable_return_is_ambiguous(discover, node, moniker) {
			emit_callable_return_type(discover, rt, moniker, graph);
		}
	}
	if let Some(params) = node.child_by_field_name("parameters") {
		emit_param_defs_and_types(discover, params, moniker, source, graph);
	}
}

fn callable_return_is_ambiguous(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	moniker: &Moniker,
) -> bool {
	let Some(name_node) = node.child_by_field_name("name") else {
		return true;
	};
	let Some(parent) = moniker.parent() else {
		return true;
	};
	discover
		.callable_table
		.get(&(
			parent,
			node_slice(name_node, discover.source_bytes).to_vec(),
		))
		.is_none_or(|entry| entry.return_type_ambiguous)
}

fn emit_callable_return_type(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	callable: &Moniker,
	graph: &mut SdkBuilder,
) {
	let types = infer_local_type_set(discover, node, callable);
	for target in types.static_types() {
		let attrs = RefAttrs {
			confidence: if is_external_shaped(target) {
				kinds::CONF_EXTERNAL
			} else {
				kinds::CONF_RESOLVED
			},
			receiver_hint: if types.is_dynamic() {
				b"python_open_type_set"
			} else {
				b""
			},
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(
			callable,
			target.clone(),
			kinds::RETURNS_TYPE,
			Some(node_position(node)),
			&attrs,
		);
	}
}

fn return_type_names(node: Node<'_>, source: &[u8]) -> (Vec<Vec<u8>>, bool) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, source);
			if name.is_empty() || name == b"None" {
				(Vec::new(), true)
			} else {
				(vec![name.to_vec()], false)
			}
		}
		"none" => (Vec::new(), true),
		"type" | "parenthesized_expression" => {
			let mut cursor = node.walk();
			let children = node.named_children(&mut cursor).collect::<Vec<_>>();
			if let [child] = children.as_slice() {
				return_type_names(*child, source)
			} else {
				(Vec::new(), true)
			}
		}
		"union_type" | "binary_operator" => {
			let mut names = Vec::new();
			let mut dynamic = false;
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				let (child_names, child_dynamic) = return_type_names(child, source);
				for name in child_names {
					if !names.contains(&name) {
						names.push(name);
					}
				}
				dynamic |= child_dynamic;
			}
			(names, dynamic)
		}
		_ => (Vec::new(), true),
	}
}

fn resolve_return_type_name(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	if let Some(target) = lookup_discovered_type(discover, scope, name) {
		return Some((target, kinds::CONF_RESOLVED));
	}
	if imported_nonconcrete_typing(discover, scope, name) {
		return None;
	}
	if let Some(target) = discover.imports.target_for(scope, &discover.module, name) {
		let confidence = discover
			.imports
			.confidence_for(scope, &discover.module, name)
			.unwrap_or(kinds::CONF_IMPORTED);
		return Some((target, confidence));
	}
	if should_skip_type_name(name) || is_typing_container(name) {
		return is_inferable_builtin_type(name).then(|| {
			(
				builtin_external_target(&discover.module, name),
				kinds::CONF_EXTERNAL,
			)
		});
	}
	None
}

// Decorated symbols classify from the outer `decorated_definition` node;
// parameters, return type and body live on the inner definition.
fn effective_definition_node(node: Node<'_>) -> Node<'_> {
	if node.kind() == "decorated_definition"
		&& let Some(def) = decorated_definition_node(node)
	{
		return def;
	}
	node
}

fn after_symbol_body(
	discover: &PyDiscover<'_>,
	kind: &[u8],
	callable: &Moniker,
	graph: &mut SdkBuilder,
) {
	if kind == kinds::FUNCTION || kind == kinds::ASYNC_FUNCTION || kind == kinds::METHOD {
		for (name, types) in discover.locals.current_type_sets() {
			emit_local_type_facts(callable, &name, None, &types, graph);
		}
		discover.locals.pop();
	}
}

fn on_symbol_emitted(
	_discover: &PyDiscover<'_>,
	node: Node<'_>,
	sym_kind: &[u8],
	sym_moniker: &Moniker,
	graph: &mut SdkBuilder,
) {
	if sym_kind != kinds::FUNCTION
		&& sym_kind != kinds::ASYNC_FUNCTION
		&& sym_kind != kinds::METHOD
		&& sym_kind != kinds::CLASS
	{
		return;
	}
	let Some(body) = effective_definition_node(node).child_by_field_name("body") else {
		return;
	};
	if let Some(docstring) = first_docstring(body) {
		emit_docstring_def(docstring, sym_moniker, graph);
	}
}

fn classify_decorated<'src>(
	discover: &PyDiscover<'_>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
	graph: &mut SdkBuilder,
) -> NodeShape<'src> {
	let mut decorators: Vec<Node<'src>> = Vec::new();
	let mut def_node: Option<Node<'src>> = None;
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		match c.kind() {
			"decorator" => decorators.push(c),
			"class_definition" | "function_definition" => def_node = Some(c),
			_ => {}
		}
	}
	let Some(def) = def_node else {
		return NodeShape::Recurse;
	};
	match def.kind() {
		"class_definition" => classify_class(discover, def, scope, source, graph, &decorators),
		"function_definition" => {
			classify_function(discover, def, scope, source, graph, &decorators)
		}
		_ => NodeShape::Recurse,
	}
}

fn classify_class<'src>(
	discover: &PyDiscover<'_>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
	_graph: &mut SdkBuilder,
	decorators: &[Node<'src>],
) -> NodeShape<'src> {
	let Some(name_node) = node.child_by_field_name("name") else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, source);
	let moniker = extend_segment(scope, kinds::CLASS, name);

	let mut annotated_by: Vec<RefSpec> = Vec::new();
	if let Some(supers) = node.child_by_field_name("superclasses") {
		collect_base_class_refs(discover, supers, scope, &mut annotated_by);
	}
	for d in decorators {
		collect_decorator_refs(discover, *d, scope, &mut annotated_by);
	}

	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::CLASS,
		visibility: visibility_from_name(name),
		signature: None,
		call_name: Vec::new(),
		call_arity: None,
		body: node.child_by_field_name("body"),
		position: node_position(node),
		annotated_by,
	})
}

fn classify_type_alias<'src>(
	discover: &PyDiscover<'_>,
	node: Node<'src>,
	scope: &Moniker,
) -> NodeShape<'src> {
	let Some(left) = node.child_by_field_name("left") else {
		return NodeShape::Recurse;
	};
	let Some(name_node) = type_alias_name_node(left) else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, discover.source_bytes);
	if name.is_empty() {
		return NodeShape::Recurse;
	}
	let moniker = extend_segment(scope, kinds::TYPE, name);
	let mut annotated_by = Vec::new();
	if let Some(right) = node.child_by_field_name("right") {
		PyTypeRefs::new(discover, scope).collect(right, &mut annotated_by);
	}
	NodeShape::Symbol(Symbol {
		moniker,
		kind: kinds::TYPE,
		visibility: visibility_from_name(name),
		signature: None,
		call_name: Vec::new(),
		call_arity: None,
		body: None,
		position: node_position(node),
		annotated_by,
	})
}

fn classify_type_alias_assignment<'src>(
	discover: &PyDiscover<'_>,
	node: Node<'src>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) -> Option<Symbol<'src>> {
	let type_node = node.child_by_field_name("type")?;
	if !type_annotation_has_name(type_node, discover.source_bytes, b"TypeAlias") {
		return None;
	}
	let left = node.child_by_field_name("left")?;
	let name_node = assignment_alias_name_node(left)?;
	let name = node_slice(name_node, discover.source_bytes);
	if name.is_empty() {
		return None;
	}
	PyTypeRefs::new(discover, scope).emit(type_node, graph);
	let moniker = extend_segment(scope, kinds::TYPE, name);
	let mut annotated_by = Vec::new();
	if let Some(right) = node.child_by_field_name("right") {
		PyTypeRefs::new(discover, scope).collect(right, &mut annotated_by);
	}
	Some(Symbol {
		moniker,
		kind: kinds::TYPE,
		visibility: visibility_from_name(name),
		signature: None,
		call_name: Vec::new(),
		call_arity: None,
		body: None,
		position: node_position(node),
		annotated_by,
	})
}

fn classify_function<'src>(
	discover: &PyDiscover<'_>,
	node: Node<'src>,
	scope: &Moniker,
	source: &'src [u8],
	graph: &mut SdkBuilder,
	decorators: &[Node<'src>],
) -> NodeShape<'src> {
	let Some(name_node) = node.child_by_field_name("name") else {
		return NodeShape::Recurse;
	};
	let name = node_slice(name_node, source);
	let is_method = is_class_scope(scope);
	let is_async = is_async_function(node);
	let kind = if is_method {
		kinds::METHOD
	} else if is_async {
		kinds::ASYNC_FUNCTION
	} else {
		kinds::FUNCTION
	};

	let slots = collect_param_slots(node, source, is_method);
	let signature =
		join_bytes_with_comma(&slots.iter().map(slot_signature_bytes).collect::<Vec<_>>());
	let moniker = extend_callable_slots(scope, kind, name, &slots);

	let mut annotated_by: Vec<RefSpec> = Vec::new();
	for d in decorators {
		collect_decorator_refs(discover, *d, scope, &mut annotated_by);
	}

	discover.locals.push();
	if let Some(params) = node.child_by_field_name("parameters") {
		record_param_locals(discover, params, source, &moniker);
	}
	if let Some(body) = node.child_by_field_name("body") {
		precollect_callable_local_names(discover, body, &moniker);
	}
	let _ = graph;

	NodeShape::Symbol(Symbol {
		moniker,
		kind,
		visibility: visibility_from_name(name),
		signature: Some(signature),
		call_name: name.to_vec(),
		call_arity: Some(slots.len()),
		body: node.child_by_field_name("body"),
		position: node_position(node),
		annotated_by,
	})
}

fn precollect_callable_local_names(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker) {
	match node.kind() {
		"function_definition" => {
			if let Some(name) = node.child_by_field_name("name") {
				discover
					.locals
					.record_name(node_slice(name, discover.source_bytes));
			}
			return;
		}
		"class_definition" => {
			if let Some(name_node) = node.child_by_field_name("name") {
				let name = node_slice(name_node, discover.source_bytes);
				discover.locals.record_name(name);
				discover
					.locals
					.record_type_binding(name, extend_segment(scope, kinds::CLASS, name));
			}
			return;
		}
		"import_statement" => {
			let mut cursor = node.walk();
			for child in node
				.children(&mut cursor)
				.filter(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
			{
				let Some((path, alias)) =
					import_module_path_and_alias(child, discover.source_bytes)
				else {
					continue;
				};
				let pieces = dotted_pieces(path, discover.source_bytes);
				if let Some(bind) = (!alias.is_empty())
					.then_some(alias)
					.or_else(|| pieces.first().copied())
				{
					discover.locals.record_name(bind.as_bytes());
				}
			}
			return;
		}
		"import_from_statement" => {
			for (name, alias) in collect_from_import_names(node, discover.source_bytes) {
				let bind = if alias.is_empty() { name } else { alias };
				discover.locals.record_name(bind.as_bytes());
			}
			return;
		}
		"list_comprehension"
		| "set_comprehension"
		| "dictionary_comprehension"
		| "generator_expression" => {
			precollect_comprehension_walrus(discover, node);
			return;
		}
		"lambda" => return,
		"assignment" | "augmented_assignment" => {
			if let Some(left) = node.child_by_field_name("left") {
				record_local_pattern(discover, left);
			}
		}
		"named_expression" => {
			if let Some(name) = node.child_by_field_name("name") {
				record_local_pattern(discover, name);
			}
		}
		"as_pattern" | "except_clause" => {
			if let Some(alias) = node.child_by_field_name("alias") {
				record_local_pattern(discover, alias);
			}
		}
		"case_pattern" => {
			let mut cursor = node.walk();
			let children = node.named_children(&mut cursor).collect::<Vec<_>>();
			if let [capture] = children.as_slice()
				&& capture.kind() == "dotted_name"
			{
				let name = node_slice(*capture, discover.source_bytes);
				if !name.contains(&b'.') && name != b"_" {
					discover.locals.record_name(name);
				}
			}
		}
		"for_statement" | "for_in_clause" => {
			if let Some(left) = node.child_by_field_name("left") {
				record_local_pattern(discover, left);
			}
		}
		_ => {}
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		precollect_callable_local_names(discover, child, scope);
	}
}

fn precollect_comprehension_walrus(discover: &PyDiscover<'_>, node: Node<'_>) {
	if node.kind() == "lambda" || node.kind() == "function_definition" {
		return;
	}
	if node.kind() == "named_expression"
		&& let Some(name) = node.child_by_field_name("name")
	{
		record_local_pattern(discover, name);
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		precollect_comprehension_walrus(discover, child);
	}
}

fn handle_assignment(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let inferred_types = node
		.child_by_field_name("type")
		.map(|typed| infer_local_type_set(discover, typed, scope));
	if let Some(typed) = node.child_by_field_name("type") {
		PyTypeRefs::new(discover, scope).emit(typed, graph);
	}
	let inside_callable = is_callable_scope(scope, &discover.module);
	if inside_callable && let Some(left) = node.child_by_field_name("left") {
		record_local_pattern(discover, left);
		record_assignment_element_types(discover, left, node.child_by_field_name("right"), scope);
		record_assignment_type(
			discover,
			scope,
			left,
			node.child_by_field_name("right"),
			inferred_types.clone(),
		);
		if discover.deep {
			emit_local_pattern(discover, left, scope, graph);
		}
	}
	if !inside_callable && let Some(left) = node.child_by_field_name("left") {
		emit_binding_pattern(discover, left, scope, graph);
		emit_binding_type(
			discover,
			left,
			node.child_by_field_name("right"),
			inferred_types.and_then(|types| types.unique()),
			scope,
			graph,
		);
		emit_static_all_exports(
			discover,
			left,
			node.child_by_field_name("right"),
			scope,
			graph,
			kinds::HINT_PY_ALL_REPLACE,
		);
	}
	if let Some(right) = node.child_by_field_name("right") {
		discover.recurse_subtree(right, scope, graph);
	}
}

fn emit_static_all_exports(
	discover: &PyDiscover<'_>,
	left: Node<'_>,
	right: Option<Node<'_>>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
	directive: &'static [u8],
) {
	if scope != &discover.module || node_slice(left, discover.source_bytes) != b"__all__" {
		return;
	}
	let conditional = is_conditionally_executed(left);
	let items = (!conditional)
		.then(|| right.and_then(|right| static_string_collection(right, discover.source_bytes)))
		.flatten();
	let marker_target = MonikerBuilder::from_view(scope.as_view())
		.segment(kinds::PATH, b"__all__")
		.build();
	let marker_attrs = RefAttrs {
		confidence: kinds::CONF_RESOLVED,
		receiver_hint: if items.is_some() {
			directive
		} else {
			kinds::HINT_PY_ALL_DYNAMIC
		},
		..RefAttrs::default()
	};
	let _ = graph.add_ref_attrs(
		scope,
		marker_target,
		kinds::REEXPORTS,
		Some(node_position(left)),
		&marker_attrs,
	);
	let Some(items) = items else { return };
	for (name, position) in items {
		let target = MonikerBuilder::from_view(scope.as_view())
			.segment(kinds::PATH, &name)
			.build();
		let attrs = RefAttrs {
			confidence: kinds::CONF_RESOLVED,
			alias: &name,
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target, kinds::REEXPORTS, Some(position), &attrs);
	}
}

fn is_module_all_assignment(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker) -> bool {
	scope == &discover.module
		&& node
			.child_by_field_name("left")
			.is_some_and(|left| node_slice(left, discover.source_bytes) == b"__all__")
}

fn handle_all_augmented_assignment(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let Some(left) = node.child_by_field_name("left") else {
		return;
	};
	let right = node.child_by_field_name("right");
	emit_static_all_exports(
		discover,
		left,
		right,
		scope,
		graph,
		kinds::HINT_PY_ALL_EXTEND,
	);
	if let Some(right) = right {
		discover.recurse_subtree(right, scope, graph);
	}
}

fn static_string_collection(node: Node<'_>, source: &[u8]) -> Option<Vec<(Vec<u8>, Position)>> {
	if !matches!(node.kind(), "list" | "tuple" | "set") {
		return None;
	}
	let mut items = Vec::new();
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if child.kind() == "comment" {
			continue;
		}
		if child.kind() != "string" {
			return None;
		}
		items.push((static_export_name(child, source)?, node_position(child)));
	}
	Some(items)
}

fn static_export_name(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	let text = node_slice(node, source);
	let quote = text.iter().position(|byte| matches!(byte, b'\'' | b'"'))?;
	let delimiter = text[quote];
	if text.get(quote + 1) == Some(&delimiter) || text.last() != Some(&delimiter) {
		return None;
	}
	let name = text.get(quote + 1..text.len().checked_sub(1)?)?;
	if name.is_empty()
		|| !name
			.iter()
			.all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
	{
		return None;
	}
	Some(name.to_vec())
}

fn handle_for(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
	if is_callable_scope(scope, &discover.module)
		&& let Some(left) = node.child_by_field_name("left")
	{
		record_local_pattern(discover, left);
		if left.kind() == "identifier" {
			let name = node_slice(left, discover.source_bytes);
			if let Some(types) = node
				.child_by_field_name("right")
				.and_then(|right| infer_iterable_value_element_type_set(discover, right, scope))
			{
				discover.locals.record_type_set(name, types);
			} else {
				discover.locals.record_unknown_type(name);
			}
		}
		if discover.deep {
			emit_local_pattern(discover, left, scope, graph);
		}
	}
	if let Some(right) = node.child_by_field_name("right") {
		discover.recurse_subtree(right, scope, graph);
	}
	if let Some(body) = node.child_by_field_name("body") {
		discover.recurse_subtree(body, scope, graph);
	}
}

fn handle_with(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
	let body = node.child_by_field_name("body");
	let mut token_cursor = node.walk();
	let async_context = node
		.children(&mut token_cursor)
		.any(|child| child.kind() == "async");
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if body.is_some_and(|body| body == child) {
			continue;
		}
		record_with_bindings(discover, child, scope, graph, async_context);
	}
	if let Some(body) = body {
		discover.recurse_subtree(body, scope, graph);
	}
}

fn handle_except(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let mut cursor = node.walk();
	let body = node
		.named_children(&mut cursor)
		.find(|child| child.kind() == "block");
	let value = node
		.child_by_field_name("value")
		.or_else(|| node.child_by_field_name("type"));
	let alias = value
		.filter(|value| value.kind() == "as_pattern")
		.and_then(|value| value.child_by_field_name("alias"))
		.and_then(binding_identifier);
	let exception = value.and_then(|value| {
		if value.kind() == "as_pattern" {
			value.named_child(0)
		} else {
			Some(value)
		}
	});
	if let Some(alias) = alias {
		let name = node_slice(alias, discover.source_bytes);
		record_local_pattern(discover, alias);
		if let Some(types) =
			exception.map(|exception| infer_local_type_set(discover, exception, scope))
		{
			discover.locals.record_type_set(name, types);
		} else {
			discover.locals.record_unknown_type(name);
		}
		if discover.deep {
			emit_local_pattern(discover, alias, scope, graph);
		}
	}
	if let Some(exception) = exception {
		discover.recurse_subtree(exception, scope, graph);
	}
	if let Some(body) = body {
		discover.recurse_subtree(body, scope, graph);
	}
	if let Some(alias) = alias {
		discover
			.locals
			.record_unknown_type(node_slice(alias, discover.source_bytes));
	}
}

fn record_with_bindings(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
	async_context: bool,
) {
	if node.kind() == "with_item"
		&& let Some(value) = node.child_by_field_name("value")
	{
		if value.kind() == "as_pattern" {
			record_with_bindings(discover, value, scope, graph, async_context);
		} else {
			discover.recurse_subtree(value, scope, graph);
		}
		return;
	}
	if let Some(alias) = node.child_by_field_name("alias") {
		let alias = binding_identifier(alias).unwrap_or(alias);
		let value = node
			.child_by_field_name("value")
			.or_else(|| node.child_by_field_name("expression"))
			.or_else(|| node.named_child(0));
		record_local_pattern(discover, alias);
		if alias.kind() == "identifier" {
			let name = node_slice(alias, discover.source_bytes);
			if let Some(types) = value.and_then(|value| {
				infer_context_alias_type_set(discover, value, scope, async_context)
			}) {
				discover.locals.record_type_set(name, types);
			} else {
				discover.locals.record_unknown_type(name);
			}
		}
		if discover.deep {
			emit_local_pattern(discover, alias, scope, graph);
		}
		if let Some(value) = value {
			discover.recurse_subtree(value, scope, graph);
		}
		return;
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		record_with_bindings(discover, child, scope, graph, async_context);
	}
}

fn infer_context_alias_type_set(
	discover: &PyDiscover<'_>,
	value: Node<'_>,
	scope: &Moniker,
	async_context: bool,
) -> Option<LocalTypeSet> {
	let context_types =
		infer_assignment_value_type_set_with_mode(discover, value, scope, async_context)?;
	let method = if async_context {
		b"__aenter__".as_slice()
	} else {
		b"__enter__".as_slice()
	};
	let mut result = LocalTypeSet::default();
	let mut saw_context_type = false;
	for context_type in context_types.static_types() {
		saw_context_type = true;
		let Some(entry) = discover
			.callable_table
			.get(&(context_type.clone(), method.to_vec()))
		else {
			result.mark_dynamic();
			continue;
		};
		if entry.return_type_ambiguous || entry.return_type_dynamic {
			result.mark_dynamic();
		}
		for name in &entry.return_type_names {
			if name == b"Self" {
				result.insert(context_type.clone());
			} else if let Some((target, _)) = resolve_return_type_name(discover, context_type, name)
			{
				result.insert(target);
			} else {
				result.mark_dynamic();
			}
		}
	}
	if !saw_context_type {
		result.mark_dynamic();
	}
	Some(result)
}

fn binding_identifier(node: Node<'_>) -> Option<Node<'_>> {
	if node.kind() == "identifier" {
		return Some(node);
	}
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.find_map(binding_identifier)
}

fn handle_lambda(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	discover.locals.push();
	if let Some(params) = node.child_by_field_name("parameters") {
		let mut cursor = params.walk();
		for child in params.named_children(&mut cursor) {
			let (name_node, _ty) = parameter_name_and_type(child);
			let Some(nn) = name_node else { continue };
			let name = node_slice(nn, discover.source_bytes);
			if name.is_empty() {
				continue;
			}
			discover.locals.record_name(name);
			if discover.deep && is_callable_scope(scope, &discover.module) {
				let m = extend_segment(scope, kinds::PARAM, name);
				let _ = graph.add_def(m, kinds::PARAM, scope, Some(node_position(nn)));
			}
		}
	}
	if let Some(body) = node.child_by_field_name("body") {
		discover.recurse_subtree(body, scope, graph);
	}
	discover.locals.pop();
}

fn handle_keyword_argument(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if let Some(value) = node.child_by_field_name("value") {
		discover.recurse_subtree(value, scope, graph);
	}
}

fn handle_attribute(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if let Some(obj) = node.child_by_field_name("object") {
		discover.recurse_subtree(obj, scope, graph);
	}
}

fn handle_identifier(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let name = node_slice(node, discover.source_bytes);
	if name.is_empty() {
		return;
	}
	let Some((target, confidence)) = resolve_identifier_read(discover, scope, name) else {
		return;
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

fn resolve_identifier_read(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
) -> Option<(Moniker, &'static [u8])> {
	let confidence = discover
		.imports
		.confidence_for(scope, &discover.module, name)
		.or_else(|| name_confidence(discover, name))?;
	let resolved_type = if confidence != kinds::CONF_LOCAL
		&& discover
			.imports
			.confidence_for(scope, &discover.module, name)
			.is_none()
	{
		lookup_discovered_type(discover, scope, name)
	} else {
		None
	};
	let (target, confidence) = if confidence == kinds::CONF_LOCAL {
		(extend_segment(scope, kinds::LOCAL, name), confidence)
	} else if let Some(import_target) = discover.imports.target_for(scope, &discover.module, name) {
		(import_target, confidence)
	} else if let Some(type_target) = resolved_type.clone() {
		(type_target, kinds::CONF_RESOLVED)
	} else if let Some(callable_target) = lookup_module_callable(discover, name) {
		(callable_target, kinds::CONF_RESOLVED)
	} else if is_python_runtime_global(name) {
		return Some((
			python_runtime_external_target(&discover.module, name),
			kinds::CONF_EXTERNAL,
		));
	} else if is_python_builtin(name) {
		return Some((
			builtin_external_target(&discover.module, name),
			kinds::CONF_EXTERNAL,
		));
	} else {
		(
			extend_segment(&discover.module, kinds::FUNCTION, name),
			kinds::CONF_UNRESOLVED,
		)
	};
	Some((target, confidence))
}

fn record_local_pattern(discover: &PyDiscover<'_>, node: Node<'_>) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, discover.source_bytes);
			if !name.is_empty() {
				discover.locals.record_name(name);
			}
		}
		"pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				record_local_pattern(discover, child);
			}
		}
		_ => {}
	}
}

fn emit_binding_pattern(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, discover.source_bytes);
			if !name.is_empty() {
				let moniker = extend_segment(scope, kinds::PATH, name);
				let _ = graph.add_def(moniker, kinds::PATH, scope, Some(node_position(node)));
			}
		}
		"pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				emit_binding_pattern(discover, child, scope, graph);
			}
		}
		_ => {}
	}
}

fn emit_binding_type(
	discover: &PyDiscover<'_>,
	left: Node<'_>,
	right: Option<Node<'_>>,
	inferred_type: Option<Moniker>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	if left.kind() != "identifier" {
		return;
	}
	let name = node_slice(left, discover.source_bytes);
	if name.is_empty() {
		return;
	}
	let Some(target) = inferred_type
		.or_else(|| right.and_then(|node| infer_assignment_value_type(discover, node, scope)))
	else {
		return;
	};
	let confidence = if is_external_shaped(&target) {
		kinds::CONF_EXTERNAL
	} else {
		kinds::CONF_RESOLVED
	};
	let source = extend_segment(scope, kinds::PATH, name);
	let attrs = RefAttrs {
		confidence,
		..RefAttrs::default()
	};
	let _ = graph.add_ref_attrs(
		&source,
		target,
		kinds::TYPED_AS,
		Some(node_position(left)),
		&attrs,
	);
}

fn emit_local_pattern(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, discover.source_bytes);
			if !name.is_empty() {
				let moniker = extend_segment(scope, kinds::LOCAL, name);
				let _ = graph.add_def(moniker, kinds::LOCAL, scope, Some(node_position(node)));
			}
		}
		"pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				emit_local_pattern(discover, child, scope, graph);
			}
		}
		_ => {}
	}
}

fn record_assignment_type(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	left: Node<'_>,
	right: Option<Node<'_>>,
	inferred_types: Option<LocalTypeSet>,
) {
	let mut targets = inferred_types.unwrap_or_default();
	if let Some(right_types) = right
		.and_then(|node| infer_assignment_value_type_set_with_mode(discover, node, scope, false))
	{
		targets.union_with(right_types);
	}
	match left.kind() {
		"identifier" => {
			let name = node_slice(left, discover.source_bytes);
			if name.is_empty() {
				return;
			}
			if !targets.is_empty() {
				discover.locals.record_type_set(name, targets);
			} else {
				discover.locals.record_unknown_type(name);
			}
		}
		"attribute" => {
			let Some((class, attr)) = self_attr_key(discover, scope, left) else {
				return;
			};
			let key = (class, attr);
			if discover.declared_instance_attr_types.contains(&key)
				|| discover
					.ambiguous_instance_attr_types
					.borrow()
					.contains(&key)
			{
				return;
			}
			let Some(target) = targets.unique() else {
				discover.instance_attr_types.borrow_mut().remove(&key);
				discover
					.ambiguous_instance_attr_types
					.borrow_mut()
					.insert(key);
				return;
			};
			let conflicts = discover
				.instance_attr_types
				.borrow()
				.get(&key)
				.is_some_and(|known| known != &target);
			if conflicts {
				discover.instance_attr_types.borrow_mut().remove(&key);
				discover
					.ambiguous_instance_attr_types
					.borrow_mut()
					.insert(key);
			} else {
				discover
					.instance_attr_types
					.borrow_mut()
					.insert(key, target);
			}
		}
		_ => {}
	}
}

fn record_assignment_element_types(
	discover: &PyDiscover<'_>,
	left: Node<'_>,
	right: Option<Node<'_>>,
	scope: &Moniker,
) {
	if left.kind() != "identifier" {
		return;
	}
	let name = node_slice(left, discover.source_bytes);
	if name.is_empty() {
		return;
	}
	if let Some(types) =
		right.and_then(|right| infer_iterable_value_element_type_set(discover, right, scope))
	{
		discover.locals.record_element_type_set(name, types);
	} else {
		discover.locals.record_unknown_element_type(name);
	}
}

fn infer_iterable_value_element_type_set(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<LocalTypeSet> {
	match node.kind() {
		"identifier" => discover
			.locals
			.lookup_element_type_set(node_slice(node, discover.source_bytes)),
		"list" | "tuple" | "set" => {
			let mut types = LocalTypeSet::default();
			let mut saw_item = false;
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				saw_item = true;
				if let Some(item_types) =
					infer_assignment_value_type_set_with_mode(discover, child, scope, false)
				{
					types.union_with(item_types);
				} else {
					types.mark_dynamic();
				}
			}
			saw_item.then_some(types)
		}
		"call" => {
			let callee = node.child_by_field_name("function")?;
			(callee.kind() == "identifier" && node_slice(callee, discover.source_bytes) == b"range")
				.then(|| LocalTypeSet::from_type(builtin_external_target(&discover.module, b"int")))
		}
		_ => None,
	}
}

fn infer_assignment_value_type(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	infer_assignment_value_type_set_with_mode(discover, node, scope, false)?.unique()
}

fn infer_assignment_value_type_set_with_mode(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	awaited: bool,
) -> Option<LocalTypeSet> {
	match node.kind() {
		"identifier" => discover
			.locals
			.lookup_type_set(node_slice(node, discover.source_bytes)),
		"call" => {
			let callee = node.child_by_field_name("function")?;
			let target = match callee.kind() {
				"identifier" => {
					let name = node_slice(callee, discover.source_bytes);
					if discover.locals.is_name(name) {
						return lookup_function_local_type(discover, name)
							.map(LocalTypeSet::from_type);
					}
					if let Some(target) = discover
						.imports
						.target_for(scope, &discover.module, name)
						.or_else(|| lookup_discovered_type(discover, scope, name))
					{
						return Some(LocalTypeSet::from_type(target));
					}
					return lookup_callable_return_type_set(discover, name, awaited);
				}
				"attribute" => attribute_callee_type(discover, callee, scope),
				_ => None,
			};
			target.map(LocalTypeSet::from_type)
		}
		"await" => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor).find_map(|child| {
				infer_assignment_value_type_set_with_mode(discover, child, scope, true)
			})
		}
		"conditional_expression" | "boolean_operator" => {
			let mut types = LocalTypeSet::default();
			let mut saw_value = false;
			let condition = node.child_by_field_name("condition");
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				if condition.is_some_and(|condition| condition == child) {
					continue;
				}
				saw_value = true;
				if let Some(child_types) =
					infer_assignment_value_type_set_with_mode(discover, child, scope, awaited)
				{
					types.union_with(child_types);
				} else {
					types.mark_dynamic();
				}
			}
			saw_value.then_some(types)
		}
		"string" | "concatenated_string" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			string_literal_type_name(node, discover.source_bytes),
		))),
		"integer" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"int",
		))),
		"float" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"float",
		))),
		"true" | "false" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"bool",
		))),
		"list" | "list_comprehension" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"list",
		))),
		"dictionary" | "dictionary_comprehension" => Some(LocalTypeSet::from_type(
			builtin_external_target(&discover.module, b"dict"),
		)),
		"set" | "set_comprehension" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"set",
		))),
		"tuple" => Some(LocalTypeSet::from_type(builtin_external_target(
			&discover.module,
			b"tuple",
		))),
		_ => None,
	}
}

fn lookup_function_local_type(discover: &PyDiscover<'_>, name: &[u8]) -> Option<Moniker> {
	discover.locals.lookup_type_binding(name)
}

fn string_literal_type_name(node: Node<'_>, source: &[u8]) -> &'static [u8] {
	let text = node_slice(node, source);
	let prefix = text.iter().take_while(|byte| !matches!(byte, b'\'' | b'"'));
	if prefix.into_iter().any(|byte| matches!(byte, b'b' | b'B')) {
		b"bytes"
	} else {
		b"str"
	}
}

fn lookup_callable_return_type_set(
	discover: &PyDiscover<'_>,
	name: &[u8],
	awaited: bool,
) -> Option<LocalTypeSet> {
	std::iter::once(discover.module.clone()).find_map(|parent| {
		let entry = discover
			.callable_table
			.get(&(parent.clone(), name.to_vec()))?;
		if entry.return_type_ambiguous || entry.is_async != awaited {
			return None;
		}
		let mut types = LocalTypeSet::default();
		for return_name in &entry.return_type_names {
			if let Some((target, _)) = resolve_return_type_name(discover, &parent, return_name) {
				types.insert(target);
			} else {
				types.mark_dynamic();
			}
		}
		if entry.return_type_dynamic {
			types.mark_dynamic();
		}
		(!entry.return_type_names.is_empty() || entry.return_type_dynamic).then_some(types)
	})
}

fn attribute_callee_type(
	discover: &PyDiscover<'_>,
	callee: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	let object = callee.child_by_field_name("object")?;
	if object.kind() != "identifier" {
		return None;
	}
	let object_name = node_slice(object, discover.source_bytes);
	let module_target = discover
		.imports
		.target_for(scope, &discover.module, object_name)?;
	if !is_external_shaped(&module_target) {
		return None;
	}
	let attr = callee.child_by_field_name("attribute")?;
	let name = node_slice(attr, discover.source_bytes);
	Some(extend_segment(&module_target, kinds::PATH, name))
}

fn is_external_shaped(target: &Moniker) -> bool {
	target
		.as_view()
		.segments()
		.next()
		.is_some_and(|segment| segment.kind == kinds::EXTERNAL_PKG)
}

fn self_attr_key(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	node: Node<'_>,
) -> Option<(Moniker, Vec<u8>)> {
	if node.kind() != "attribute" {
		return None;
	}
	let obj = node.child_by_field_name("object")?;
	if obj.kind() != "identifier"
		|| !matches!(node_slice(obj, discover.source_bytes), b"self" | b"cls")
	{
		return None;
	}
	let class = enclosing_class(scope, &discover.module)?;
	let attr = last_attribute(node, discover.source_bytes)
		.as_bytes()
		.to_vec();
	if attr.is_empty() {
		return None;
	}
	Some((class, attr))
}

fn name_confidence(discover: &PyDiscover<'_>, name: &[u8]) -> Option<&'static [u8]> {
	crate::lang::kinds::name_confidence_for(discover.locals.is_name(name), discover.deep)
}

fn record_param_locals(
	discover: &PyDiscover<'_>,
	params: Node<'_>,
	source: &[u8],
	scope: &Moniker,
) {
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let Some(name_node) = name_node else { continue };
		let name = node_slice(name_node, source);
		if name.is_empty() {
			continue;
		}
		discover.locals.record_name(name);
		if parameter_has_pattern(child, "dictionary_splat_pattern") {
			discover.locals.record_type_set(
				name,
				LocalTypeSet::from_type(builtin_external_target(&discover.module, b"dict")),
			);
		} else if parameter_has_pattern(child, "list_splat_pattern") {
			discover.locals.record_type_set(
				name,
				LocalTypeSet::from_type(builtin_external_target(&discover.module, b"tuple")),
			);
		} else if let Some(type_node) = type_node {
			discover
				.locals
				.record_type_set(name, infer_local_type_set(discover, type_node, scope));
			if let Some(element_types) =
				infer_iterable_annotation_element_type_set(discover, type_node, scope)
			{
				discover.locals.record_element_type_set(name, element_types);
			}
		}
	}
}

fn emit_param_defs_and_types(
	discover: &PyDiscover<'_>,
	params: Node<'_>,
	callable: &Moniker,
	source: &[u8],
	graph: &mut SdkBuilder,
) {
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let Some(name_node) = name_node else { continue };
		let name = node_slice(name_node, source);
		if name.is_empty() {
			continue;
		}
		if discover.deep {
			let moniker = extend_segment(callable, kinds::PARAM, name);
			let _ = graph.add_def(moniker, kinds::PARAM, callable, Some(node_position(child)));
		}
		if let Some(typed) = type_node {
			PyTypeRefs::new(discover, callable).emit(typed, graph);
		}
	}
}

fn emit_local_type_facts(
	scope: &Moniker,
	name: &[u8],
	position: Option<Position>,
	types: &LocalTypeSet,
	graph: &mut SdkBuilder,
) {
	for target in types.static_types() {
		let attrs = RefAttrs {
			confidence: if is_external_shaped(target) {
				kinds::CONF_EXTERNAL
			} else {
				kinds::CONF_RESOLVED
			},
			alias: name,
			receiver_hint: if types.is_dynamic() {
				b"python_open_type_set"
			} else {
				b""
			},
			..RefAttrs::default()
		};
		let _ = graph.add_ref_attrs(scope, target.clone(), kinds::TYPED_AS, position, &attrs);
	}
}

fn collect_base_class_refs(
	discover: &PyDiscover<'_>,
	supers: Node<'_>,
	scope: &Moniker,
	out: &mut Vec<RefSpec>,
) {
	let mut cursor = supers.walk();
	for child in supers.named_children(&mut cursor) {
		let (target, confidence) = match qualified_base_target(discover, child, scope) {
			Some(qualified) => qualified,
			None => {
				let name = match base_class_name(child, discover.source_bytes) {
					Some(name) => name,
					None => continue,
				};
				resolve_type_target(discover, scope, &name, kinds::CLASS)
			}
		};
		out.push(RefSpec {
			kind: kinds::EXTENDS,
			target,
			confidence,
			position: node_position(child),
			receiver_hint: b"",
			alias: b"",
		});
	}
}

// `class C(mod.Base)` anchors the base on the imported module `mod`
// instead of collapsing to the bare attribute name.
fn qualified_base_target(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<(Moniker, &'static [u8])> {
	let base = match node.kind() {
		"attribute" => node,
		"subscript" => {
			let value = node.child_by_field_name("value")?;
			if value.kind() != "attribute" {
				return None;
			}
			value
		}
		_ => return None,
	};
	let object = base.child_by_field_name("object")?;
	if object.kind() != "identifier" {
		return None;
	}
	let object_name = node_slice(object, discover.source_bytes);
	let module_target = discover
		.imports
		.target_for(scope, &discover.module, object_name)?;
	let attr = base.child_by_field_name("attribute")?;
	let name = node_slice(attr, discover.source_bytes);
	if name.is_empty() {
		return None;
	}
	let confidence = if is_external_shaped(&module_target) {
		kinds::CONF_EXTERNAL
	} else {
		discover
			.imports
			.confidence_for(scope, &discover.module, object_name)
			.unwrap_or(kinds::CONF_IMPORTED)
	};
	Some((
		extend_segment(&module_target, kinds::PATH, name),
		confidence,
	))
}

fn base_class_name(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	let name = match node.kind() {
		"identifier" => node_slice(node, source).to_vec(),
		"attribute" => last_attribute(node, source).as_bytes().to_vec(),
		"subscript" => {
			let value = node.child_by_field_name("value")?;
			match value.kind() {
				"identifier" => node_slice(value, source).to_vec(),
				"attribute" => last_attribute(value, source).as_bytes().to_vec(),
				_ => return None,
			}
		}
		"keyword_argument" => return None,
		_ => return None,
	};
	if name.is_empty() { None } else { Some(name) }
}

fn collect_decorator_refs(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	out: &mut Vec<RefSpec>,
) {
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		let Some((name, name_node)) = decorator_name(child, discover.source_bytes) else {
			continue;
		};
		if name.is_empty() {
			continue;
		}
		let (target, confidence) = match qualified_decorator_target(discover, name_node, scope) {
			Some(qualified) => qualified,
			None => resolve_type_target(discover, scope, &name, kinds::FUNCTION),
		};
		out.push(RefSpec {
			kind: kinds::ANNOTATES,
			target,
			confidence,
			position: node_position(name_node),
			receiver_hint: b"",
			alias: b"",
		});
	}
}

// `@obj.deco` anchors on the imported module or the module-level value
// `obj` instead of collapsing to the bare attribute name.
fn qualified_decorator_target(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<(Moniker, &'static [u8])> {
	if node.kind() != "attribute" {
		return None;
	}
	let object = node.child_by_field_name("object")?;
	if object.kind() != "identifier" {
		return None;
	}
	let object_name = node_slice(object, discover.source_bytes);
	let attr = node.child_by_field_name("attribute")?;
	let name = node_slice(attr, discover.source_bytes);
	if name.is_empty() {
		return None;
	}
	let (base, confidence) = match discover
		.imports
		.target_for(scope, &discover.module, object_name)
	{
		Some(target) => {
			let confidence = if is_external_shaped(&target) {
				kinds::CONF_EXTERNAL
			} else {
				kinds::CONF_IMPORTED
			};
			(target, confidence)
		}
		None => (
			extend_segment(&discover.module, kinds::PATH, object_name),
			kinds::CONF_NAME_MATCH,
		),
	};
	Some((extend_segment(&base, kinds::FUNCTION, name), confidence))
}

fn decorator_name<'tree>(node: Node<'tree>, source: &[u8]) -> Option<(Vec<u8>, Node<'tree>)> {
	match node.kind() {
		"identifier" => Some((node_slice(node, source).to_vec(), node)),
		"attribute" => Some((last_attribute(node, source).as_bytes().to_vec(), node)),
		"call" => {
			let function = node.child_by_field_name("function")?;
			match function.kind() {
				"identifier" => Some((node_slice(function, source).to_vec(), function)),
				"attribute" => Some((
					last_attribute(function, source).as_bytes().to_vec(),
					function,
				)),
				_ => None,
			}
		}
		_ => None,
	}
}

impl<'src_lang> PyDiscover<'src_lang> {
	fn recurse_subtree(&self, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
		let walker = PyWalker::new(self, self.source_bytes);
		walker.dispatch(node, scope, graph);
	}
}

fn infer_type_target(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, discover.source_bytes);
			if let Some(target) = lookup_discovered_type(discover, scope, name) {
				return Some(target);
			}
			if imported_nonconcrete_typing(discover, scope, name) {
				return None;
			}
			if let Some(target) = discover.imports.target_for(scope, &discover.module, name) {
				return Some(target);
			}
			if should_skip_type_name(name) || is_typing_container(name) {
				return is_inferable_builtin_type(name)
					.then(|| builtin_external_target(&discover.module, name));
			}
			let (target, _) = resolve_type_target(discover, scope, name, kinds::CLASS);
			Some(target)
		}
		"attribute" => {
			if qualified_nonconcrete_typing(discover, node, scope) {
				return None;
			}
			let name = last_attribute(node, discover.source_bytes);
			if should_skip_type_name(name.as_bytes()) || is_typing_container(name.as_bytes()) {
				return None;
			}
			let (target, _) = resolve_type_target(discover, scope, name.as_bytes(), kinds::CLASS);
			Some(target)
		}
		"union_type" | "binary_operator" => infer_unique_child_type(discover, node, scope),
		"subscript" | "generic_type" if has_typing_container_base(discover, node, scope) => None,
		"type"
		| "subscript"
		| "generic_type"
		| "type_parameter"
		| "member_type"
		| "constrained_type"
		| "splat_type"
		| "tuple"
		| "list"
		| "expression_list"
		| "parenthesized_expression" => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor)
				.find_map(|child| infer_type_target(discover, child, scope))
		}
		_ => None,
	}
}

fn infer_local_type_set(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> LocalTypeSet {
	if matches!(node.kind(), "type" | "parenthesized_expression") {
		let mut cursor = node.walk();
		let children = node.named_children(&mut cursor).collect::<Vec<_>>();
		if let [child] = children.as_slice() {
			return infer_local_type_set(discover, *child, scope);
		}
	}
	if matches!(node.kind(), "union_type" | "binary_operator") {
		return infer_child_type_set(discover, node, scope);
	}
	if matches!(
		node.kind(),
		"type_parameter" | "tuple" | "list" | "expression_list"
	) {
		let mut cursor = node.walk();
		if node.named_children(&mut cursor).count() > 1 {
			return infer_child_type_set(discover, node, scope);
		}
	}
	if matches!(node.kind(), "subscript" | "generic_type")
		&& let Some(base) = node.named_child(0)
		&& base.kind() == "identifier"
	{
		let name = node_slice(base, discover.source_bytes);
		if is_inferable_builtin_type(name) {
			return LocalTypeSet::from_type(builtin_external_target(&discover.module, name));
		}
	}
	if is_none_type(node, discover.source_bytes) {
		let mut types = LocalTypeSet::default();
		types.mark_dynamic();
		return types;
	}
	infer_type_target(discover, node, scope)
		.map(LocalTypeSet::from_type)
		.unwrap_or_else(|| {
			let mut types = LocalTypeSet::default();
			types.mark_dynamic();
			types
		})
}

fn infer_child_type_set(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> LocalTypeSet {
	let mut types = LocalTypeSet::default();
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		if is_none_type(child, discover.source_bytes) {
			types.mark_dynamic();
		} else {
			types.union_with(infer_local_type_set(discover, child, scope));
		}
	}
	if types.is_empty() {
		types.mark_dynamic();
	}
	types
}

fn infer_iterable_annotation_element_type_set(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<LocalTypeSet> {
	if matches!(node.kind(), "type" | "parenthesized_expression") {
		let mut cursor = node.walk();
		let children = node.named_children(&mut cursor).collect::<Vec<_>>();
		if let [child] = children.as_slice() {
			return infer_iterable_annotation_element_type_set(discover, *child, scope);
		}
	}
	if matches!(node.kind(), "union_type" | "binary_operator") {
		let mut types = LocalTypeSet::default();
		let mut saw_iterable = false;
		let mut cursor = node.walk();
		for child in node.named_children(&mut cursor) {
			if let Some(child_types) =
				infer_iterable_annotation_element_type_set(discover, child, scope)
			{
				types.union_with(child_types);
				saw_iterable = true;
			} else {
				types.mark_dynamic();
			}
		}
		return saw_iterable.then_some(types);
	}
	if !matches!(node.kind(), "subscript" | "generic_type") {
		return None;
	}
	let base = node.named_child(0)?;
	let base_name = match base.kind() {
		"identifier" => node_slice(base, discover.source_bytes).to_vec(),
		"attribute" => last_attribute(base, discover.source_bytes)
			.as_bytes()
			.to_vec(),
		_ => return None,
	};
	if !is_iterable_annotation_container(&base_name) {
		return None;
	}
	let arguments = node.named_child(1)?;
	if matches!(base_name.as_slice(), b"tuple" | b"Tuple") {
		return Some(infer_child_type_set(discover, arguments, scope));
	}
	let mut cursor = arguments.walk();
	let first = arguments
		.named_children(&mut cursor)
		.next()
		.unwrap_or(arguments);
	Some(infer_local_type_set(discover, first, scope))
}

fn is_iterable_annotation_container(name: &[u8]) -> bool {
	matches!(
		name,
		b"list"
			| b"set" | b"tuple"
			| b"dict" | b"List"
			| b"Set" | b"Tuple"
			| b"Dict" | b"Iterable"
			| b"Iterator"
			| b"AsyncIterable"
			| b"AsyncIterator"
			| b"Sequence"
			| b"Collection"
			| b"Generator"
			| b"Mapping"
			| b"MutableMapping"
	)
}

fn is_none_type(node: Node<'_>, source: &[u8]) -> bool {
	node.kind() == "none" || node_slice(node, source) == b"None"
}

fn infer_unique_child_type(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	let mut unique = None;
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		let candidate = infer_type_target(discover, child, scope)?;
		if unique.as_ref().is_some_and(|known| known != &candidate) {
			return None;
		}
		unique = Some(candidate);
	}
	unique
}

fn has_typing_container_base(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker) -> bool {
	let mut cursor = node.walk();
	let Some(base) = node.named_children(&mut cursor).next() else {
		return false;
	};
	let name = match base.kind() {
		"identifier" => {
			let name = node_slice(base, discover.source_bytes);
			if imported_typing_container(discover, scope, name) {
				return true;
			}
			name
		}
		"attribute" => last_attribute(base, discover.source_bytes).as_bytes(),
		_ => return false,
	};
	is_typing_container(name)
}

fn imported_typing_container(discover: &PyDiscover<'_>, scope: &Moniker, name: &[u8]) -> bool {
	imported_nonconcrete_typing(discover, scope, name)
}

fn imported_nonconcrete_typing(discover: &PyDiscover<'_>, scope: &Moniker, name: &[u8]) -> bool {
	let Some(target) = discover.imports.target_for(scope, &discover.module, name) else {
		return false;
	};
	let segments = target.as_view().segments().collect::<Vec<_>>();
	let Some(root) = segments.first() else {
		return false;
	};
	let Some(last) = segments.last() else {
		return false;
	};
	root.kind == kinds::EXTERNAL_PKG
		&& matches!(root.name, b"typing" | b"typing_extensions")
		&& (is_typing_container(last.name)
			|| matches!(last.name, b"Any" | b"Self" | b"Never" | b"NoReturn"))
}

fn qualified_nonconcrete_typing(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> bool {
	let Some(object) = node.child_by_field_name("object") else {
		return false;
	};
	if object.kind() != "identifier" {
		return false;
	}
	let object_name = node_slice(object, discover.source_bytes);
	let Some(module) = discover
		.imports
		.target_for(scope, &discover.module, object_name)
	else {
		return false;
	};
	let Some(root) = module.as_view().segments().next() else {
		return false;
	};
	let leaf = last_attribute(node, discover.source_bytes);
	root.kind == kinds::EXTERNAL_PKG
		&& matches!(root.name, b"typing" | b"typing_extensions")
		&& (is_typing_container(leaf.as_bytes())
			|| matches!(leaf.as_bytes(), b"Any" | b"Self" | b"Never" | b"NoReturn"))
}

fn lookup_discovered_type(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
) -> Option<Moniker> {
	lookup_type_target(&discover.type_table, scope, name)
}

fn resolve_type_target(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
	fallback_kind: &[u8],
) -> (Moniker, &'static [u8]) {
	if let Some(m) = lookup_discovered_type(discover, scope, name) {
		return (m, kinds::CONF_RESOLVED);
	}
	if let Some(m) = discover.imports.target_for(scope, &discover.module, name) {
		let confidence = discover
			.imports
			.confidence_for(scope, &discover.module, name)
			.unwrap_or(kinds::CONF_NAME_MATCH);
		return (m, confidence);
	}
	if is_python_builtin(name) {
		return (
			builtin_external_target(&discover.module, name),
			kinds::CONF_EXTERNAL,
		);
	}
	let target = extend_segment(&discover.module, fallback_kind, name);
	let confidence = discover
		.imports
		.confidence_for(scope, &discover.module, name)
		.unwrap_or(kinds::CONF_NAME_MATCH);
	(target, confidence)
}

fn lookup_callable_in_scope(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	name: &[u8],
	kind: &[u8],
) -> Option<Moniker> {
	let mut parents = Vec::with_capacity(2);
	if let Some(class) = enclosing_class(scope, &discover.module) {
		parents.push(class);
	}
	parents.push(discover.module.clone());
	for parent in parents {
		let Some(entry) = discover
			.callable_table
			.get(&(parent.clone(), name.to_vec()))
		else {
			continue;
		};
		if entry.kind == kind {
			return Some(extend_segment(&parent, kind, &entry.segment));
		}
	}
	None
}

fn lookup_module_callable(discover: &PyDiscover<'_>, name: &[u8]) -> Option<Moniker> {
	[kinds::FUNCTION, kinds::ASYNC_FUNCTION]
		.into_iter()
		.find_map(|kind| lookup_callable_in_scope(discover, &discover.module, name, kind))
}

fn lookup_callable(discover: &PyDiscover<'_>, scope: &Moniker, name: &[u8]) -> Moniker {
	lookup_callable_in_scope(discover, scope, name, kinds::METHOD)
		.or_else(|| lookup_callable_in_scope(discover, scope, name, kinds::FUNCTION))
		.or_else(|| lookup_callable_in_scope(discover, scope, name, kinds::ASYNC_FUNCTION))
		.unwrap_or_else(|| extend_segment(&discover.module, kinds::FUNCTION, name))
}

fn lookup_method_on_typed_receiver(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	receiver: Node<'_>,
	name: &[u8],
) -> Option<CallableTarget> {
	let target_type = match receiver.kind() {
		"identifier" => discover
			.locals
			.lookup_type(node_slice(receiver, discover.source_bytes)),
		"attribute" => lookup_self_attr_type(discover, scope, receiver),
		_ => None,
	}?;
	lookup_callable_on_type(discover, &target_type, name, kinds::METHOD)
}

fn lookup_self_attr_type(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	receiver: Node<'_>,
) -> Option<Moniker> {
	if receiver.kind() != "attribute" {
		return None;
	}
	let obj = receiver.child_by_field_name("object")?;
	if obj.kind() != "identifier"
		|| !matches!(node_slice(obj, discover.source_bytes), b"self" | b"cls")
	{
		return None;
	}
	let class = enclosing_class(scope, &discover.module)?;
	let attr = last_attribute(receiver, discover.source_bytes)
		.as_bytes()
		.to_vec();
	discover
		.instance_attr_types
		.borrow()
		.get(&(class, attr))
		.cloned()
}

fn lookup_self_named_attr_type(
	discover: &PyDiscover<'_>,
	scope: &Moniker,
	attr: &[u8],
) -> Option<Moniker> {
	let class = enclosing_class(scope, &discover.module)?;
	discover
		.instance_attr_types
		.borrow()
		.get(&(class, attr.to_vec()))
		.cloned()
}

fn lookup_callable_on_type(
	discover: &PyDiscover<'_>,
	type_moniker: &Moniker,
	name: &[u8],
	kind: &[u8],
) -> Option<CallableTarget> {
	if let Some(entry) = discover
		.callable_table
		.get(&(type_moniker.clone(), name.to_vec()))
	{
		if entry.kind != kind {
			return None;
		}
		return Some(CallableTarget {
			moniker: extend_segment(type_moniker, kind, &entry.segment),
			confidence: kinds::CONF_RESOLVED,
		});
	}
	type_moniker
		.as_view()
		.segments()
		.last()
		.filter(|segment| segment.kind == kinds::PATH)
		.map(|_| CallableTarget {
			moniker: extend_segment(type_moniker, kind, name),
			confidence: if is_external_shaped(type_moniker) {
				kinds::CONF_EXTERNAL
			} else {
				kinds::CONF_IMPORTED
			},
		})
}

fn enclosing_class(scope: &Moniker, module: &Moniker) -> Option<Moniker> {
	let view = scope.as_view();
	let segs: Vec<_> = view.segments().collect();
	let idx = segs.iter().rposition(|s| s.kind == b"class")?;
	let mut b = crate::core::moniker::MonikerBuilder::new();
	b.project(view.project());
	for s in &segs[..=idx] {
		b.segment(s.kind, s.name);
	}
	let out = b.build();
	if &out == module { None } else { Some(out) }
}

fn is_async_function(node: Node<'_>) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.any(|child| child.kind() == "async")
}

pub(super) fn collect_callable_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	is_class_scope: bool,
	out: &mut HashMap<(Moniker, Vec<u8>), CallableEntry>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let (class_node, function_node) = match child.kind() {
			"class_definition" => (Some(child), None),
			"function_definition" => (None, Some(child)),
			"decorated_definition" => {
				let mut def = None;
				let mut dc = child.walk();
				for c in child.children(&mut dc) {
					if matches!(c.kind(), "class_definition" | "function_definition") {
						def = Some(c);
						break;
					}
				}
				match def.map(|n| n.kind()) {
					Some("class_definition") => (def, None),
					Some("function_definition") => (None, def),
					_ => (None, None),
				}
			}
			_ => (None, None),
		};
		if let Some(class_node) = class_node {
			let Some(name_node) = class_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let scope = extend_segment(parent, kinds::CLASS, name);
			if let Some(body) = class_node.child_by_field_name("body") {
				collect_callable_table(body, source, &scope, true, out);
			}
		} else if let Some(function_node) = function_node {
			let Some(name_node) = function_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let slots = collect_param_slots(function_node, source, is_class_scope);
			let seg = callable_segment_slots(name, &slots);
			let kind = if is_class_scope {
				kinds::METHOD
			} else if is_async_function(function_node) {
				kinds::ASYNC_FUNCTION
			} else {
				kinds::FUNCTION
			};
			let (return_type_names, return_type_dynamic) = function_node
				.child_by_field_name("return_type")
				.map(|node| return_type_names(node, source))
				.unwrap_or_else(|| (Vec::new(), true));
			let is_async = is_async_function(function_node);
			let key = (parent.clone(), name.to_vec());
			match out.entry(key) {
				std::collections::hash_map::Entry::Vacant(entry) => {
					entry.insert(CallableEntry {
						kind,
						segment: seg,
						return_type_names,
						return_type_dynamic,
						return_type_ambiguous: false,
						is_async,
					});
				}
				std::collections::hash_map::Entry::Occupied(mut entry) => {
					let callable = entry.get_mut();
					callable.return_type_ambiguous |= callable.return_type_names
						!= return_type_names
						|| callable.return_type_dynamic != return_type_dynamic
						|| callable.is_async != is_async;
					callable.kind = kind;
					callable.segment = seg;
					callable.return_type_names = if callable.return_type_ambiguous {
						Vec::new()
					} else {
						return_type_names
					};
					callable.return_type_dynamic =
						callable.return_type_ambiguous || return_type_dynamic;
					callable.is_async = is_async;
				}
			}
		} else {
			collect_callable_table(child, source, parent, is_class_scope, out);
		}
	}
}

pub(super) fn collect_type_table<'src>(
	node: Node<'src>,
	source: &'src [u8],
	parent: &Moniker,
	is_class_scope: bool,
	out: &mut TypeTable,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() == "type_alias_statement" {
			if let Some(left) = child.child_by_field_name("left")
				&& let Some(name_node) = type_alias_name_node(left)
			{
				let name = node_slice(name_node, source);
				if !name.is_empty() {
					let m = extend_segment(parent, kinds::TYPE, name);
					record_type_candidate(out, name, m);
				}
			}
			continue;
		}
		if child.kind() == "assignment"
			&& child
				.child_by_field_name("type")
				.is_some_and(|n| type_annotation_has_name(n, source, b"TypeAlias"))
		{
			if let Some(left) = child.child_by_field_name("left")
				&& let Some(name_node) = assignment_alias_name_node(left)
			{
				let name = node_slice(name_node, source);
				if !name.is_empty() {
					let m = extend_segment(parent, kinds::TYPE, name);
					record_type_candidate(out, name, m);
				}
			}
			continue;
		}
		let (class_node, function_node) = match child.kind() {
			"class_definition" => (Some(child), None),
			"function_definition" => (None, Some(child)),
			"decorated_definition" => match decorated_definition_node(child).map(|d| d.kind()) {
				Some("class_definition") => (decorated_definition_node(child), None),
				Some("function_definition") => (None, decorated_definition_node(child)),
				_ => (None, None),
			},
			_ => (None, None),
		};
		if let Some(class_node) = class_node {
			let Some(name_node) = class_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let m = extend_segment(parent, kinds::CLASS, name);
			record_type_candidate(out, name, m.clone());
			if let Some(body) = class_node.child_by_field_name("body") {
				collect_type_table(body, source, &m, true, out);
			}
		} else if let Some(function_node) = function_node {
			let Some(function_scope) =
				function_scope_moniker(function_node, source, parent, is_class_scope)
			else {
				continue;
			};
			if let Some(body) = function_node.child_by_field_name("body") {
				collect_type_table(body, source, &function_scope, false, out);
			}
		} else {
			collect_type_table(child, source, parent, is_class_scope, out);
		}
	}
}

pub(super) fn collect_instance_attr_types(
	node: Node<'_>,
	source: &[u8],
	parent: &Moniker,
	is_class_scope: bool,
	type_table: &TypeTable,
	out: &mut HashMap<(Moniker, Vec<u8>), Moniker>,
) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		let (class_node, function_node) = match child.kind() {
			"class_definition" => (Some(child), None),
			"function_definition" => (None, Some(child)),
			"decorated_definition" => match decorated_definition_node(child).map(|d| d.kind()) {
				Some("class_definition") => (decorated_definition_node(child), None),
				Some("function_definition") => (None, decorated_definition_node(child)),
				_ => (None, None),
			},
			_ => (None, None),
		};
		if let Some(class_node) = class_node {
			let Some(name_node) = class_node.child_by_field_name("name") else {
				continue;
			};
			let name = node_slice(name_node, source);
			let class_scope = extend_segment(parent, kinds::CLASS, name);
			collect_class_init_attr_types(class_node, source, &class_scope, type_table, out);
			if let Some(body) = class_node.child_by_field_name("body") {
				collect_instance_attr_types(body, source, &class_scope, true, type_table, out);
			}
		} else if let Some(function_node) = function_node {
			let Some(function_scope) =
				function_scope_moniker(function_node, source, parent, is_class_scope)
			else {
				continue;
			};
			if let Some(body) = function_node.child_by_field_name("body") {
				collect_instance_attr_types(body, source, &function_scope, false, type_table, out);
			}
		} else {
			collect_instance_attr_types(child, source, parent, is_class_scope, type_table, out);
		}
	}
}

fn collect_class_init_attr_types(
	class_node: Node<'_>,
	source: &[u8],
	class_scope: &Moniker,
	type_table: &TypeTable,
	out: &mut HashMap<(Moniker, Vec<u8>), Moniker>,
) {
	let Some(body) = class_node.child_by_field_name("body") else {
		return;
	};
	let mut cursor = body.walk();
	for child in body.named_children(&mut cursor) {
		let function_node = match child.kind() {
			"function_definition" => Some(child),
			"decorated_definition" => decorated_definition_node(child).filter(|d| {
				d.kind() == "function_definition"
					&& d.child_by_field_name("name")
						.is_some_and(|n| node_slice(n, source) == b"__init__")
			}),
			_ => None,
		};
		let Some(function_node) = function_node else {
			continue;
		};
		let Some(name_node) = function_node.child_by_field_name("name") else {
			continue;
		};
		if node_slice(name_node, source) != b"__init__" {
			continue;
		}
		let Some(method_scope) = function_scope_moniker(function_node, source, class_scope, true)
		else {
			continue;
		};
		let params = function_node
			.child_by_field_name("parameters")
			.map(|params| collect_param_type_bindings(params, source, &method_scope, type_table))
			.unwrap_or_default();
		if let Some(body) = function_node.child_by_field_name("body") {
			collect_init_attr_assignments(
				body,
				source,
				class_scope,
				&method_scope,
				&params,
				type_table,
				out,
			);
		}
	}
}

fn collect_param_type_bindings(
	params: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	type_table: &TypeTable,
) -> HashMap<Vec<u8>, Moniker> {
	let mut out = HashMap::new();
	let mut cursor = params.walk();
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let (Some(name_node), Some(type_node)) = (name_node, type_node) else {
			continue;
		};
		let name = node_slice(name_node, source);
		if matches!(name, b"self" | b"cls") {
			continue;
		}
		if let Some(target) = static_infer_type_target(type_node, source, scope, type_table) {
			out.insert(name.to_vec(), target);
		}
	}
	out
}

fn collect_init_attr_assignments(
	node: Node<'_>,
	source: &[u8],
	class_scope: &Moniker,
	method_scope: &Moniker,
	params: &HashMap<Vec<u8>, Moniker>,
	type_table: &TypeTable,
	out: &mut HashMap<(Moniker, Vec<u8>), Moniker>,
) {
	if node.kind() == "function_definition" || node.kind() == "class_definition" {
		return;
	}
	if node.kind() == "assignment"
		&& let Some(left) = node.child_by_field_name("left")
		&& let Some(attr) = self_attr_name(left, source)
	{
		let annotation_type = node
			.child_by_field_name("type")
			.and_then(|t| static_infer_type_target(t, source, method_scope, type_table));
		let right_type = node.child_by_field_name("right").and_then(|right| {
			static_assignment_value_type(right, source, method_scope, params, type_table)
		});
		if let Some(target) = annotation_type.or(right_type) {
			out.entry((class_scope.clone(), attr)).or_insert(target);
		}
	}
	let mut cursor = node.walk();
	for child in node.named_children(&mut cursor) {
		collect_init_attr_assignments(
			child,
			source,
			class_scope,
			method_scope,
			params,
			type_table,
			out,
		);
	}
}

fn static_assignment_value_type(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	params: &HashMap<Vec<u8>, Moniker>,
	type_table: &TypeTable,
) -> Option<Moniker> {
	match node.kind() {
		"identifier" => params.get(node_slice(node, source)).cloned(),
		"call" => {
			let callee = node.child_by_field_name("function")?;
			if callee.kind() == "identifier" {
				lookup_type_target(type_table, scope, node_slice(callee, source))
			} else {
				None
			}
		}
		"await" => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor).find_map(|child| {
				static_assignment_value_type(child, source, scope, params, type_table)
			})
		}
		_ => None,
	}
}

fn static_infer_type_target(
	node: Node<'_>,
	source: &[u8],
	scope: &Moniker,
	type_table: &TypeTable,
) -> Option<Moniker> {
	match node.kind() {
		"identifier" => {
			let name = node_slice(node, source);
			if should_skip_type_name(name) || is_typing_container(name) {
				return None;
			}
			lookup_type_target(type_table, scope, name)
		}
		"attribute" => {
			let name = last_attribute(node, source).as_bytes();
			if should_skip_type_name(name) || is_typing_container(name) {
				return None;
			}
			lookup_type_target(type_table, scope, name)
		}
		"type"
		| "subscript"
		| "generic_type"
		| "type_parameter"
		| "member_type"
		| "constrained_type"
		| "splat_type"
		| "tuple"
		| "list"
		| "union_type"
		| "binary_operator"
		| "expression_list"
		| "parenthesized_expression" => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor)
				.find_map(|child| static_infer_type_target(child, source, scope, type_table))
		}
		_ => None,
	}
}

fn lookup_type_target(type_table: &TypeTable, scope: &Moniker, name: &[u8]) -> Option<Moniker> {
	type_table
		.get(name)?
		.iter()
		.filter(|candidate| type_candidate_visible(candidate, scope))
		.max_by_key(|candidate| type_candidate_depth(candidate))
		.cloned()
}

fn type_candidate_visible(candidate: &Moniker, scope: &Moniker) -> bool {
	candidate
		.parent()
		.is_some_and(|parent| parent.as_view().is_ancestor_of(&scope.as_view()))
}

fn type_candidate_depth(candidate: &Moniker) -> u16 {
	candidate
		.parent()
		.map(|parent| parent.as_view().segment_count())
		.unwrap_or_default()
}

fn record_type_candidate(out: &mut TypeTable, name: &[u8], moniker: Moniker) {
	if name.is_empty() {
		return;
	}
	out.entry(name.to_vec()).or_default().push(moniker);
}

fn decorated_definition_node(node: Node<'_>) -> Option<Node<'_>> {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.find(|child| matches!(child.kind(), "class_definition" | "function_definition"))
}

fn function_scope_moniker(
	function_node: Node<'_>,
	source: &[u8],
	parent: &Moniker,
	is_class_scope: bool,
) -> Option<Moniker> {
	let name_node = function_node.child_by_field_name("name")?;
	let name = node_slice(name_node, source);
	let slots = collect_param_slots(function_node, source, is_class_scope);
	let kind = if is_class_scope {
		kinds::METHOD
	} else if is_async_function(function_node) {
		kinds::ASYNC_FUNCTION
	} else {
		kinds::FUNCTION
	};
	Some(extend_callable_slots(parent, kind, name, &slots))
}

fn self_attr_name(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	if node.kind() != "attribute" {
		return None;
	}
	let obj = node.child_by_field_name("object")?;
	if obj.kind() != "identifier" || !matches!(node_slice(obj, source), b"self" | b"cls") {
		return None;
	}
	let attr = last_attribute(node, source).as_bytes().to_vec();
	if attr.is_empty() { None } else { Some(attr) }
}

fn type_alias_name_node(alias_type: Node<'_>) -> Option<Node<'_>> {
	match alias_type.kind() {
		"identifier" => Some(alias_type),
		"type" | "generic_type" | "member_type" => {
			let mut cursor = alias_type.walk();
			alias_type
				.named_children(&mut cursor)
				.find_map(type_alias_name_node)
		}
		_ => None,
	}
}

fn assignment_alias_name_node(left: Node<'_>) -> Option<Node<'_>> {
	match left.kind() {
		"identifier" => Some(left),
		"pattern" => {
			let mut cursor = left.walk();
			left.named_children(&mut cursor)
				.find_map(assignment_alias_name_node)
		}
		_ => None,
	}
}

fn type_annotation_has_name(node: Node<'_>, source: &[u8], expected: &[u8]) -> bool {
	if node.kind() == "identifier" && node_slice(node, source) == expected {
		return true;
	}
	if node.kind() == "attribute" && last_attribute(node, source).as_bytes() == expected {
		return true;
	}
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.any(|child| type_annotation_has_name(child, source, expected))
}

fn parameter_name_and_type<'tree>(
	param: Node<'tree>,
) -> (Option<Node<'tree>>, Option<Node<'tree>>) {
	match param.kind() {
		"identifier" => (Some(param), None),
		"default_parameter" => (param.child_by_field_name("name"), None),
		"typed_parameter" => {
			let ty = param.child_by_field_name("type");
			let mut cursor = param.walk();
			let mut name = None;
			for c in param.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					name = Some(c);
					break;
				}
				if matches!(c.kind(), "list_splat_pattern" | "dictionary_splat_pattern") {
					let mut pattern_cursor = c.walk();
					name = c
						.named_children(&mut pattern_cursor)
						.find(|child| child.kind() == "identifier");
					break;
				}
			}
			(name, ty)
		}
		"typed_default_parameter" => (
			param.child_by_field_name("name"),
			param.child_by_field_name("type"),
		),
		"list_splat_pattern" | "dictionary_splat_pattern" => {
			let mut cursor = param.walk();
			let mut name = None;
			for c in param.named_children(&mut cursor) {
				if c.kind() == "identifier" {
					name = Some(c);
					break;
				}
			}
			(name, None)
		}
		_ => (None, None),
	}
}

fn parameter_has_pattern(node: Node<'_>, expected: &str) -> bool {
	if node.kind() == expected {
		return true;
	}
	let mut cursor = node.walk();
	node.named_children(&mut cursor)
		.any(|child| parameter_has_pattern(child, expected))
}

fn call_argument_count(call: Node<'_>) -> usize {
	let Some(arguments) = call.child_by_field_name("arguments") else {
		return 0;
	};
	let mut cursor = arguments.walk();
	arguments.named_children(&mut cursor).count()
}

fn collect_param_slots(function: Node<'_>, source: &[u8], is_method: bool) -> Vec<CallableSlot> {
	let Some(params) = function.child_by_field_name("parameters") else {
		return Vec::new();
	};
	let mut out: Vec<CallableSlot> = Vec::new();
	let mut cursor = params.walk();
	let mut idx = 0usize;
	for child in params.named_children(&mut cursor) {
		let (name_node, type_node) = parameter_name_and_type(child);
		let Some(name_node) = name_node else { continue };
		let Ok(name_str) = name_node.utf8_text(source) else {
			continue;
		};
		if is_method && idx == 0 && (name_str == "self" || name_str == "cls") {
			idx += 1;
			continue;
		}
		idx += 1;
		let r#type = type_node
			.and_then(|t| t.utf8_text(source).ok())
			.map(crate::lang::callable::normalize_type_text)
			.unwrap_or_default();
		out.push(CallableSlot {
			name: name_str.as_bytes().to_vec(),
			r#type,
		});
	}
	out
}

fn last_attribute<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
	if let Some(attr) = node.child_by_field_name("attribute") {
		return attr.utf8_text(source).unwrap_or("");
	}
	""
}

fn receiver_hint<'a>(obj: Node<'_>, source: &'a [u8]) -> &'a [u8] {
	use crate::lang::kinds::{HINT_CALL, HINT_CLS, HINT_MEMBER, HINT_SELF, HINT_SUBSCRIPT};
	match obj.kind() {
		"identifier" => match obj.utf8_text(source).unwrap_or("") {
			"self" => HINT_SELF,
			"cls" => HINT_CLS,
			other => other.as_bytes(),
		},
		"attribute" => HINT_MEMBER,
		"call" => HINT_CALL,
		"subscript" => HINT_SUBSCRIPT,
		_ => b"",
	}
}

fn dotted_pieces<'a>(node: Node<'_>, source: &'a [u8]) -> Vec<&'a str> {
	let mut out = Vec::new();
	let mut cursor = node.walk();
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier"
			&& let Ok(s) = c.utf8_text(source)
		{
			out.push(s);
		}
	}
	out
}

fn relative_import_pieces<'a>(node: Node<'_>, source: &'a [u8]) -> (Vec<&'a str>, usize) {
	let mut leading_dots = 0usize;
	let mut pieces: Vec<&str> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children(&mut cursor) {
		match c.kind() {
			"import_prefix" => {
				leading_dots = import_prefix_dot_count(c);
			}
			"dotted_name" => {
				pieces = dotted_pieces(c, source);
			}
			_ => {}
		}
	}
	(pieces, leading_dots)
}

fn has_wildcard_import(node: Node<'_>) -> bool {
	let mut cursor = node.walk();
	node.children(&mut cursor)
		.any(|child| child.kind() == "wildcard_import")
}

fn import_module_path_and_alias<'tree, 'src>(
	node: Node<'tree>,
	source: &'src [u8],
) -> Option<(Node<'tree>, &'src str)> {
	match node.kind() {
		"aliased_import" => {
			let path_node = node.child_by_field_name("name")?;
			let alias = node
				.child_by_field_name("alias")
				.and_then(|name| name.utf8_text(source).ok())
				.unwrap_or("");
			Some((path_node, alias))
		}
		_ => Some((node, "")),
	}
}

fn import_prefix_dot_count(node: Node<'_>) -> usize {
	let mut count = 0usize;
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() == "." {
			count += 1;
		}
	}
	count
}

fn collect_from_import_names<'src>(
	node: Node<'_>,
	source: &'src [u8],
) -> Vec<(&'src str, &'src str)> {
	let mut out: Vec<(&'src str, &'src str)> = Vec::new();
	let mut cursor = node.walk();
	for c in node.children_by_field_name("name", &mut cursor) {
		match c.kind() {
			"dotted_name" => {
				let leaf = dotted_leaf(c, source);
				if !leaf.is_empty() {
					out.push((leaf, ""));
				}
			}
			"aliased_import" => {
				let name_node = c.child_by_field_name("name");
				let alias = c
					.child_by_field_name("alias")
					.and_then(|n| n.utf8_text(source).ok())
					.unwrap_or("");
				let leaf = match name_node {
					Some(n) if n.kind() == "dotted_name" => dotted_leaf(n, source),
					Some(n) => n.utf8_text(source).unwrap_or(""),
					None => "",
				};
				if !leaf.is_empty() {
					out.push((leaf, alias));
				}
			}
			_ => {}
		}
	}
	out
}

fn dotted_leaf<'src>(node: Node<'_>, source: &'src [u8]) -> &'src str {
	let mut cursor = node.walk();
	let mut last = "";
	for c in node.named_children(&mut cursor) {
		if c.kind() == "identifier"
			&& let Ok(s) = c.utf8_text(source)
		{
			last = s;
		}
	}
	last
}

fn build_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	confidence: &[u8],
) -> Moniker {
	let project = importer.as_view().project();
	if leading_dots > 0 {
		return build_relative_module_target(importer, pieces, leading_dots);
	}
	if pieces.is_empty() {
		let mut b = MonikerBuilder::new();
		b.project(project);
		return b.build();
	}
	if confidence == kinds::CONF_IMPORTED {
		let mut b = MonikerBuilder::new();
		b.project(project);
		b.segment(crate::lang::kinds::LANG, b"python");
		let last = pieces.len() - 1;
		for (i, p) in pieces.iter().enumerate() {
			let kind = if i == last {
				kinds::MODULE
			} else {
				kinds::PACKAGE
			};
			b.segment(kind, p.as_bytes());
		}
		return b.build();
	}
	let mut b = MonikerBuilder::new();
	b.project(project);
	b.segment(kinds::EXTERNAL_PKG, pieces[0].as_bytes());
	for p in &pieces[1..] {
		b.segment(kinds::PATH, p.as_bytes());
	}
	b.build()
}

fn build_relative_module_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
) -> Moniker {
	let view = importer.as_view();
	let depth = view.segment_count() as usize;
	let keep = depth
		.saturating_sub(1)
		.saturating_sub(leading_dots.saturating_sub(1));
	if keep == 0 {
		let mut b = MonikerBuilder::new();
		b.project(view.project());
		let head = ".".repeat(leading_dots);
		b.segment(kinds::EXTERNAL_PKG, head.as_bytes());
		for p in pieces {
			b.segment(kinds::PATH, p.as_bytes());
		}
		return b.build();
	}
	let mut b = MonikerBuilder::from_view(view);
	b.truncate(keep);
	if pieces.is_empty() {
		return b.build();
	}
	let last = pieces.len() - 1;
	for (i, p) in pieces.iter().enumerate() {
		let kind = if i == last {
			kinds::MODULE
		} else {
			kinds::PACKAGE
		};
		b.segment(kind, p.as_bytes());
	}
	b.build()
}

fn build_imported_symbol_target(
	importer: &Moniker,
	pieces: &[&str],
	leading_dots: usize,
	name: &[u8],
	confidence: &[u8],
) -> Moniker {
	let module = build_module_target(importer, pieces, leading_dots, confidence);
	let language_regime =
		leading_dots > 0 || (confidence == kinds::CONF_IMPORTED && !pieces.is_empty());
	if language_regime {
		extend_segment(&module, kinds::PATH, name)
	} else {
		extend_segment(&module, kinds::FUNCTION, name)
	}
}

fn external_or_imported(pieces: &[&str]) -> &'static [u8] {
	if pieces.is_empty() {
		return kinds::CONF_IMPORTED;
	}
	if STDLIB_PACKAGES.binary_search(&pieces[0]).is_ok() {
		return kinds::CONF_EXTERNAL;
	}
	kinds::CONF_IMPORTED
}

fn should_skip_type_name(name: &[u8]) -> bool {
	name.is_empty() || BUILTIN_TYPE_NAMES.binary_search(&name).is_ok()
}

fn is_inferable_builtin_type(name: &[u8]) -> bool {
	BUILTIN_TYPE_NAMES.binary_search(&name).is_ok() && !matches!(name, b"None" | b"TypeAlias")
}

fn type_ref_container(kind: &str) -> bool {
	matches!(
		kind,
		"type"
			| "subscript"
			| "generic_type"
			| "type_parameter"
			| "member_type"
			| "constrained_type"
			| "splat_type"
			| "tuple" | "list"
			| "union_type"
			| "binary_operator"
			| "expression_list"
			| "parenthesized_expression"
	)
}

fn is_typing_container(name: &[u8]) -> bool {
	TYPING_CONTAINER_NAMES.binary_search(&name).is_ok()
}

pub(crate) const PY_BUILTIN_NAMES: &[&[u8]] = &[
	b"ArithmeticError",
	b"AssertionError",
	b"AttributeError",
	b"BaseException",
	b"BaseExceptionGroup",
	b"BlockingIOError",
	b"BrokenPipeError",
	b"BufferError",
	b"BytesWarning",
	b"ChildProcessError",
	b"ConnectionAbortedError",
	b"ConnectionError",
	b"ConnectionRefusedError",
	b"ConnectionResetError",
	b"DeprecationWarning",
	b"EOFError",
	b"Ellipsis",
	b"EncodingWarning",
	b"EnvironmentError",
	b"Exception",
	b"ExceptionGroup",
	b"False",
	b"FileExistsError",
	b"FileNotFoundError",
	b"FloatingPointError",
	b"FutureWarning",
	b"GeneratorExit",
	b"IOError",
	b"ImportError",
	b"ImportWarning",
	b"IndentationError",
	b"IndexError",
	b"InterruptedError",
	b"IsADirectoryError",
	b"KeyError",
	b"KeyboardInterrupt",
	b"LookupError",
	b"MemoryError",
	b"ModuleNotFoundError",
	b"NameError",
	b"None",
	b"NotADirectoryError",
	b"NotImplemented",
	b"NotImplementedError",
	b"OSError",
	b"OverflowError",
	b"PendingDeprecationWarning",
	b"PermissionError",
	b"ProcessLookupError",
	b"RecursionError",
	b"ReferenceError",
	b"ResourceWarning",
	b"RuntimeError",
	b"RuntimeWarning",
	b"StopAsyncIteration",
	b"StopIteration",
	b"SyntaxError",
	b"SyntaxWarning",
	b"SystemError",
	b"SystemExit",
	b"TabError",
	b"TimeoutError",
	b"True",
	b"TypeError",
	b"UnboundLocalError",
	b"UnicodeDecodeError",
	b"UnicodeEncodeError",
	b"UnicodeError",
	b"UnicodeTranslateError",
	b"UnicodeWarning",
	b"UserWarning",
	b"ValueError",
	b"Warning",
	b"ZeroDivisionError",
	b"__import__",
	b"abs",
	b"aiter",
	b"all",
	b"anext",
	b"any",
	b"ascii",
	b"bin",
	b"bool",
	b"breakpoint",
	b"bytearray",
	b"bytes",
	b"callable",
	b"chr",
	b"classmethod",
	b"compile",
	b"complex",
	b"copyright",
	b"credits",
	b"delattr",
	b"dict",
	b"dir",
	b"divmod",
	b"enumerate",
	b"eval",
	b"exec",
	b"exit",
	b"filter",
	b"float",
	b"format",
	b"frozenset",
	b"getattr",
	b"globals",
	b"hasattr",
	b"hash",
	b"help",
	b"hex",
	b"id",
	b"input",
	b"int",
	b"isinstance",
	b"issubclass",
	b"iter",
	b"len",
	b"license",
	b"list",
	b"locals",
	b"map",
	b"max",
	b"memoryview",
	b"min",
	b"next",
	b"object",
	b"oct",
	b"open",
	b"ord",
	b"pow",
	b"print",
	b"property",
	b"quit",
	b"range",
	b"repr",
	b"reversed",
	b"round",
	b"set",
	b"setattr",
	b"slice",
	b"sorted",
	b"staticmethod",
	b"str",
	b"sum",
	b"super",
	b"tuple",
	b"type",
	b"vars",
	b"zip",
];

fn is_python_builtin(name: &[u8]) -> bool {
	PY_BUILTIN_NAMES.binary_search(&name).is_ok()
}

fn is_python_runtime_global(name: &[u8]) -> bool {
	matches!(
		name,
		b"__builtins__"
			| b"__cached__"
			| b"__file__"
			| b"__loader__"
			| b"__name__"
			| b"__package__"
			| b"__spec__"
	)
}

fn python_runtime_external_target(module: &Moniker, name: &[u8]) -> Moniker {
	let mut builder = MonikerBuilder::new();
	builder.project(module.as_view().project());
	builder.segment(kinds::EXTERNAL_PKG, b"python_runtime");
	builder.segment(kinds::PATH, name);
	builder.build()
}

fn builtin_external_target(module: &Moniker, name: &[u8]) -> Moniker {
	let mut b = MonikerBuilder::new();
	b.project(module.as_view().project());
	b.segment(kinds::EXTERNAL_PKG, b"builtins");
	b.segment(kinds::PATH, name);
	b.build()
}

const BUILTIN_TYPE_NAMES: &[&[u8]] = &[
	b"None",
	b"TypeAlias",
	b"bool",
	b"bytes",
	b"complex",
	b"dict",
	b"float",
	b"frozenset",
	b"int",
	b"list",
	b"object",
	b"set",
	b"str",
	b"tuple",
];

const TYPING_CONTAINER_NAMES: &[&[u8]] = &[
	b"Annotated",
	b"AsyncIterator",
	b"Awaitable",
	b"Callable",
	b"ClassVar",
	b"Final",
	b"FrozenSet",
	b"Iterable",
	b"Iterator",
	b"Literal",
	b"Mapping",
	b"MutableMapping",
	b"Optional",
	b"Protocol",
	b"Sequence",
	b"TypeAlias",
	b"Union",
];

pub(crate) const STDLIB_PACKAGES: &[&str] = &[
	"__future__",
	"abc",
	"aifc",
	"antigravity",
	"argparse",
	"array",
	"ast",
	"asynchat",
	"asyncio",
	"asyncore",
	"atexit",
	"audioop",
	"base64",
	"bdb",
	"binascii",
	"bisect",
	"builtins",
	"bz2",
	"cProfile",
	"calendar",
	"cgi",
	"cgitb",
	"chunk",
	"cmath",
	"cmd",
	"code",
	"codecs",
	"codeop",
	"collections",
	"colorsys",
	"compileall",
	"concurrent",
	"configparser",
	"contextlib",
	"contextvars",
	"copy",
	"copyreg",
	"crypt",
	"csv",
	"ctypes",
	"curses",
	"dataclasses",
	"datetime",
	"dbm",
	"decimal",
	"difflib",
	"dis",
	"distutils",
	"doctest",
	"email",
	"encodings",
	"ensurepip",
	"enum",
	"errno",
	"faulthandler",
	"fcntl",
	"filecmp",
	"fileinput",
	"fnmatch",
	"fractions",
	"ftplib",
	"functools",
	"gc",
	"genericpath",
	"getopt",
	"getpass",
	"gettext",
	"glob",
	"graphlib",
	"grp",
	"gzip",
	"hashlib",
	"heapq",
	"hmac",
	"html",
	"http",
	"idlelib",
	"imaplib",
	"imghdr",
	"imp",
	"importlib",
	"inspect",
	"io",
	"ipaddress",
	"itertools",
	"json",
	"keyword",
	"lib2to3",
	"linecache",
	"locale",
	"logging",
	"lzma",
	"mailbox",
	"mailcap",
	"marshal",
	"math",
	"mimetypes",
	"mmap",
	"modulefinder",
	"msilib",
	"msvcrt",
	"multiprocessing",
	"netrc",
	"nis",
	"nntplib",
	"nt",
	"ntpath",
	"nturl2path",
	"numbers",
	"opcode",
	"operator",
	"optparse",
	"os",
	"ossaudiodev",
	"pathlib",
	"pdb",
	"pickle",
	"pickletools",
	"pipes",
	"pkgutil",
	"platform",
	"plistlib",
	"poplib",
	"posix",
	"posixpath",
	"pprint",
	"profile",
	"pstats",
	"pty",
	"pwd",
	"py_compile",
	"pyclbr",
	"pydoc",
	"pydoc_data",
	"pyexpat",
	"queue",
	"quopri",
	"random",
	"re",
	"readline",
	"reprlib",
	"resource",
	"rlcompleter",
	"runpy",
	"sched",
	"secrets",
	"select",
	"selectors",
	"shelve",
	"shlex",
	"shutil",
	"signal",
	"site",
	"smtpd",
	"smtplib",
	"sndhdr",
	"socket",
	"socketserver",
	"spwd",
	"sqlite3",
	"sre_compile",
	"sre_constants",
	"sre_parse",
	"ssl",
	"stat",
	"statistics",
	"string",
	"stringprep",
	"struct",
	"subprocess",
	"sunau",
	"symtable",
	"sys",
	"sysconfig",
	"syslog",
	"tabnanny",
	"tarfile",
	"telnetlib",
	"tempfile",
	"termios",
	"textwrap",
	"this",
	"threading",
	"time",
	"timeit",
	"tkinter",
	"token",
	"tokenize",
	"tomllib",
	"trace",
	"traceback",
	"tracemalloc",
	"tty",
	"turtle",
	"turtledemo",
	"types",
	"typing",
	"unicodedata",
	"unittest",
	"urllib",
	"uu",
	"uuid",
	"venv",
	"warnings",
	"wave",
	"weakref",
	"webbrowser",
	"winreg",
	"winsound",
	"wsgiref",
	"xdrlib",
	"xml",
	"xmlrpc",
	"zipapp",
	"zipfile",
	"zipimport",
	"zlib",
	"zoneinfo",
];

fn visibility_from_name(name: &[u8]) -> &'static [u8] {
	if name.len() >= 4 && name.starts_with(b"__") && name.ends_with(b"__") {
		return kinds::VIS_PUBLIC;
	}
	if name.starts_with(b"__") {
		return kinds::VIS_PRIVATE;
	}
	if name.starts_with(b"_") {
		return kinds::VIS_MODULE;
	}
	kinds::VIS_PUBLIC
}
fn namespace_for_kind(kind: &[u8]) -> Namespace {
	if kind == kinds::CLASS || kind == kinds::TYPE {
		Namespace::Type
	} else if kind == kinds::FUNCTION || kind == kinds::ASYNC_FUNCTION || kind == kinds::METHOD {
		Namespace::Value
	} else {
		Namespace::Unified
	}
}

fn static_visibility(value: &[u8]) -> &'static [u8] {
	if value == kinds::VIS_PUBLIC {
		kinds::VIS_PUBLIC
	} else if value == kinds::VIS_PRIVATE {
		kinds::VIS_PRIVATE
	} else if value == kinds::VIS_MODULE {
		kinds::VIS_MODULE
	} else {
		b""
	}
}

fn static_confidence(value: &[u8]) -> &'static [u8] {
	if value == kinds::CONF_RESOLVED {
		kinds::CONF_RESOLVED
	} else if value == kinds::CONF_LOCAL {
		kinds::CONF_LOCAL
	} else if value == kinds::CONF_IMPORTED {
		kinds::CONF_IMPORTED
	} else if value == kinds::CONF_EXTERNAL {
		kinds::CONF_EXTERNAL
	} else if value == kinds::CONF_UNRESOLVED {
		kinds::CONF_UNRESOLVED
	} else {
		kinds::CONF_NAME_MATCH
	}
}

fn namespace_for_ref(kind: &[u8]) -> Namespace {
	if kind == kinds::USES_TYPE || kind == kinds::EXTENDS || kind == kinds::INSTANTIATES {
		Namespace::Type
	} else {
		Namespace::Value
	}
}

fn def_call_metadata(
	kind: &'static [u8],
	_name: &[u8],
	attrs: &DefAttrs<'_>,
) -> (Vec<u8>, Option<usize>) {
	if !attrs.call_name.is_empty() || attrs.call_arity.is_some() {
		return (attrs.call_name.to_vec(), attrs.call_arity);
	}
	if !is_python_callable_kind(kind) {
		return (Vec::new(), None);
	}
	(Vec::new(), None)
}

fn ref_call_metadata(
	kind: &'static [u8],
	_target: &Moniker,
	attrs: &RefAttrs<'_>,
) -> (Vec<u8>, Option<usize>) {
	if !attrs.call_name.is_empty() || attrs.call_arity.is_some() {
		return (attrs.call_name.to_vec(), attrs.call_arity);
	}
	if !matches!(
		kind,
		kinds::CALLS | kinds::METHOD_CALL | kinds::INSTANTIATES
	) {
		return (Vec::new(), None);
	}
	(Vec::new(), None)
}

fn is_python_callable_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::FUNCTION | kinds::ASYNC_FUNCTION | kinds::METHOD
	)
}

fn is_callable_scope(scope: &Moniker, module: &Moniker) -> bool {
	if scope == module {
		return false;
	}
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::FUNCTION || last.kind == kinds::ASYNC_FUNCTION || last.kind == kinds::METHOD
}

fn is_class_scope(scope: &Moniker) -> bool {
	let Some(last) = scope.as_view().segments().last() else {
		return false;
	};
	last.kind == kinds::CLASS
}

pub(super) fn first_docstring<'src>(body: Node<'src>) -> Option<Node<'src>> {
	let mut cursor = body.walk();
	let first = body.named_children(&mut cursor).next()?;
	if first.kind() != "expression_statement" {
		return None;
	}
	let mut inner = first.walk();
	let expr = first.named_children(&mut inner).next()?;
	if matches!(expr.kind(), "string" | "concatenated_string") {
		Some(expr)
	} else {
		None
	}
}

fn emit_docstring_def(node: Node<'_>, parent: &Moniker, graph: &mut SdkBuilder) {
	let m =
		crate::lang::callable::extend_segment_u32(parent, kinds::COMMENT, node.start_byte() as u32);
	let _ = graph.add_def(m, kinds::COMMENT, parent, Some(node_position(node)));
}
