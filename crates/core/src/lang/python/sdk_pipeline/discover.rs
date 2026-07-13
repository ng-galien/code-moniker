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

pub(super) struct PyDiscover<'src> {
	pub(super) module: Moniker,
	pub(super) source_bytes: &'src [u8],
	pub(super) deep: bool,
	pub(super) imports: PyImportBindings,
	pub(super) locals: PyLocalScopes,
	pub(super) instance_attr_types: RefCell<HashMap<(Moniker, Vec<u8>), Moniker>>,
	pub(super) type_table: TypeTable,
	pub(super) callable_table: HashMap<(Moniker, Vec<u8>), CallableEntry>,
}

pub(super) type TypeTable = HashMap<Vec<u8>, Vec<Moniker>>;

#[derive(Clone)]
pub(super) struct CallableEntry {
	pub(super) kind: &'static [u8],
	pub(super) segment: Vec<u8>,
}

pub(super) struct DiscoveredPythonFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
}

pub(super) struct PyImportBindings {
	confidences: RefCell<HashMap<Vec<u8>, &'static [u8]>>,
	targets: RefCell<HashMap<Vec<u8>, Moniker>>,
}

impl PyImportBindings {
	fn new() -> Self {
		Self {
			confidences: RefCell::new(HashMap::new()),
			targets: RefCell::new(HashMap::new()),
		}
	}

	fn bind(&self, name: &[u8], confidence: &'static [u8]) {
		self.confidences
			.borrow_mut()
			.insert(name.to_vec(), confidence);
	}

	fn bind_target(&self, name: &[u8], target: &Moniker) {
		if name.is_empty() {
			return;
		}
		self.targets
			.borrow_mut()
			.insert(name.to_vec(), target.clone());
	}

	fn confidence_for(&self, name: &[u8]) -> Option<&'static [u8]> {
		self.confidences.borrow().get(name).copied()
	}

	fn target_for(&self, name: &[u8]) -> Option<Moniker> {
		self.targets.borrow().get(name).cloned()
	}
}

pub(super) struct PyLocalScopes {
	names: RefCell<Vec<HashSet<Vec<u8>>>>,
	types: RefCell<Vec<HashMap<Vec<u8>, Moniker>>>,
}

impl PyLocalScopes {
	fn new() -> Self {
		Self {
			names: RefCell::new(Vec::new()),
			types: RefCell::new(Vec::new()),
		}
	}

	fn push(&self) {
		self.names.borrow_mut().push(HashSet::new());
		self.types.borrow_mut().push(HashMap::new());
	}

	fn pop(&self) {
		self.names.borrow_mut().pop();
		self.types.borrow_mut().pop();
	}

	fn record_name(&self, name: &[u8]) {
		if let Some(top) = self.names.borrow_mut().last_mut() {
			top.insert(name.to_vec());
		}
	}

	fn is_name(&self, name: &[u8]) -> bool {
		self.names.borrow().iter().any(|frame| frame.contains(name))
	}

	fn record_type(&self, name: &[u8], target: Moniker) {
		if let Some(top) = self.types.borrow_mut().last_mut() {
			top.insert(name.to_vec(), target);
		}
	}

