use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::{Lang, kinds};
use rustc_hash::FxHashMap;

use crate::linkage::catalog::{CandidateCatalog, LinkageCandidate, LinkageQuery, SymbolSet};
use crate::linkage::resolve::resolve_global_scope;
use crate::source::CodeIndexMaterial;

#[derive(Default)]
pub(in crate::linkage) struct JavaOnDemandImports {
	by_source: FxHashMap<usize, Vec<OnDemandImport>>,
}

#[derive(Clone)]
enum OnDemandImport {
	Package(Moniker),
	StaticType(Moniker),
}

struct JavaImportedTarget {
	target: Moniker,
	requires_static: bool,
}

impl JavaOnDemandImports {
	pub(in crate::linkage) fn build(material: &CodeIndexMaterial) -> Self {
		let mut imports = Self::default();
		for (file_idx, file) in material.files.iter().enumerate() {
			if file.lang != Lang::Java {
				continue;
			}
			for reference in file.graph.refs() {
				if reference.kind != kinds::IMPORTS_MODULE || reference.alias.as_ref() != b"*" {
					continue;
				}
				let Some(last) = reference.target.as_view().segments().last() else {
					continue;
				};
				let import = if last.kind == kinds::MODULE {
					OnDemandImport::StaticType(reference.target.clone())
				} else if last.kind == kinds::PACKAGE {
					OnDemandImport::Package(reference.target.clone())
				} else {
					continue;
				};
				imports.by_source.entry(file_idx).or_default().push(import);
			}
		}
		imports
	}

	pub(in crate::linkage) fn matching_targets(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog,
	) -> SymbolSet {
		let mut targets = SymbolSet::new();
		if query
			.material
			.files
			.get(query.source_file)
			.is_none_or(|source| source.lang != Lang::Java)
		{
			return targets;
		}
		if query.confidence != Some("name_match") {
			return targets;
		}
		let Some(last) = query.target_last else {
			return targets;
		};
		let forwarded = self
			.by_source
			.get(&query.source_file)
			.into_iter()
			.flatten()
			.filter_map(|import| import.forward(query.reference_kind, last))
			.collect::<Vec<_>>();
		for imported in &forwarded {
			let forwarded_query = query.with_target(&imported.target);
			for symbol in resolve_global_scope(&forwarded_query, candidates).iter() {
				if imported.requires_static
					&& !candidates
						.candidate(symbol)
						.is_some_and(|candidate| candidate_is_static(&candidate))
				{
					continue;
				}
				targets.insert(symbol);
			}
		}
		targets
	}
}

impl OnDemandImport {
	fn forward(&self, reference_kind: &str, last: Segment<'_>) -> Option<JavaImportedTarget> {
		match self {
			Self::Package(package) if is_type_reference(reference_kind) => {
				let mut builder = MonikerBuilder::from_view(package.as_view());
				builder.segment(kinds::MODULE, last.name);
				builder.segment(kinds::PATH, last.name);
				Some(JavaImportedTarget {
					target: builder.build(),
					requires_static: false,
				})
			}
			Self::StaticType(owner) if is_static_member_reference(reference_kind) => {
				let owner_name = owner.as_view().segments().last()?.name;
				let mut builder = MonikerBuilder::from_view(owner.as_view());
				builder.segment(kinds::CLASS, owner_name);
				builder.segment(last.kind, last.name);
				Some(JavaImportedTarget {
					target: builder.build(),
					requires_static: true,
				})
			}
			_ => None,
		}
	}
}

fn is_type_reference(kind: &str) -> bool {
	matches!(
		kind.as_bytes(),
		kinds::ANNOTATES
			| kinds::EXTENDS
			| kinds::IMPLEMENTS
			| kinds::INSTANTIATES
			| kinds::RETURNS_TYPE
			| kinds::TYPED_AS
			| kinds::USES_TYPE
	)
}

fn is_static_member_reference(kind: &str) -> bool {
	matches!(kind.as_bytes(), kinds::CALLS | kinds::READS | kinds::WRITES)
}

fn candidate_is_static(candidate: &LinkageCandidate<'_>) -> bool {
	if candidate.signature == b"static" || candidate.signature.starts_with(b"static ") {
		return true;
	}
	let segments = candidate.moniker.as_view().segments().collect::<Vec<_>>();
	let [.., owner, member] = segments.as_slice() else {
		return false;
	};
	member.kind == kinds::ENUM_CONSTANT
		|| (member.kind == kinds::FIELD
			&& matches!(owner.kind, kinds::INTERFACE | kinds::ANNOTATION_TYPE))
}
