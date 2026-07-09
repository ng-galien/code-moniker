use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::kinds::BIND_IMPORT;
use code_moniker_core::core::moniker::Moniker;
use rustc_hash::FxHashMap;

use crate::code::ref_kind;
use crate::environment;

use super::model::{HunkCoverage, RefChange, RefChangeKind, SymbolChange};
use super::pairing::FileSide;

pub struct RenameContext {
	pairs: Vec<(Moniker, Moniker)>,
}

impl RenameContext {
	pub fn from_changes(changes: &[SymbolChange]) -> Self {
		let pairs = changes
			.iter()
			.filter_map(|change| {
				let old = change.old.as_ref()?.moniker.clone();
				let new = change.new.as_ref()?.moniker.clone();
				(old != new).then_some((old, new))
			})
			.collect();
		Self { pairs }
	}

	pub fn push_pair(&mut self, old: Moniker, new: Moniker) {
		if old != new {
			self.pairs.push((old, new));
		}
	}

	fn apply(&self, target: &Moniker) -> Option<Moniker> {
		let view = target.as_view();
		let mut best: Option<&(Moniker, Moniker)> = None;
		for pair in &self.pairs {
			if !pair.0.as_view().is_ancestor_of(&view) {
				continue;
			}
			if best.is_none_or(|kept| pair.0.as_encoded().len() > kept.0.as_encoded().len()) {
				best = Some(pair);
			}
		}
		let (from, to) = best?;
		let mut bytes = to.as_encoded().to_vec();
		bytes.extend_from_slice(&target.as_encoded()[from.as_encoded().len()..]);
		Moniker::from_encoded(bytes).ok()
	}
}

type RefKey = (Vec<u8>, Vec<u8>, Vec<u8>, Option<usize>, Vec<u8>, Vec<u8>);

struct RefFact {
	raw_key: RefKey,
	mapped_key: Option<RefKey>,
	ref_kind: String,
	import: bool,
	target: Moniker,
	line_range: Option<(u32, u32)>,
}

pub fn pair_refs(
	base: &FileSide<'_>,
	current: &FileSide<'_>,
	ctx: &RenameContext,
) -> Vec<RefChange> {
	let mut old_facts = collect_ref_facts(base, Some(ctx));
	let mut new_facts = collect_ref_facts(current, None);
	cancel_unchanged(&mut old_facts, &mut new_facts);
	let mut changes = pair_retargets(&mut old_facts, &mut new_facts, current);
	changes.extend(old_facts.into_iter().flatten().map(|fact| RefChange {
		kind: RefChangeKind::Removed,
		file_path: base.file_path.to_path_buf(),
		ref_kind: fact.ref_kind,
		old_target: Some(fact.target),
		new_target: None,
		old_line_range: fact.line_range,
		new_line_range: None,
	}));
	changes.extend(new_facts.into_iter().flatten().map(|fact| RefChange {
		kind: RefChangeKind::Added,
		file_path: current.file_path.to_path_buf(),
		ref_kind: fact.ref_kind,
		old_target: None,
		new_target: Some(fact.target),
		old_line_range: None,
		new_line_range: fact.line_range,
	}));
	changes
}

fn collect_ref_facts(file: &FileSide<'_>, ctx: Option<&RenameContext>) -> Vec<Option<RefFact>> {
	file.graph
		.refs()
		.map(|record| Some(ref_fact(file, record, ctx)))
		.collect()
}

fn ref_fact(file: &FileSide<'_>, record: &RefRecord, ctx: Option<&RenameContext>) -> RefFact {
	let source = file.graph.def_at(record.source).moniker.clone();
	let raw_key = ref_key(record, &source, &record.target);
	let mapped_key = ctx.and_then(|ctx| {
		let mapped_source = ctx.apply(&source);
		let mapped_target = ctx.apply(&record.target);
		if mapped_source.is_none() && mapped_target.is_none() {
			return None;
		}
		Some(ref_key(
			record,
			mapped_source.as_ref().unwrap_or(&source),
			mapped_target.as_ref().unwrap_or(&record.target),
		))
	});
	RefFact {
		raw_key,
		mapped_key,
		ref_kind: ref_kind(record),
		import: record.binding.as_ref() == BIND_IMPORT,
		target: record.target.clone(),
		line_range: record
			.position
			.map(|(start, end)| environment::line_range(file.source, start, end)),
	}
}

