use std::path::Path;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::code::{def_kind, is_navigable_def, last_name};
use crate::environment;

use super::fingerprint::{
	FingerprintScope, IdentityTail, def_fingerprints, identity_tail, split_callable_name,
};
use super::model::{ChangeFacets, Confidence, SemanticKind, SymbolChange, SymbolSide};

pub struct FileSide<'a> {
	pub lang: Lang,
	pub graph: &'a CodeGraph,
	pub source: &'a str,
	pub file_path: &'a Path,
}

pub struct PairInputs<'a> {
	pub base: FileSide<'a>,
	pub current: FileSide<'a>,
	pub file_moved: bool,
}

struct SideDef {
	tail: IdentityTail,
	base_name: Vec<u8>,
	params: Option<Vec<u8>>,
	has_body: bool,
	text_hash: u64,
	side: SymbolSide,
}

struct PairingState {
	old: Vec<Option<SideDef>>,
	new: Vec<Option<SideDef>>,
	changes: Vec<SymbolChange>,
	file_moved: bool,
}

pub fn pair_file(inputs: PairInputs<'_>) -> Vec<SymbolChange> {
	let mut state = PairingState {
		old: collect_side_defs(&inputs.base),
		new: collect_side_defs(&inputs.current),
		changes: Vec::new(),
		file_moved: inputs.file_moved,
	};
	pair_exact_tails(&mut state);
	let mut renames = pair_signature_changes(&mut state);
	renames.extend(pair_renames(&mut state));
	propagate_container_renames(&mut state, renames);
	finalize_unpaired(&mut state);
	sort_changes(&mut state.changes);
	state.changes
}

fn collect_side_defs(file: &FileSide<'_>) -> Vec<Option<SideDef>> {
	let nested_spans: Vec<(u32, u32)> = file
		.graph
		.defs()
		.filter(|def| is_navigable_def(file.lang, def))
		.filter_map(|def| def.position)
		.collect();
	file.graph
		.defs()
		.filter(|def| is_navigable_def(file.lang, def))
		.filter_map(|def| side_def(file, def, &nested_spans))
		.map(Some)
		.collect()
}

fn side_def(file: &FileSide<'_>, def: &DefRecord, nested_spans: &[(u32, u32)]) -> Option<SideDef> {
	let tail = identity_tail(&def.moniker, file.graph.root())?;
	let last = tail.last()?;
	let (base_name, params) = split_callable_name(&last.name);
	let base_name = base_name.to_vec();
	let params = params.map(<[u8]>::to_vec);
	let prints = def
		.position
		.map(|span| {
			def_fingerprints(FingerprintScope {
				source: file.source,
				span,
				name: &base_name,
				nested_spans,
			})
		})
		.unwrap_or_default();
	let side = SymbolSide {
		moniker: def.moniker.clone(),
		file_path: file.file_path.to_path_buf(),
		kind: def_kind(def),
		name: last_name(&def.moniker),
		visibility: String::from_utf8_lossy(&def.visibility).into_owned(),
		signature: String::from_utf8_lossy(&def.signature).into_owned(),
		line_range: def
			.position
			.map(|(start, end)| environment::line_range(file.source, start, end)),
		body_hash: prints.body,
	};
	Some(SideDef {
		tail,
		base_name,
		params,
		has_body: def.position.is_some(),
		text_hash: prints.text,
		side,
	})
}

fn pair_exact_tails(state: &mut PairingState) {
	let mut by_tail: FxHashMap<IdentityTail, usize> = FxHashMap::default();
	for (idx, slot) in state.new.iter().enumerate() {
		if let Some(def) = slot {
			by_tail.insert(def.tail.clone(), idx);
		}
	}
	for old_slot in &mut state.old {
		let Some(new_idx) = old_slot.as_ref().and_then(|def| by_tail.remove(&def.tail)) else {
			continue;
		};
		let old = old_slot.take().expect("slot checked above");
		let new = state.new[new_idx].take().expect("indexed above");
		if let Some(change) = classify_matched(old, new, state.file_moved) {
			state.changes.push(change);
		}
	}
}

fn classify_matched(old: SideDef, new: SideDef, file_moved: bool) -> Option<SymbolChange> {
	let comparable = old.has_body && new.has_body;
	let body_changed = comparable && old.side.body_hash != new.side.body_hash;
	let text_changed = comparable && old.text_hash != new.text_hash;
	let facets = ChangeFacets {
		body_changed,
		signature_changed: old.side.signature != new.side.signature || old.params != new.params,
		visibility_changed: old.side.visibility != new.side.visibility,
		header_changed: text_changed && !body_changed,
		file_moved,
	};
	if !facets.any() {
		return None;
	}
	let kind = if file_moved {
		SemanticKind::Moved
	} else if facets.body_changed {
		SemanticKind::BodyModified
	} else if facets.signature_changed {
		SemanticKind::SignatureChanged
	} else {
		SemanticKind::AttributeChanged
	};
	Some(paired_change(kind, facets, old, new))
}

