// code-moniker: ignore-file[smell-feature-envy-local]
use std::collections::HashMap;
use std::path::Path;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::Lang;
use code_moniker_core::lang::c::UNRESOLVED_ANGLE_INCLUDE;
use code_moniker_core::lang::kinds;
use roaring::RoaringBitmap;

use crate::linkage::catalog::{CandidateCatalog, LinkageQuery, SymbolSet, query_keys};
use crate::linkage::source_groups::{LinkPermission, SourceGroupPolicy};
use crate::source::CodeIndexMaterial;
use crate::sources::conventional_include;

pub(in crate::linkage) struct CIncludeVisibility {
	visible_by_file: Vec<RoaringBitmap>,
	pub(super) topology: CIncludeTopology,
	type_aliases: HashMap<Vec<u8>, Vec<CTypeAlias>>,
	unindexed_external_dependents: RoaringBitmap,
}

pub(super) struct CIncludeTopology {
	forward_by_file: Vec<RoaringBitmap>,
	direct: Vec<RoaringBitmap>,
	angle_targets: HashMap<(usize, usize), usize>,
}

impl CIncludeTopology {
	pub(super) fn angle_target(&self, file: usize, reference: usize) -> Option<usize> {
		self.angle_targets.get(&(file, reference)).copied()
	}

	pub(super) fn directly_includes(&self, source: usize, header: usize) -> bool {
		self.direct
			.get(source)
			.is_some_and(|targets| targets.contains(header as u32))
	}
}

struct CTypeAlias {
	source_file: usize,
	target: (Vec<u8>, Vec<u8>),
}