fn ref_key(record: &RefRecord, source: &Moniker, target: &Moniker) -> RefKey {
	(
		source.as_encoded().to_vec(),
		target.as_encoded().to_vec(),
		record.kind.to_vec(),
		record.call_arity,
		record.alias.to_vec(),
		record.binding.to_vec(),
	)
}

fn cancel_unchanged(old_facts: &mut [Option<RefFact>], new_facts: &mut [Option<RefFact>]) {
	let mut by_key: FxHashMap<RefKey, Vec<usize>> = FxHashMap::default();
	for (idx, fact) in new_facts.iter().enumerate() {
		if let Some(fact) = fact {
			by_key.entry(fact.raw_key.clone()).or_default().push(idx);
		}
	}
	for old_slot in old_facts.iter_mut() {
		let Some(fact) = old_slot else { continue };
		let Some(matches) = by_key.get_mut(&fact.raw_key) else {
			continue;
		};
		let Some(new_idx) = matches.pop() else {
			continue;
		};
		new_facts[new_idx] = None;
		*old_slot = None;
	}
}

fn pair_retargets(
	old_facts: &mut [Option<RefFact>],
	new_facts: &mut [Option<RefFact>],
	current: &FileSide<'_>,
) -> Vec<RefChange> {
	let mut by_key: FxHashMap<RefKey, Vec<usize>> = FxHashMap::default();
	for (idx, fact) in new_facts.iter().enumerate() {
		if let Some(fact) = fact {
			by_key.entry(fact.raw_key.clone()).or_default().push(idx);
		}
	}
	let mut changes = Vec::new();
	for old_slot in old_facts.iter_mut() {
		let Some(fact) = old_slot else { continue };
		let Some(mapped_key) = fact.mapped_key.as_ref() else {
			continue;
		};
		let Some(new_idx) = by_key.get_mut(mapped_key).and_then(Vec::pop) else {
			continue;
		};
		let old = old_slot.take().expect("checked above");
		let new = new_facts[new_idx].take().expect("indexed above");
		let kind = if old.import {
			RefChangeKind::ImportRetargeted
		} else {
			RefChangeKind::CallSiteRetargeted
		};
		changes.push(RefChange {
			kind,
			file_path: current.file_path.to_path_buf(),
			ref_kind: new.ref_kind,
			old_target: Some(old.target),
			new_target: Some(new.target),
			old_line_range: old.line_range,
			new_line_range: new.line_range,
		});
	}
	changes
}

pub struct CoverageInputs<'a> {
	pub old_hunks: &'a [(u32, u32)],
	pub new_hunks: &'a [(u32, u32)],
	pub old_explained: &'a [(u32, u32)],
	pub new_explained: &'a [(u32, u32)],
}

pub fn hunk_coverage(inputs: CoverageInputs<'_>) -> HunkCoverage {
	HunkCoverage {
		old_residual: residual_spans(inputs.old_hunks, inputs.old_explained),
		new_residual: residual_spans(inputs.new_hunks, inputs.new_explained),
	}
}

fn residual_spans(hunks: &[(u32, u32)], explained: &[(u32, u32)]) -> Vec<(u32, u32)> {
	let covered = merged_spans(explained);
	let mut out = Vec::new();
	for &(start, end) in hunks {
		let mut cursor = start;
		for &(covered_start, covered_end) in &covered {
			if covered_end < cursor || covered_start > end {
				continue;
			}
			if covered_start > cursor {
				out.push((cursor, covered_start - 1));
			}
			cursor = cursor.max(covered_end.saturating_add(1));
			if cursor > end {
				break;
			}
		}
		if cursor <= end {
			out.push((cursor, end));
		}
	}
	out
}

fn merged_spans(spans: &[(u32, u32)]) -> Vec<(u32, u32)> {
	let mut sorted = spans.to_vec();
	sorted.sort_unstable();
	let mut merged: Vec<(u32, u32)> = Vec::new();
	for (start, end) in sorted {
		match merged.last_mut() {
			Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
			_ => merged.push((start, end)),
		}
	}
	merged
}

#[cfg(test)]
mod tests {
	use super::super::pairing::{PairInputs, finish_files, pair_file};
	use super::*;
	use code_moniker_core::lang::Lang;
	use std::path::Path;

	struct Extraction {
		graph: code_moniker_core::core::code_graph::CodeGraph,
		source: String,
		rel: String,
	}