fn paired_change(
	kind: SemanticKind,
	facets: ChangeFacets,
	old: SideDef,
	new: SideDef,
) -> SymbolChange {
	SymbolChange {
		kind,
		confidence: Confidence::Certain,
		facets,
		old: Some(old.side),
		new: Some(new.side),
	}
}

type RenameMap = Vec<(IdentityTail, IdentityTail)>;
type SlotGroups<K> = FxHashMap<K, (Vec<usize>, Vec<usize>)>;

fn pair_signature_changes(state: &mut PairingState) -> RenameMap {
	let mut groups: SlotGroups<(IdentityTail, String, Vec<u8>)> = FxHashMap::default();
	collect_groups(&state.old, &mut groups, 0, signature_key);
	collect_groups(&state.new, &mut groups, 1, signature_key);
	let mut retargets = Vec::new();
	for (olds, news) in groups.into_values() {
		let [old_idx] = olds.as_slice() else { continue };
		let [new_idx] = news.as_slice() else { continue };
		let old = state.old[*old_idx].take().expect("grouped old");
		let new = state.new[*new_idx].take().expect("grouped new");
		retargets.push((old.tail.clone(), new.tail.clone()));
		let facets = ChangeFacets {
			signature_changed: true,
			visibility_changed: old.side.visibility != new.side.visibility,
			file_moved: state.file_moved,
			..ChangeFacets::default()
		};
		state.changes.push(paired_change(
			SemanticKind::SignatureChanged,
			facets,
			old,
			new,
		));
	}
	retargets
}

fn signature_key(def: &SideDef) -> Option<(IdentityTail, String, Vec<u8>)> {
	def.params.as_ref()?;
	Some((
		def.tail.parent(),
		def.side.kind.clone(),
		def.base_name.clone(),
	))
}

fn pair_renames(state: &mut PairingState) -> RenameMap {
	let mut groups: SlotGroups<(IdentityTail, String, u64)> = FxHashMap::default();
	collect_groups(&state.old, &mut groups, 0, rename_key);
	collect_groups(&state.new, &mut groups, 1, rename_key);
	let mut retargets = Vec::new();
	for (olds, news) in groups.into_values() {
		let [old_idx] = olds.as_slice() else { continue };
		let [new_idx] = news.as_slice() else { continue };
		let differs = state.old[*old_idx]
			.as_ref()
			.zip(state.new[*new_idx].as_ref())
			.is_some_and(|(old, new)| old.base_name != new.base_name);
		if !differs {
			continue;
		}
		let old = state.old[*old_idx].take().expect("grouped old");
		let new = state.new[*new_idx].take().expect("grouped new");
		retargets.push((old.tail.clone(), new.tail.clone()));
		let facets = ChangeFacets {
			signature_changed: old.params != new.params,
			visibility_changed: old.side.visibility != new.side.visibility,
			file_moved: state.file_moved,
			..ChangeFacets::default()
		};
		state
			.changes
			.push(paired_change(SemanticKind::Renamed, facets, old, new));
	}
	retargets
}

fn rename_key(def: &SideDef) -> Option<(IdentityTail, String, u64)> {
	if !def.has_body {
		return None;
	}
	Some((def.tail.parent(), def.side.kind.clone(), def.text_hash))
}

fn collect_groups<K: std::hash::Hash + Eq>(
	slots: &[Option<SideDef>],
	groups: &mut SlotGroups<K>,
	side: usize,
	key: impl Fn(&SideDef) -> Option<K>,
) {
	for (idx, slot) in slots.iter().enumerate() {
		let Some(group_key) = slot.as_ref().and_then(&key) else {
			continue;
		};
		let entry = groups.entry(group_key).or_default();
		if side == 0 {
			entry.0.push(idx);
		} else {
			entry.1.push(idx);
		}
	}
}

fn propagate_container_renames(state: &mut PairingState, mut renames: RenameMap) {
	while !renames.is_empty() {
		if !rewrite_tails(&mut state.old, &renames) {
			return;
		}
		pair_exact_tails(state);
		let mut next = pair_signature_changes(state);
		next.extend(pair_renames(state));
		renames = next;
	}
}

