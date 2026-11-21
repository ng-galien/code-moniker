use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use crate::core::code_graph::FunctionLinkage;
use crate::core::moniker::Moniker;
use crate::core::moniker::query::bare_callable_name;
use crate::lang::sdk::{DiscoveredDef, ResolvedRef, ResolvedRefDeduper, TypeExpr};

use super::super::Presets;
use super::defs::{collect_defs, predeclare_types};
use super::function_linkage::header_guard;
use super::refs::collect_refs;

pub(super) type CallableTable = HashMap<Vec<u8>, Vec<u8>>;
pub(super) type ReturnTypeTable = HashMap<Vec<u8>, TypeExpr>;
pub(super) type FieldTypeTable = HashMap<(Moniker, Vec<u8>), TypeExpr>;
pub(super) type VarTypeTable = HashMap<Vec<u8>, TypeExpr>;

pub(super) struct DiscoveredCFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
	pub function_linkage: HashMap<Moniker, FunctionLinkage>,
}

pub(super) struct CDiscover<'src> {
	pub(super) root: Moniker,
	pub(super) source: &'src [u8],
	pub(super) deep: bool,
	pub(super) defs: Vec<DiscoveredDef>,
	pub(super) refs: Vec<ResolvedRef>,
	pub(super) function_linkage: HashMap<Moniker, FunctionLinkage>,
	pub(super) internal_functions: HashSet<Vec<u8>>,
	pub(super) header_guard: Option<usize>,
	ref_deduper: ResolvedRefDeduper,
	pub(super) type_table: HashMap<Vec<u8>, Moniker>,
	pub(super) callables: CallableTable,
	pub(super) macros: CallableTable,
	pub(super) return_types: ReturnTypeTable,
	pub(super) field_types: FieldTypeTable,
	pub(super) var_types: VarTypeTable,
	pub(super) presets: Presets,
}

pub(super) fn discover_file(
	root: Moniker,
	source: &[u8],
	deep: bool,
	root_node: Node<'_>,
	presets: &Presets,
) -> DiscoveredCFile {
	let mut discover = CDiscover {
		root: root.clone(),
		source,
		deep,
		defs: Vec::new(),
		refs: Vec::new(),
		function_linkage: HashMap::new(),
		internal_functions: HashSet::new(),
		header_guard: header_guard(root_node, source),
		ref_deduper: ResolvedRefDeduper::default(),
		type_table: HashMap::new(),
		callables: CallableTable::new(),
		macros: CallableTable::new(),
		return_types: ReturnTypeTable::new(),
		field_types: FieldTypeTable::new(),
		var_types: VarTypeTable::new(),
		presets: presets.clone(),
	};
	predeclare_types(&mut discover, root_node, &root);
	collect_defs(&mut discover, root_node, &root);
	collect_refs(&mut discover, root_node, &root);
	discover.suppress_internal_signatures();
	DiscoveredCFile {
		root,
		defs: discover.defs,
		refs: discover.refs,
		function_linkage: discover.function_linkage,
	}
}

impl CDiscover<'_> {
	fn suppress_internal_signatures(&mut self) {
		if self.internal_functions.is_empty() {
			return;
		}
		for (moniker, linkage) in &mut self.function_linkage {
			if moniker.as_view().segments().last().is_some_and(|segment| {
				self.internal_functions
					.contains(bare_callable_name(segment.name))
			}) {
				linkage.signature = None;
			}
		}
	}

	pub(super) fn push_def(&mut self, def: DiscoveredDef) {
		if !self
			.defs
			.iter()
			.any(|existing| existing.moniker == def.moniker)
		{
			self.defs.push(def);
		}
	}

	pub(super) fn upsert_def(&mut self, def: DiscoveredDef) {
		if let Some(existing) = self
			.defs
			.iter_mut()
			.find(|existing| existing.moniker == def.moniker)
		{
			*existing = def;
		} else {
			self.defs.push(def);
		}
	}

	pub(super) fn push_ref(&mut self, reference: ResolvedRef) {
		self.ref_deduper.push(&mut self.refs, reference);
	}
}