	fn extract(source: &str, rel: &str) -> Extraction {
		Extraction {
			graph: environment::extract_source(Lang::Rs, source, Path::new(rel)),
			source: source.to_string(),
			rel: rel.to_string(),
		}
	}

	fn file_side(extraction: &Extraction) -> FileSide<'_> {
		FileSide {
			lang: Lang::Rs,
			graph: &extraction.graph,
			source: &extraction.source,
			file_path: Path::new(&extraction.rel),
		}
	}

	#[test]
	fn call_sites_retarget_after_a_rename() {
		let base = extract(
			"fn helper(x: u32) -> u32 { x }\nfn caller() { helper(1); helper(2); }\n",
			"src/lib.rs",
		);
		let current = extract(
			"fn assist(x: u32) -> u32 { x }\nfn caller() { assist(1); assist(2); }\n",
			"src/lib.rs",
		);
		let symbol_changes = finish_files(vec![pair_file(PairInputs {
			base: file_side(&base),
			current: file_side(&current),
			file_moved: false,
		})]);
		let ctx = RenameContext::from_changes(&symbol_changes);

		let ref_changes = pair_refs(&file_side(&base), &file_side(&current), &ctx);

		let retargeted_calls: Vec<_> = ref_changes
			.iter()
			.filter(|change| {
				change.kind == RefChangeKind::CallSiteRetargeted && change.ref_kind == "calls"
			})
			.collect();
		assert_eq!(retargeted_calls.len(), 2, "{ref_changes:?}");
		assert!(
			ref_changes.iter().all(|change| change.kind.is_retarget()),
			"no stray added/removed refs: {ref_changes:?}"
		);
	}

	#[test]
	fn imports_retarget_through_a_module_prefix_pair() {
		let base = extract(
			"mod helpers;\nuse crate::helpers::assist;\n\nfn caller() { assist(); }\n",
			"src/lib.rs",
		);
		let current = extract(
			"mod support;\nuse crate::support::assist;\n\nfn caller() { assist(); }\n",
			"src/lib.rs",
		);
		let old_module = extract("pub fn assist() {}\n", "src/helpers.rs");
		let new_module = extract("pub fn assist() {}\n", "src/support.rs");
		let mut ctx = RenameContext::from_changes(&[]);
		ctx.push_pair(
			old_module.graph.root().clone(),
			new_module.graph.root().clone(),
		);

		let ref_changes = pair_refs(&file_side(&base), &file_side(&current), &ctx);

		assert!(
			ref_changes
				.iter()
				.any(|change| change.kind == RefChangeKind::ImportRetargeted
					&& change.ref_kind == "imports_symbol"),
			"symbol import must retarget through the module prefix: {ref_changes:?}"
		);
		assert!(
			ref_changes.iter().all(|change| change.kind.is_retarget()),
			"{ref_changes:?}"
		);
	}

	#[test]
	fn unrelated_ref_edits_stay_added_and_removed() {
		let base = extract("fn caller() { alpha(); }\n", "src/lib.rs");
		let current = extract("fn caller() { beta(); }\n", "src/lib.rs");
		let ctx = RenameContext::from_changes(&[]);

		let ref_changes = pair_refs(&file_side(&base), &file_side(&current), &ctx);

		let labels: Vec<_> = ref_changes.iter().map(|change| change.kind).collect();
		assert!(labels.contains(&RefChangeKind::Added), "{ref_changes:?}");
		assert!(labels.contains(&RefChangeKind::Removed), "{ref_changes:?}");
	}

	#[test]
	fn coverage_subtracts_explained_spans() {
		let coverage = hunk_coverage(CoverageInputs {
			old_hunks: &[],
			new_hunks: &[(10, 20), (30, 31)],
			old_explained: &[],
			new_explained: &[(9, 15), (18, 20)],
		});

		assert_eq!(coverage.new_residual, vec![(16, 17), (30, 31)]);
		assert!(!coverage.explained());
	}

	#[test]
	fn coverage_is_explained_when_all_hunks_are_covered() {
		let coverage = hunk_coverage(CoverageInputs {
			old_hunks: &[(5, 6)],
			new_hunks: &[(10, 20)],
			old_explained: &[(1, 8)],
			new_explained: &[(10, 14), (15, 20)],
		});

		assert!(coverage.explained(), "{coverage:?}");
	}
}
