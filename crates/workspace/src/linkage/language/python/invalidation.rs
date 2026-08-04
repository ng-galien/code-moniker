use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::{Lang, kinds};
use rustc_hash::FxHashMap;

use crate::linkage::change::LinkageRefreshImpact;
use crate::snapshot::{RecordTable, ReferenceRecord, SourceId};
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) fn binding_invalidation_sources(
	references: &RecordTable<ReferenceRecord>,
	material: &CodeIndexMaterial,
	impact: &LinkageRefreshImpact,
	edited_sources: &BTreeSet<SourceId>,
	edited_files: &BTreeSet<usize>,
) -> BTreeSet<SourceId> {
	let existing_python_edit = edited_files.iter().any(|file| {
		material
			.files
			.get(*file)
			.is_some_and(|file| file.lang == Lang::Python)
	});
	let python_path_changed = impact
		.changed_paths()
		.iter()
		.any(|path| path.extension().is_some_and(|extension| extension == "py"));
	if !existing_python_edit && !python_path_changed {
		return BTreeSet::new();
	}
	let removed_binding = impact.references().removed_binding();
	let changed_binding = impact.references().changed_ids().iter().any(|id| {
		references
			.iter()
			.find(|reference| reference.id == *id)
			.is_some_and(|reference| {
				matches!(
					reference.kind.as_bytes(),
					kinds::IMPORTS_MODULE | kinds::IMPORTS_SYMBOL | kinds::REEXPORTS
				)
			})
	});
	let changed_module_definition = !impact.definitions().retargeted_identities().is_empty()
		|| impact.definitions().candidate_ids().iter().any(|id| {
			material
				.symbol_moniker(id)
				.and_then(|moniker| moniker.parent())
				.is_some_and(|owner| {
					let segments = owner.as_view().segments().collect::<Vec<_>>();
					segments.first().is_some_and(|segment| {
						segment.kind == kinds::LANG && segment.name == b"python"
					}) && segments
						.last()
						.is_some_and(|segment| segment.kind == kinds::MODULE)
				})
		});
	if !removed_binding && !changed_binding && !changed_module_definition {
		return BTreeSet::new();
	}
	let removed_module_keys = removed_python_module_keys(material, impact.changed_paths());
	let binding_seed_known = existing_python_edit || !removed_module_keys.is_empty();
	let affected_sources =
		affected_binding_sources(references, material, edited_sources, removed_module_keys);
	if affected_sources.is_empty() && python_path_changed && !binding_seed_known {
		return material
			.files
			.iter()
			.filter(|file| file.lang == Lang::Python)
			.map(|file| file.source_id)
			.collect();
	}
	affected_sources
}

fn affected_binding_sources(
	references: &RecordTable<ReferenceRecord>,
	material: &CodeIndexMaterial,
	edited_sources: &BTreeSet<SourceId>,
	removed_module_keys: BTreeSet<Vec<Vec<u8>>>,
) -> BTreeSet<SourceId> {
	let module_by_source = material
		.symbols()
		.filter_map(|(symbol, moniker)| {
			let source = material.symbol_source(&symbol)?;
			let key = python_module_key(moniker)?;
			Some((source, key))
		})
		.collect::<FxHashMap<_, _>>();
	let mut affected_sources = edited_sources
		.iter()
		.filter(|source| module_by_source.contains_key(*source))
		.cloned()
		.collect::<BTreeSet<_>>();
	let mut affected_modules = affected_sources
		.iter()
		.filter_map(|source| module_by_source.get(source).cloned())
		.collect::<BTreeSet<_>>();
	affected_modules.extend(removed_module_keys);

	loop {
		let mut changed = false;
		for reference in references.iter().filter(|reference| {
			matches!(
				reference.kind.as_bytes(),
				kinds::IMPORTS_MODULE | kinds::IMPORTS_SYMBOL | kinds::REEXPORTS
			)
		}) {
			let Some(target) = material.reference_target(&reference.id) else {
				continue;
			};
			let Some(target_module) = python_module_key(target) else {
				continue;
			};
			if !affected_modules.contains(&target_module) {
				continue;
			}
			let source = reference.source;
			let Some(source_module) = module_by_source.get(&source) else {
				continue;
			};
			changed |= affected_sources.insert(source);
			changed |= affected_modules.insert(source_module.clone());
		}
		if !changed {
			break;
		}
	}
	affected_sources
}

fn removed_python_module_keys(
	material: &CodeIndexMaterial,
	changed_paths: &[PathBuf],
) -> BTreeSet<Vec<Vec<u8>>> {
	changed_paths
		.iter()
		.filter(|path| path.extension().is_some_and(|extension| extension == "py"))
		.filter(|path| !path.exists())
		.filter_map(|path| {
			material
				.source_catalog
				.sources
				.roots
				.iter()
				.filter_map(|root| path.strip_prefix(&root.path).ok())
				.min_by_key(|relative| relative.components().count())
		})
		.filter_map(python_module_key_from_path)
		.collect()
}

fn python_module_key_from_path(path: &Path) -> Option<Vec<Vec<u8>>> {
	let mut components = path
		.parent()
		.into_iter()
		.flat_map(Path::components)
		.filter_map(|component| component.as_os_str().to_str())
		.map(|component| component.as_bytes().to_vec())
		.collect::<Vec<_>>();
	let stem = path.file_stem()?.to_str()?;
	if stem != "__init__" {
		components.push(stem.as_bytes().to_vec());
	}
	Some(components)
}

fn python_module_key(moniker: &Moniker) -> Option<Vec<Vec<u8>>> {
	let mut current = moniker.clone();
	loop {
		let segments = current.as_view().segments().collect::<Vec<_>>();
		if segments
			.first()
			.is_some_and(|segment| segment.kind == kinds::LANG && segment.name == b"python")
			&& segments
				.last()
				.is_some_and(|segment| segment.kind == kinds::MODULE)
		{
			let mut key = segments[1..]
				.iter()
				.filter(|segment| {
					matches!(segment.kind, kinds::PACKAGE | kinds::MODULE | kinds::PATH)
				})
				.map(|segment| segment.name.to_vec())
				.collect::<Vec<_>>();
			if key.last().is_some_and(|name| name == b"__init__") {
				key.pop();
			}
			return Some(key);
		}
		current = current.parent()?;
	}
}