fn rewrite_tails(slots: &mut [Option<SideDef>], renames: &RenameMap) -> bool {
	let mut rewrote = false;
	for slot in slots {
		let Some(def) = slot else { continue };
		for (from, to) in renames {
			let Some(tail) = def.tail.rewrite_prefix(from, to) else {
				continue;
			};
			def.tail = tail;
			rewrote = true;
			break;
		}
	}
	rewrote
}

fn finalize_unpaired(state: &mut PairingState) {
	let removed = unpaired_changes(&mut state.old, SemanticKind::Removed, state.file_moved);
	let added = unpaired_changes(&mut state.new, SemanticKind::Added, state.file_moved);
	state.changes.extend(removed);
	state.changes.extend(added);
}

fn unpaired_changes(
	slots: &mut [Option<SideDef>],
	kind: SemanticKind,
	file_moved: bool,
) -> Vec<SymbolChange> {
	let defs: Vec<SideDef> = slots.iter_mut().filter_map(Option::take).collect();
	let tails: Vec<IdentityTail> = defs.iter().map(|def| def.tail.clone()).collect();
	defs.into_iter()
		.filter(|def| {
			!tails
				.iter()
				.any(|tail| tail != &def.tail && def.tail.starts_with(tail))
		})
		.map(|def| {
			let facets = ChangeFacets {
				file_moved,
				..ChangeFacets::default()
			};
			let (old, new) = match kind {
				SemanticKind::Removed => (Some(def.side), None),
				_ => (None, Some(def.side)),
			};
			SymbolChange {
				kind,
				confidence: Confidence::Certain,
				facets,
				old,
				new,
			}
		})
		.collect()
}

fn sort_changes(changes: &mut [SymbolChange]) {
	changes.sort_by(|a, b| change_order(a).cmp(&change_order(b)));
}

