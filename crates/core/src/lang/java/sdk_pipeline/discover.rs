use std::collections::HashMap;

use tree_sitter::Node;

use crate::core::moniker::Moniker;
use crate::lang::sdk::{DiscoveredDef, ResolvedRef, TypeExpr};

use super::defs::collect_defs;
use super::imports::{ImportedSymbol, collect_imports};
use super::refs::collect_refs;

pub(super) type CallableTable = HashMap<(Moniker, Vec<u8>, usize), Vec<u8>>;
pub(super) type ReturnTypeTable = HashMap<(Moniker, Vec<u8>, usize), TypeExpr>;
pub(super) type FieldTypeTable = HashMap<(Moniker, Vec<u8>), TypeExpr>;

pub(super) struct DiscoveredJavaFile {
	pub root: Moniker,
	pub defs: Vec<DiscoveredDef>,
	pub refs: Vec<ResolvedRef>,
}

pub(super) struct JavaDiscover<'src> {
	pub(super) root: Moniker,
	pub(super) source: &'src [u8],
	pub(super) deep: bool,
	pub(super) defs: Vec<DiscoveredDef>,
	pub(super) refs: Vec<ResolvedRef>,
	pub(super) imports: Vec<ImportedSymbol>,
	pub(super) callables: CallableTable,
	pub(super) return_types: ReturnTypeTable,
	pub(super) field_types: FieldTypeTable,
	pub(super) type_table: HashMap<Vec<u8>, Moniker>,
}

impl<'src> JavaDiscover<'src> {
	pub fn run(
		root: Moniker,
		source: &'src [u8],
		deep: bool,
		root_node: Node<'_>,
	) -> DiscoveredJavaFile {
		let mut discover = Self {
			root: root.clone(),
			source,
			deep,
			defs: Vec::new(),
			refs: Vec::new(),
			imports: Vec::new(),
			callables: HashMap::new(),
			return_types: HashMap::new(),
			field_types: HashMap::new(),
			type_table: HashMap::new(),
		};
		collect_imports(&mut discover, root_node, &root);
		collect_defs(&mut discover, root_node, &root);
		collect_refs(&mut discover, root_node, &root);
		DiscoveredJavaFile {
			root,
			defs: discover.defs,
			refs: discover.refs,
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

	pub(super) fn push_ref(&mut self, reference: ResolvedRef) {
		if !self
			.refs
			.iter()
			.any(|existing| same_ref(existing, &reference))
		{
			self.refs.push(reference);
		}
	}
}

fn same_ref(left: &ResolvedRef, right: &ResolvedRef) -> bool {
	left.source == right.source
		&& left.target == right.target
		&& left.kind == right.kind
		&& left.position == right.position
		&& left.confidence == right.confidence
}
