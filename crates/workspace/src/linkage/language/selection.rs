use code_moniker_core::lang::{Lang, kinds};

use crate::linkage::catalog::{CandidateCatalog, LinkageQuery, SymbolSet};
use crate::snapshot::ResolutionEvidence;
use crate::source::CodeIndexMaterial;

pub(in crate::linkage) fn prefer_concrete_definitions(
	material: &CodeIndexMaterial,
	catalog: &CandidateCatalog,
	targets: SymbolSet,
) -> SymbolSet {
	if targets.len() < 2 {
		return targets;
	}
	let mut concrete = SymbolSet::new();
	for symbol in targets.iter() {
		let is_alias = catalog.candidate(symbol).is_some_and(|candidate| {
			candidate
				.last_segment
				.is_some_and(|segment| segment.kind == kinds::PATH)
				&& material
					.files
					.get(candidate.source_file)
					.is_some_and(|file| file.lang == Lang::Rs)
		});
		if !is_alias {
			concrete.insert(symbol);
		}
	}
	if concrete.is_empty() || concrete.len() == targets.len() {
		targets
	} else {
		concrete
	}
}

pub(in crate::linkage) fn global_resolution_evidence(
	query: &LinkageQuery<'_>,
) -> ResolutionEvidence {
	if query.reference_kind == "calls"
		&& query
			.material
			.files
			.get(query.source_file)
			.is_some_and(|file| file.lang == Lang::Sql)
		&& !super::sql_call_has_strong_evidence(query)
	{
		ResolutionEvidence::NameMatch
	} else {
		ResolutionEvidence::GlobalBinding
	}
}

pub(in crate::linkage) fn local_resolution_evidence(
	query: &LinkageQuery<'_>,
	candidates: &CandidateCatalog,
	targets: &SymbolSet,
) -> ResolutionEvidence {
	let lang = query
		.material
		.files
		.get(query.source_file)
		.map(|file| file.lang);
	if lang == Some(Lang::Sql) && query.reference_kind == "calls" {
		return if super::sql_call_has_strong_evidence(query) {
			ResolutionEvidence::LocalBinding
		} else {
			ResolutionEvidence::NameMatch
		};
	}
	if lang != Some(Lang::Sql) && query.confidence != Some("name_match") {
		return ResolutionEvidence::LocalBinding;
	}
	if !matches!(lang, Some(Lang::Cs | Lang::Sql)) {
		return ResolutionEvidence::LocalBinding;
	}
	let exact = candidates.indexes().symbol_by_moniker(query.target);
	if exact.is_some_and(|exact| targets.iter().any(|target| target == exact)) {
		ResolutionEvidence::LocalBinding
	} else {
		ResolutionEvidence::NameMatch
	}
}

pub(in crate::linkage) fn confirm_name_match_targets(
	candidates: &CandidateCatalog,
	query: &LinkageQuery<'_>,
	targets: SymbolSet,
) -> SymbolSet {
	if targets.len() <= 1 {
		return targets;
	}
	let targets = match query.confidence {
		Some("name_match") => restrict_to_source_package(candidates, query, targets),
		Some("imported") => targets,
		_ => return targets,
	};
	let source_srcset = file_srcset(query.material, query.source_file);
	prefer_same_srcset(candidates, &source_srcset, targets)
}

fn restrict_to_source_package(
	candidates: &CandidateCatalog,
	query: &LinkageQuery<'_>,
	targets: SymbolSet,
) -> SymbolSet {
	let source_packages = file_package_chain(query.material, query.source_file);
	if source_packages.is_empty() {
		return targets;
	}
	let mut same_package = SymbolSet::new();
	for symbol in targets.iter() {
		let Some(candidate) = candidates.candidate(symbol) else {
			continue;
		};
		if moniker_package_chain(candidate.moniker) == source_packages {
			same_package.insert(symbol);
		}
	}
	same_package
}

fn prefer_same_srcset(
	candidates: &CandidateCatalog,
	source_srcset: &[u8],
	targets: SymbolSet,
) -> SymbolSet {
	if source_srcset.is_empty() || targets.len() <= 1 {
		return targets;
	}
	let mut same_srcset = SymbolSet::new();
	for symbol in targets.iter() {
		let Some(candidate) = candidates.candidate(symbol) else {
			continue;
		};
		if moniker_srcset(candidate.moniker) == source_srcset {
			same_srcset.insert(symbol);
		}
	}
	if same_srcset.is_empty() {
		targets
	} else {
		same_srcset
	}
}

fn file_srcset(material: &CodeIndexMaterial, file_idx: usize) -> Vec<u8> {
	let Some(file) = material.files.get(file_idx) else {
		return Vec::new();
	};
	if file.graph.def_count() == 0 {
		return Vec::new();
	}
	moniker_srcset(&file.graph.def_at(0).moniker)
}

fn moniker_srcset(moniker: &code_moniker_core::core::moniker::Moniker) -> Vec<u8> {
	moniker
		.as_view()
		.segments()
		.find(|segment| segment.kind == b"srcset")
		.map(|segment| segment.name.to_vec())
		.unwrap_or_default()
}

fn file_package_chain(material: &CodeIndexMaterial, file_idx: usize) -> Vec<Vec<u8>> {
	let Some(file) = material.files.get(file_idx) else {
		return Vec::new();
	};
	if file.graph.def_count() == 0 {
		return Vec::new();
	}
	moniker_package_chain(&file.graph.def_at(0).moniker)
}

fn moniker_package_chain(moniker: &code_moniker_core::core::moniker::Moniker) -> Vec<Vec<u8>> {
	moniker
		.as_view()
		.segments()
		.filter(|segment| segment.kind == kinds::PACKAGE)
		.map(|segment| segment.name.to_vec())
		.collect()
}