fn change_order(change: &SymbolChange) -> (u32, &str) {
	let side = change
		.new
		.as_ref()
		.or(change.old.as_ref())
		.expect("a change has at least one side");
	let line = side.line_range.map(|(start, _)| start).unwrap_or(u32::MAX);
	(line, side.name.as_str())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	fn pair_rust(base: &str, current: &str) -> Vec<SymbolChange> {
		pair_lang(Lang::Rs, base, current, "src/lib.rs")
	}

	fn pair_lang(lang: Lang, base: &str, current: &str, rel: &str) -> Vec<SymbolChange> {
		let base_graph = environment::extract_source(lang, base, Path::new(rel));
		let current_graph = environment::extract_source(lang, current, Path::new(rel));
		pair_file(PairInputs {
			base: FileSide {
				lang,
				graph: &base_graph,
				source: base,
				file_path: Path::new(rel),
			},
			current: FileSide {
				lang,
				graph: &current_graph,
				source: current,
				file_path: Path::new(rel),
			},
			file_moved: false,
		})
	}

	fn kinds(changes: &[SymbolChange]) -> Vec<SemanticKind> {
		changes.iter().map(|change| change.kind).collect()
	}

	#[test]
	fn body_edit_reports_body_modified() {
		let changes = pair_rust(
			"fn kept() {}\nfn edited() { let x = 1; }\n",
			"fn kept() {}\nfn edited() { let x = 2; }\n",
		);

		assert_eq!(kinds(&changes), vec![SemanticKind::BodyModified]);
		let change = &changes[0];
		assert!(change.facets.body_changed);
		assert!(!change.facets.signature_changed);
		assert_eq!(change.confidence, Confidence::Certain);
		assert!(
			change
				.new
				.as_ref()
				.is_some_and(|side| side.name.starts_with("edited")),
		);
	}

	#[test]
	fn param_addition_reports_signature_changed() {
		let changes = pair_rust(
			"fn grow(a: u32) -> u32 { a }\n",
			"fn grow(a: u32, b: u32) -> u32 { a + b }\n",
		);

		assert_eq!(kinds(&changes), vec![SemanticKind::SignatureChanged]);
		let change = &changes[0];
		assert!(change.facets.signature_changed);
		assert!(!change.facets.body_changed);
		assert_eq!(change.confidence, Confidence::Certain);
		assert!(change.old.is_some() && change.new.is_some());
	}

	#[test]
	fn pure_rename_pairs_old_and_new_names() {
		let changes = pair_rust(
			"fn old_name(n: u32) -> u32 { old_name(n) }\nfn stay() {}\n",
			"fn fresh_name(n: u32) -> u32 { fresh_name(n) }\nfn stay() {}\n",
		);

		assert_eq!(kinds(&changes), vec![SemanticKind::Renamed]);
		let change = &changes[0];
		assert_eq!(change.confidence, Confidence::Certain);
		assert!(
			change
				.old
				.as_ref()
				.is_some_and(|side| side.name.starts_with("old_name"))
		);
		assert!(
			change
				.new
				.as_ref()
				.is_some_and(|side| side.name.starts_with("fresh_name"))
		);
	}

	#[test]
	fn rename_with_body_edit_stays_removed_plus_added() {
		let changes = pair_rust(
			"fn old_name() { let x = 1; }\n",
			"fn fresh_name() { let x = 2; }\n",
		);

		let mut sorted = kinds(&changes);
		sorted.sort_by_key(|kind| kind.label());
		assert_eq!(sorted, vec![SemanticKind::Added, SemanticKind::Removed]);
	}

	#[test]
	fn visibility_only_change_reports_attribute_changed() {
		let changes = pair_rust("fn open() { work(); }\n", "pub fn open() { work(); }\n");

		assert_eq!(kinds(&changes), vec![SemanticKind::AttributeChanged]);
		let change = &changes[0];
		assert!(change.facets.visibility_changed);
		assert!(!change.facets.body_changed);
	}

	#[test]
	fn ambiguous_duplicate_bodies_stay_removed_plus_added() {
		let changes = pair_rust(
			"fn twin_a() { work(); }\nfn twin_b() { work(); }\n",
			"fn twin_c() { work(); }\nfn twin_d() { work(); }\n",
		);

		let mut sorted = kinds(&changes);
		sorted.sort_by_key(|kind| kind.label());
		assert_eq!(
			sorted,
			vec![
				SemanticKind::Added,
				SemanticKind::Added,
				SemanticKind::Removed,
				SemanticKind::Removed
			]
		);
	}

	#[test]
	fn container_rename_subsumes_unchanged_children() {
		let changes = pair_rust(
			"struct Holder;\nimpl Holder {\n\tfn touch(&self) { work(); }\n\tfn poke(&self) {}\n}\n",
			"struct Keeper;\nimpl Keeper {\n\tfn touch(&self) { work(); }\n\tfn poke(&self) {}\n}\n",
		);

		assert_eq!(
			kinds(&changes),
			vec![SemanticKind::Renamed],
			"children unchanged under a container rename must not surface: {changes:?}"
		);
		assert!(
			changes[0]
				.new
				.as_ref()
				.is_some_and(|side| side.name == "Keeper")
		);
	}

	#[test]
	fn container_rename_still_reports_edited_children() {
		let changes = pair_rust(
			"struct Holder;\nimpl Holder {\n\tfn touch(&self) { work(); }\n}\n",
			"struct Keeper;\nimpl Keeper {\n\tfn touch(&self) { rest(); }\n}\n",
		);

		let labels = kinds(&changes);
		assert!(labels.contains(&SemanticKind::Renamed), "{changes:?}");
		assert!(labels.contains(&SemanticKind::BodyModified), "{changes:?}");
		assert_eq!(labels.len(), 2, "{changes:?}");
	}

	#[test]
	fn added_subtree_collapses_to_its_root() {
		let changes = pair_rust(
			"fn kept() {}\n",
			"fn kept() {}\nstruct Fresh {\n\tcount: u32,\n}\nimpl Fresh {\n\tfn build() {}\n}\n",
		);

		let added: Vec<_> = changes
			.iter()
			.filter(|change| change.kind == SemanticKind::Added)
			.collect();
		assert_eq!(added.len(), 1, "{changes:?}");
		assert!(
			added[0]
				.new
				.as_ref()
				.is_some_and(|side| side.name == "Fresh")
		);
	}

	#[test]
	fn typescript_rename_pairs() {
		let changes = pair_lang(
			Lang::Ts,
			"export function oldName(a: number): number { return oldName(a); }\n",
			"export function freshName(a: number): number { return freshName(a); }\n",
			"src/util.ts",
		);

		assert_eq!(kinds(&changes), vec![SemanticKind::Renamed], "{changes:?}");
	}

	#[test]
	fn java_body_edit_reports_body_modified() {
		let changes = pair_lang(
			Lang::Java,
			"class Service {\n\tint total() { return 1; }\n}\n",
			"class Service {\n\tint total() { return 2; }\n}\n",
			"src/Service.java",
		);

		assert_eq!(
			kinds(&changes),
			vec![SemanticKind::BodyModified],
			"{changes:?}"
		);
	}
}