impl CIncludeVisibility {
	pub(in crate::linkage) fn build(material: &CodeIndexMaterial) -> Self {
		let (direct, reverse, angle_targets) = include_edges(material);
		let forward_by_file = (0..material.files.len())
			.map(|file| Self::reachable(file, &direct))
			.collect::<Vec<_>>();
		let visible_by_file = visible_files(&direct, &reverse, &forward_by_file);
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
			.filter(|(_, visible)| !visible.is_disjoint(&declared_external_dependencies))
			.map(|(file_idx, _)| file_idx as u32)
			.collect();
		Self {
			visible_by_file,
			topology: CIncludeTopology {
				forward_by_file,
				direct,
				angle_targets,
			},
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
		let Some(visible) = self.visible_by_file.get(source_file) else {
			return SymbolSet::new();
		};
		catalog
			.indexes()
			.symbols_by_language_key(Lang::C, name)
			.into_iter()
			.flat_map(|symbols| symbols.iter())
			.filter(|symbol| {
				catalog.candidate(*symbol).is_some_and(|candidate| {
					visible.contains(candidate.source_file as u32)
						&& candidate
							.last_segment
							.is_some_and(|segment| segment.kind == b"macro")
						&& candidate.call_name == Some(name)
				})
			})
			.collect()
	}

	fn matching_candidates(
		visible: &RoaringBitmap,
		query: &LinkageQuery<'_>,
		catalog: &CandidateCatalog,
	) -> SymbolSet {
		let mut candidates = SymbolSet::new();
		let direct_call = query.reference_kind == "calls"
			&& query
				.target_last
				.is_some_and(|segment| segment.kind == kinds::FUNC);
		let indexes = catalog.indexes();
		for key in query_keys(query) {
			let globals = indexes.symbols_by_language_key(Lang::C, &key);
			let lexical = if direct_call {
				None
			} else {
				indexes.c_lexical_symbols_by_key(&key)
			};
			let named_count = globals.map_or(0, SymbolSet::len) + lexical.map_or(0, SymbolSet::len);
			// Frequent names can have more definitions than this file has visible headers.
			if direct_call || named_count as u64 <= visible.len() {
				for symbol in globals.into_iter().chain(lexical).flat_map(SymbolSet::iter) {
					if catalog.candidate(symbol).is_some_and(|candidate| {
						visible.contains(candidate.source_file as u32)
							&& super::matches_include_candidate(query, &candidate)
					}) {
						candidates.insert(symbol);
					}
				}
				continue;
			}
			for source_file in visible.iter() {
				let Some(symbols) = indexes.symbols_by_source_key(source_file as usize, &key)
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

	fn reachable(start: usize, edges: &[RoaringBitmap]) -> RoaringBitmap {
		let mut reached = RoaringBitmap::from_iter([start as u32]);
		let mut pending = vec![start];
		while let Some(current) = pending.pop() {
			if let Some(next) = edges.get(current) {
				pending.extend(
					next.iter()
						.filter(|file| reached.insert(*file))
						.map(|file| file as usize),
				);
			}
		}
		reached
	}
}

fn visible_files(
	direct: &[RoaringBitmap],
	reverse: &[RoaringBitmap],
	forward: &[RoaringBitmap],
) -> Vec<RoaringBitmap> {
	let (components, component_of) = include_components(direct, reverse);
	let mut visible: Vec<_> = components
		.iter()
		.map(|members| forward[members[0]].clone())
		.collect();
	// Each component inherits the visibility of its includers. A cycle has
	// one shared forward closure, so each edge between components is sufficient.
	for (component, members) in components.iter().enumerate() {
		let (earlier, later) = visible.split_at_mut(component + 1);
		for &file in members {
			for target in direct[file].iter() {
				let next = component_of[target as usize];
				if next != component {
					later[next - component - 1] |= &earlier[component];
				}
			}
		}
	}
	component_of
		.into_iter()
		.map(|component| visible[component].clone())
		.collect()
}

// Kosaraju's two passes yield strongly connected components in topological
// order. Both walks are iterative so chains of includes do not grow the stack.
fn include_components(
	direct: &[RoaringBitmap],
	reverse: &[RoaringBitmap],
) -> (Vec<Vec<usize>>, Vec<usize>) {
	let mut seen = RoaringBitmap::new();
	let mut finished = Vec::with_capacity(direct.len());
	let mut walk = Vec::new();
	for file in 0..direct.len() {
		walk.push((file, false));
		while let Some((node, expanded)) = walk.pop() {
			if expanded {
				finished.push(node);
			} else if seen.insert(node as u32) {
				walk.push((node, true));
				walk.extend(
					direct[node]
						.iter()
						.map(|neighbor| (neighbor as usize, false)),
				);
			}
		}
	}

	let mut components = Vec::new();
	let mut component_of = vec![usize::MAX; direct.len()];
	let mut pending = Vec::new();
	for file in finished.into_iter().rev() {
		if component_of[file] != usize::MAX {
			continue;
		}
		let component = components.len();
		let mut members = Vec::new();
		component_of[file] = component;
		pending.push(file);
		while let Some(node) = pending.pop() {
			members.push(node);
			for neighbor in reverse[node].iter() {
				let neighbor = neighbor as usize;
				if component_of[neighbor] == usize::MAX {
					component_of[neighbor] = component;
					pending.push(neighbor);
				}
			}
		}
		components.push(members);
	}
	(components, component_of)
}

// The ordinary preprocessor pass handles competing macro invocations.
pub(super) fn included_c_calls(
	visibility: &CIncludeVisibility,
	query: &LinkageQuery<'_>,
	catalog: &CandidateCatalog,
) -> Option<SymbolSet> {
	let visible = visibility.topology.forward_by_file.get(query.source_file)?;
	let name = query.call_name?.as_bytes();
	let Some(named) = catalog.indexes().symbols_by_language_key(Lang::C, name) else {
		return Some(SymbolSet::new());
	};
	let mut targets = SymbolSet::new();
	for symbol in named.iter() {
		let Some(candidate) = catalog.candidate(symbol) else {
			continue;
		};
		if !visible.contains(candidate.source_file as u32) {
			continue;
		}
		match candidate.last_segment.map(|segment| segment.kind) {
			Some(b"macro") if candidate.call_name == Some(name) => return None,
			Some(kinds::FUNC)
				if candidate.source_file != query.source_file
					&& super::matches_include_candidate(query, &candidate) =>
			{
				targets.insert(symbol);
			}
			_ => {}
		}
	}
	Some(targets)
}

type IncludeEdges = (
	Vec<RoaringBitmap>,
	Vec<RoaringBitmap>,
	HashMap<(usize, usize), usize>,
);

fn include_edges(material: &CodeIndexMaterial) -> IncludeEdges {
	let sources = &material.source_catalog.sources;
	let mut file_by_root = HashMap::new();
	let mut file_by_path = HashMap::new();
	for (file_idx, file) in material.files.iter().enumerate() {
		if file.lang == Lang::C && !sources.files[file_idx].retired {
			let path = file.rel_path.as_path();
			let local_path = if sources.multi {
				path.strip_prefix(&sources.roots[file.source_root].label)
					.unwrap_or(path)
			} else {
				path
			};
			file_by_root.insert(file.graph.root(), file_idx);
			file_by_path.insert((file.source_root, local_path), file_idx);
		}
	}
	let mut direct = vec![RoaringBitmap::new(); material.files.len()];
	let mut reverse = vec![RoaringBitmap::new(); material.files.len()];
	let mut angle_targets = HashMap::new();
	let mut lookup = HashMap::new();
	let groups = SourceGroupPolicy::build(material);
	for (source, file) in material
		.files
		.iter()
		.enumerate()
		.filter(|(_, file)| file.lang == Lang::C)
	{
		for (idx, reference) in file.graph.refs().enumerate() {
			if reference.kind != kinds::IMPORTS_MODULE {
				continue;
			}
			let explicit = file_by_root.get(&reference.target).copied();
			let target = explicit.or_else(|| {
				let raw = std::str::from_utf8(&reference.receiver_hint)
					.ok()?
					.strip_prefix(UNRESOLVED_ANGLE_INCLUDE)?;
				*lookup.entry((file.source_root, raw)).or_insert_with(|| {
					let path = conventional_include(Path::new(raw))?;
					file_by_path
						.get(&(file.source_root, path.as_path()))
						.copied()
				})
			});
			if let Some(target) = target
				&& groups.link_permission(material, source, target) != Some(LinkPermission::Blocked)
			{
				if explicit.is_none() {
					angle_targets.insert((source, idx), target);
				}
				direct[source].insert(target as u32);
				reverse[target].insert(source as u32);
			}
		}
	}
	(direct, reverse, angle_targets)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::code::{CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions};
	use crate::snapshot::WorkspaceRequest;
	use crate::source::{
		LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort,
	};

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

		let forward = (0..direct.len())
			.map(|file| CIncludeVisibility::reachable(file, &direct))
			.collect::<Vec<_>>();
		let visible = visible_files(&direct, &reverse, &forward);

		assert_eq!(visible[2].iter().collect::<Vec<_>>(), vec![0, 1, 2]);
	}

	#[test]
	fn shared_visibility_matches_individual_closures() {
		let check = |direct: Vec<RoaringBitmap>| {
			let mut reverse = vec![RoaringBitmap::new(); direct.len()];
			for (file, targets) in direct.iter().enumerate() {
				for target in targets.iter() {
					reverse[target as usize].insert(file as u32);
				}
			}
			let forward: Vec<_> = (0..direct.len())
				.map(|file| CIncludeVisibility::reachable(file, &direct))
				.collect();
			let visible = visible_files(&direct, &reverse, &forward);
			for (file, visible) in visible.into_iter().enumerate() {
				let mut original = RoaringBitmap::new();
				for context in CIncludeVisibility::reachable(file, &reverse).iter() {
					original |= &forward[context as usize];
				}
				assert_eq!(visible, original, "file {file} in {direct:?}");
			}
		};
		for edges in 0..(1 << 9) {
			check(
				(0..3)
					.map(|file| {
						(0..3)
							.filter(|target| edges & (1 << (file * 3 + target)) != 0)
							.collect()
					})
					.collect(),
			);
		}
		// A diamond and a cycle with two entry points and an isolated file.
		check(
			[&[1, 2][..], &[3], &[3], &[4], &[3], &[2, 6], &[], &[]]
				.into_iter()
				.map(|targets| targets.iter().copied().collect())
				.collect(),
		);
	}

	#[test]
	fn indexed_include_lookup_preserves_lexical_symbols_after_refresh() {
		let temp = tempfile::tempdir().unwrap();
		let root = temp.path();
		let header = root.join("demo.h");
		let original = "static inline int step(int arg) { int local = arg; return local; }\n";
		std::fs::write(root.join("client.c"), "#include \"demo.h\"\n").unwrap();
		std::fs::write(root.join("other.h"), original).unwrap();
		std::fs::write(&header, original).unwrap();
		let cache = LocalResourceCache::default();
		let options = LocalSourceCatalogOptions::new(vec![root.to_path_buf()], None);
		let source_catalog = LocalSourceCatalog::new(options, cache.clone())
			.load_catalog(&WorkspaceRequest::new("c-lexical-candidates"))
			.unwrap();
		let mut port = LocalCodeIndex::new(LocalCodeIndexOptions::default(), cache.clone());
		let index = port.build_index(&source_catalog).unwrap();
		let material = cache.index_material(index.generation).unwrap();
		let mut catalog = CandidateCatalog::new(&material, index.inventory.catalog().clone());
		let source = material
			.files
			.iter()
			.position(|file| file.rel_path == Path::new("client.c"))
			.unwrap();
		let included = material
			.files
			.iter()
			.position(|file| file.rel_path == Path::new("demo.h"))
			.unwrap();
		let symbols = |material: &CodeIndexMaterial| {
			material.files[included]
				.graph
				.defs()
				.enumerate()
				.filter(|(_, def)| matches!(def.kind.as_ref(), kinds::LOCAL | kinds::PARAM))
				.map(|(idx, def)| (idx, def.moniker.clone()))
				.collect::<Vec<_>>()
		};
		let matching =
			|material: &CodeIndexMaterial, catalog: &CandidateCatalog, target: &Moniker| {
				let visibility = CIncludeVisibility::build(material);
				let visible = &visibility.visible_by_file[source];
				assert_eq!(
					visible.iter().collect::<Vec<_>>(),
					vec![source as u32, included as u32]
				);
				let target_last = target.as_view().segments().last();
				let name = target_last.unwrap().name;
				let indexes = catalog.indexes();
				assert!(indexes.symbols_by_language_key(Lang::C, name).is_none());
				assert!(
					indexes.c_lexical_symbols_by_key(name).unwrap().len() as u64 <= visible.len(),
					"the fixture must exercise the inverse lookup"
				);
				let query = LinkageQuery {
					material,
					target,
					target_first: target.as_view().segments().next(),
					target_last,
					target_segment_count: target.as_view().segments().count(),
					reference_kind: "reads",
					call_name: None,
					call_arity: None,
					confidence: None,
					source_file: source,
				};
				CIncludeVisibility::matching_candidates(visible, &query, catalog)
			};
		let previous = symbols(&material);
		assert_eq!(previous.len(), 2);
		for (def, target) in &previous {
			let candidates = matching(&material, &catalog, target);
			assert_eq!(
				candidates.iter().collect::<Vec<_>>(),
				[catalog.symbol_at(included, *def).unwrap()]
			);
		}

		std::fs::write(
			&header,
			original.replace("arg", "value").replace("local", "amount"),
		)
		.unwrap();
		let refreshed = port.refresh_paths(&index, &[header]).unwrap().index;
		let material = cache.index_material(refreshed.generation).unwrap();
		catalog.refresh_files(&material, refreshed.inventory.catalog().clone());
		for (_, target) in previous {
			let name = target.as_view().segments().last().unwrap().name;
			assert_eq!(
				catalog
					.indexes()
					.c_lexical_symbols_by_key(name)
					.unwrap()
					.len(),
				1
			);
			assert!(matching(&material, &catalog, &target).is_empty());
		}
		let current = symbols(&material);
		assert_eq!(current.len(), 2);
		for (def, target) in &current {
			let candidates = matching(&material, &catalog, target);
			assert_eq!(
				candidates.iter().collect::<Vec<_>>(),
				[catalog.symbol_at(included, *def).unwrap()]
			);
		}
	}
}