	fn lookup_type(&self, name: &[u8]) -> Option<Moniker> {
		self.types
			.borrow()
			.iter()
			.rev()
			.find_map(|frame| frame.get(name).cloned())
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
		let confidence = self
			.discover
			.imports
			.confidence_for(name)
			.or_else(|| name_confidence(self.discover, name))?;
		if confidence == kinds::CONF_LOCAL {
			return Some(CallResolution {
				target: extend_segment(self.scope, kinds::LOCAL, name),
				kind: kinds::CALLS,
				confidence,
				receiver_hint: Vec::new(),
				call_name: name.to_vec(),
				call_arity: Some(arity),
			});
		}
		if let Some(target) = self.discover.imports.target_for(name) {
			return Some(CallResolution {
				target,
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
			confidence: kinds::CONF_NAME_MATCH,
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
		let import_target = self.discover.imports.target_for(receiver_name)?;
		Some(CallResolution {
			target: extend_segment(&import_target, kinds::FUNCTION, name),
			kind: kinds::CALLS,
			confidence: self
				.discover
				.imports
				.confidence_for(receiver_name)
				.unwrap_or(kinds::CONF_NAME_MATCH),
			receiver_hint: hint.to_vec(),
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
		let mut cursor = node.walk();
		let targets: Vec<_> = node
			.children(&mut cursor)
			.filter(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
			.collect();
		for target in targets {
			self.emit_import_module(target, pos);
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

		if has_wildcard_import(node) {
			self.emit_ref(module_target, kinds::IMPORTS_MODULE, confidence, b"", pos);
			return;
		}

		for (name, alias) in collect_from_import_names(node, self.discover.source_bytes) {
			self.emit_imported_symbol(&module_import, name, alias, confidence, pos);
		}
	}

	fn emit_import_module(&mut self, node: Node<'_>, pos: Position) {
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
		self.discover.imports.bind(bind.as_bytes(), confidence);

		let target = build_module_target(&self.discover.module, &pieces, 0, confidence);
		self.discover.imports.bind_target(bind.as_bytes(), &target);
		self.emit_ref(
			target,
			kinds::IMPORTS_MODULE,
			confidence,
			alias.as_bytes(),
			pos,
		);
	}

	fn emit_imported_symbol(
		&mut self,
		module_import: &ModuleImport<'_>,
		name: &str,
		alias: &str,
		confidence: &'static [u8],
		pos: Position,
	) {
		let bind = if !alias.is_empty() { alias } else { name };
		self.discover.imports.bind(bind.as_bytes(), confidence);
		let target = build_imported_symbol_target(
			&self.discover.module,
			&module_import.pieces,
			module_import.leading_dots,
			name.as_bytes(),
			confidence,
		);
		self.discover.imports.bind_target(bind.as_bytes(), &target);
		self.emit_ref(
			target,
			kinds::IMPORTS_SYMBOL,
			confidence,
			alias.as_bytes(),
			pos,
		);
	}

	fn emit_ref(
		&mut self,
		target: Moniker,
		kind: &'static [u8],
		confidence: &'static [u8],
		alias: &[u8],
		pos: Position,
	) {
		let attrs = RefAttrs {
			confidence,
			alias,
			..RefAttrs::default()
		};
		let _ = self
			.graph
			.add_ref_attrs(self.scope, target, kind, Some(pos), &attrs);
	}
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
		after_symbol_body(self.discover, kind);
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
			instance_attr_types: RefCell::new(instance_attr_types),
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
	if let Some(rt) = node.child_by_field_name("return_type") {
		PyTypeRefs::new(discover, moniker).emit(rt, graph);
	}
	if let Some(params) = node.child_by_field_name("parameters") {
		emit_param_defs_and_types(discover, params, moniker, source, graph);
	}
}

fn after_symbol_body(discover: &PyDiscover<'_>, kind: &[u8]) {
	if kind == kinds::FUNCTION || kind == kinds::ASYNC_FUNCTION || kind == kinds::METHOD {
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
	let Some(body) = node.child_by_field_name("body") else {
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

fn handle_assignment(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
	graph: &mut SdkBuilder,
) {
	let inferred_type = node
		.child_by_field_name("type")
		.and_then(|typed| infer_type_target(discover, typed, scope));
	if let Some(typed) = node.child_by_field_name("type") {
		PyTypeRefs::new(discover, scope).emit(typed, graph);
	}
	let inside_callable = is_callable_scope(scope, &discover.module);
	if inside_callable && let Some(left) = node.child_by_field_name("left") {
		record_local_pattern(discover, left);
		record_assignment_type(
			discover,
			scope,
			left,
			node.child_by_field_name("right"),
			inferred_type,
		);
		if discover.deep {
			emit_local_pattern(discover, left, scope, graph);
		}
	}
	if !inside_callable && let Some(left) = node.child_by_field_name("left") {
		emit_binding_pattern(discover, left, scope, graph);
	}
	if let Some(right) = node.child_by_field_name("right") {
		discover.recurse_subtree(right, scope, graph);
	}
}

fn handle_for(discover: &PyDiscover<'_>, node: Node<'_>, scope: &Moniker, graph: &mut SdkBuilder) {
	if is_callable_scope(scope, &discover.module)
		&& let Some(left) = node.child_by_field_name("left")
	{
		record_local_pattern(discover, left);
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
		.confidence_for(name)
		.or_else(|| name_confidence(discover, name))?;
	let resolved_type =
		if confidence != kinds::CONF_LOCAL && discover.imports.confidence_for(name).is_none() {
			lookup_discovered_type(discover, scope, name)
		} else {
			None
		};
	let target = if confidence == kinds::CONF_LOCAL {
		extend_segment(scope, kinds::LOCAL, name)
	} else if let Some(import_target) = discover.imports.target_for(name) {
		import_target
	} else if let Some(type_target) = resolved_type.clone() {
		type_target
	} else {
		extend_segment(&discover.module, kinds::FUNCTION, name)
	};
	let confidence = if resolved_type.is_some() {
		kinds::CONF_RESOLVED
	} else {
		confidence
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
	inferred_type: Option<Moniker>,
) {
	let right_type = right.and_then(|node| infer_assignment_value_type(discover, node, scope));
	let target = inferred_type.or(right_type);
	match left.kind() {
		"identifier" => {
			let Some(target) = target else { return };
			let name = node_slice(left, discover.source_bytes);
			if !name.is_empty() {
				discover.locals.record_type(name, target);
			}
		}
		"attribute" => {
			let Some(target) = target else { return };
			let Some((class, attr)) = self_attr_key(discover, scope, left) else {
				return;
			};
			discover
				.instance_attr_types
				.borrow_mut()
				.entry((class, attr))
				.or_insert(target);
		}
		_ => {}
	}
}

fn infer_assignment_value_type(
	discover: &PyDiscover<'_>,
	node: Node<'_>,
	scope: &Moniker,
) -> Option<Moniker> {
	match node.kind() {
		"identifier" => discover
			.locals
			.lookup_type(node_slice(node, discover.source_bytes)),
		"call" => {
			let callee = node.child_by_field_name("function")?;
			match callee.kind() {
				"identifier" => {
					let name = node_slice(callee, discover.source_bytes);
					lookup_discovered_type(discover, scope, name)
						.or_else(|| external_callee_type(discover, name))
				}
				"attribute" => attribute_callee_type(discover, callee),
				_ => None,
			}
		}
		"await" => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor)
				.find_map(|child| infer_assignment_value_type(discover, child, scope))
		}
		_ => None,
	}
}

fn external_callee_type(discover: &PyDiscover<'_>, name: &[u8]) -> Option<Moniker> {
	let target = discover.imports.target_for(name)?;
	is_external_shaped(&target).then_some(target)
}

fn attribute_callee_type(discover: &PyDiscover<'_>, callee: Node<'_>) -> Option<Moniker> {
	let object = callee.child_by_field_name("object")?;
	if object.kind() != "identifier" {
		return None;
	}
	let object_name = node_slice(object, discover.source_bytes);
	let module_target = discover.imports.target_for(object_name)?;
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
		if let Some(type_node) = type_node
			&& let Some(target) = infer_type_target(discover, type_node, scope)
		{
			discover.locals.record_type(name, target);
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

fn collect_base_class_refs(
	discover: &PyDiscover<'_>,
	supers: Node<'_>,
	scope: &Moniker,
	out: &mut Vec<RefSpec>,
) {
	let mut cursor = supers.walk();
	for child in supers.named_children(&mut cursor) {
		let name = match base_class_name(child, discover.source_bytes) {
			Some(name) => name,
			None => continue,
		};
		let (target, confidence) = resolve_type_target(discover, scope, &name, kinds::CLASS);
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

fn base_class_name(node: Node<'_>, source: &[u8]) -> Option<Vec<u8>> {
	let name = match node.kind() {
		"identifier" => node_slice(node, source).to_vec(),
		"attribute" => last_attribute(node, source).as_bytes().to_vec(),
		"subscript" => match node.child_by_field_name("value") {
			Some(value) => match value.kind() {
				"identifier" => node_slice(value, source).to_vec(),
				"attribute" => last_attribute(value, source).as_bytes().to_vec(),
				_ => return None,
			},
			None => return None,
		},
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
		let (target, confidence) = resolve_type_target(discover, scope, &name, kinds::FUNCTION);
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
			if should_skip_type_name(name) || is_typing_container(name) {
				return None;
			}
			let (target, _) = resolve_type_target(discover, scope, name, kinds::CLASS);
			Some(target)
		}
		"attribute" => {
			let name = last_attribute(node, discover.source_bytes);
			if should_skip_type_name(name.as_bytes()) || is_typing_container(name.as_bytes()) {
				return None;
			}
			let (target, _) = resolve_type_target(discover, scope, name.as_bytes(), kinds::CLASS);
			Some(target)
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
				.find_map(|child| infer_type_target(discover, child, scope))
		}
		_ => None,
	}
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
	if let Some(m) = discover.imports.target_for(name) {
		let confidence = discover
			.imports
			.confidence_for(name)
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
		.confidence_for(name)
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
			confidence: kinds::CONF_IMPORTED,
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
			out.insert(
				(parent.clone(), name.to_vec()),
				CallableEntry { kind, segment: seg },
			);
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
				if matches!(
					c.kind(),
					"identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
				) {
					name = Some(c);
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
