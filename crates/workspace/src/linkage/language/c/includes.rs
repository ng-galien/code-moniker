// code-moniker: ignore-file[smell-feature-envy-local]
use std::collections::HashMap;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::kinds;
use roaring::RoaringBitmap;

use crate::linkage::catalog::{CandidateCatalog, LinkageQuery, SymbolSet, query_keys};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) struct CIncludeVisibility {
	visible_by_file: Vec<RoaringBitmap>,
	type_aliases: HashMap<Vec<u8>, Vec<CTypeAlias>>,
	unindexed_external_dependents: RoaringBitmap,
}

struct CTypeAlias {
	source_file: usize,
	target: (Vec<u8>, Vec<u8>),
}

impl CIncludeVisibility {
	pub(in crate::linkage) fn build(material: &CodeIndexMaterial) -> Self {
		let mut file_by_root = HashMap::<Moniker, usize>::new();
		for (file_idx, file) in material.files.iter().enumerate() {
			if file.lang == Lang::C {
				file_by_root.insert(file.graph.root().clone(), file_idx);
			}
		}
		let mut direct = vec![RoaringBitmap::new(); material.files.len()];
		let mut reverse = vec![RoaringBitmap::new(); material.files.len()];
		for (source_file, file) in material.files.iter().enumerate() {
			if file.lang != Lang::C {
				continue;
			}
			for reference in file.graph.refs() {
				if reference.kind != kinds::IMPORTS_MODULE {
					continue;
				}
				let Some(&target_file) = file_by_root.get(&reference.target) else {
					continue;
				};
				direct[source_file].insert(target_file as u32);
				reverse[target_file].insert(source_file as u32);
			}
		}

		let visible_by_file: Vec<RoaringBitmap> = (0..material.files.len())
			.map(|source_file| Self::visible_files(source_file, &direct, &reverse))
			.collect();
		let declared_external_dependencies: RoaringBitmap = material
			.files
			.iter()
			.enumerate()
			.filter(|(_, file)| {
				file.lang == Lang::C
					&& file.graph.refs().any(|reference| {
						reference.kind == kinds::IMPORTS_MODULE
							&& reference.receiver_hint == b"c_build_dependency"
					})
			})
			.map(|(file_idx, _)| file_idx as u32)
			.collect();
		let unindexed_external_dependents = visible_by_file
			.iter()
			.enumerate()
			.filter(|(_, visible)| {
				visible
					.iter()
					.any(|file| declared_external_dependencies.contains(file))
			})
			.map(|(file_idx, _)| file_idx as u32)
			.collect();
		Self {
			visible_by_file,
			type_aliases: Self::collect_type_aliases(material),
			unindexed_external_dependents,
		}
	}

	pub(in crate::linkage) fn candidates(
		&self,
		query: &LinkageQuery<'_>,
		catalog: &CandidateCatalog,
	) -> SymbolSet {
		let Some(visible) = self.visible_by_file.get(query.source_file) else {
			return SymbolSet::new();
		};
		let candidates = Self::matching_candidates(visible, query, catalog);
		if !candidates.is_empty() {
			return candidates;
		}
		let Some((owner, field)) = Self::field_owner(query.target) else {
			return candidates;
		};
		let Some(owner_name) = owner
			.as_view()
			.segments()
			.last()
			.map(|segment| segment.name)
		else {
			return candidates;
		};
		let mut aliased = SymbolSet::new();
		let Some(aliases) = self.type_aliases.get(owner_name) else {
			return candidates;
		};
		for alias in aliases {
			if !visible.contains(alias.source_file as u32) {
				continue;
			}
			let rewritten = Self::replace_owner(&owner, field, &alias.target);
			let rewritten_query = query.with_target(&rewritten);
			for symbol in Self::matching_candidates(visible, &rewritten_query, catalog).iter() {
				aliased.insert(symbol);
			}
		}
		aliased
	}

	pub(in crate::linkage) fn depends_on_unindexed_external(&self, source_file: usize) -> bool {
		self.unindexed_external_dependents
			.contains(source_file as u32)
	}

	pub(in crate::linkage) fn macros_named(
		&self,
		source_file: usize,
		name: &[u8],
		catalog: &CandidateCatalog,
	) -> SymbolSet {
		let mut macros = SymbolSet::new();
		let Some(visible) = self.visible_by_file.get(source_file) else {
			return macros;
		};
		for file in visible.iter() {
			let Some(candidates) = catalog.indexes().symbols_by_source_key(file as usize, name)
			else {
				continue;
			};
			for symbol in candidates.iter() {
				if catalog.candidate(symbol).is_some_and(|candidate| {
					candidate
						.last_segment
						.is_some_and(|segment| segment.kind == b"macro")
						&& candidate.call_name == Some(name)
				}) {
					macros.insert(symbol);
				}
			}
		}
		macros
	}

	fn matching_candidates(
		visible: &RoaringBitmap,
		query: &LinkageQuery<'_>,
		catalog: &CandidateCatalog,
	) -> SymbolSet {
		let mut candidates = SymbolSet::new();
		for key in query_keys(query) {
			for source_file in visible.iter() {
				let Some(symbols) = catalog
					.indexes()
					.symbols_by_source_key(source_file as usize, &key)
				else {
					continue;
				};
				for symbol in symbols.iter() {
					if catalog.candidate(symbol).is_some_and(|candidate| {
						super::matches_include_candidate(query, &candidate)
					}) {
						candidates.insert(symbol);
					}
				}
			}
		}
		candidates
	}

	fn collect_type_aliases(material: &CodeIndexMaterial) -> HashMap<Vec<u8>, Vec<CTypeAlias>> {
		let mut aliases = HashMap::<Vec<u8>, Vec<CTypeAlias>>::new();
		for (source_file, file) in material.files.iter().enumerate() {
			if file.lang != Lang::C {
				continue;
			}
			for reference in file.graph.refs() {
				if reference.kind != kinds::USES_TYPE {
					continue;
				}
				let source = &file.graph.def_at(reference.source).moniker;
				let Some(alias) = source.as_view().segments().last() else {
					continue;
				};
				let Some(target) = reference.target.as_view().segments().last() else {
					continue;
				};
				if alias.kind == kinds::TYPE
					&& matches!(target.kind, kinds::TYPE | b"struct" | b"enum")
				{
					aliases
						.entry(alias.name.to_vec())
						.or_default()
						.push(CTypeAlias {
							source_file,
							target: (target.kind.to_vec(), target.name.to_vec()),
						});
				}
			}
		}
		aliases
	}

	fn field_owner(target: &Moniker) -> Option<(Moniker, Segment<'_>)> {
		let field = target.as_view().segments().last()?;
		if field.kind != kinds::FIELD {
			return None;
		}
		Some((target.parent()?, field))
	}

	fn replace_owner(owner: &Moniker, field: Segment<'_>, target: &(Vec<u8>, Vec<u8>)) -> Moniker {
		let mut builder = MonikerBuilder::new();
		builder.project(owner.as_view().project());
		let segments = owner.as_view().segments().collect::<Vec<_>>();
		for (index, segment) in segments.iter().enumerate() {
			if index == segments.len() - 1 {
				builder.segment(&target.0, &target.1);
			} else {
				builder.segment(segment.kind, segment.name);
			}
		}
		builder.segment(field.kind, field.name).build()
	}

	fn visible_files(
		source_file: usize,
		direct: &[RoaringBitmap],
		reverse: &[RoaringBitmap],
	) -> RoaringBitmap {
		let contexts = Self::reachable(source_file, reverse);
		let mut visible = RoaringBitmap::new();
		for context in contexts.iter() {
			visible |= Self::reachable(context as usize, direct);
		}
		visible
	}

	fn reachable(start: usize, edges: &[RoaringBitmap]) -> RoaringBitmap {
		let mut reached = RoaringBitmap::new();
		let mut pending = vec![start];
		while let Some(current) = pending.pop() {
			if !reached.insert(current as u32) {
				continue;
			}
			if let Some(next) = edges.get(current) {
				pending.extend(next.iter().map(|file| file as usize));
			}
		}
		reached
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn included_fragment_sees_includer_sibling_headers() {
		let direct = vec![
			RoaringBitmap::from_iter([1, 2]),
			RoaringBitmap::new(),
			RoaringBitmap::new(),
		];
		let reverse = vec![
			RoaringBitmap::new(),
			RoaringBitmap::from_iter([0]),
			RoaringBitmap::from_iter([0]),
		];

		let visible = CIncludeVisibility::visible_files(2, &direct, &reverse);

		assert_eq!(visible.iter().collect::<Vec<_>>(), vec![0, 1, 2]);
	}
}
