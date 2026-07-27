use std::collections::HashSet;

use code_moniker_workspace::code::CodeIndexGraphDiff;
use code_moniker_workspace::snapshot::{SymbolInventoryIndex, SymbolSet};

use super::{CompiledWorkspaceRules, WorkspaceEvaluation, WorkspaceSymbolViolation, group};

pub(crate) struct WorkspaceIncrementalEvaluation {
	pub evaluation: WorkspaceEvaluation,
	pub dirty_symbols: usize,
	pub evaluated_symbols: usize,
	pub affected_groups: usize,
}

pub(crate) struct WorkspaceIncrementalInput<'a> {
	pub previous_inventory: &'a SymbolInventoryIndex,
	pub current_inventory: &'a SymbolInventoryIndex,
	pub previous_universe: &'a SymbolSet,
	pub current_universe: &'a SymbolSet,
	pub diff: &'a CodeIndexGraphDiff,
	pub compiled: &'a CompiledWorkspaceRules,
	pub previous: &'a WorkspaceEvaluation,
}

pub(crate) fn evaluate_workspace_rules_incremental(
	input: WorkspaceIncrementalInput<'_>,
) -> WorkspaceIncrementalEvaluation {
	let WorkspaceIncrementalInput {
		previous_inventory,
		current_inventory,
		previous_universe,
		current_universe,
		diff,
		compiled,
		previous,
	} = input;
	let (mut previous_dirty, mut current_dirty) =
		dirty_symbol_sets(previous_inventory, current_inventory, diff);
	previous_dirty.intersect_with(previous_universe);
	current_dirty.intersect_with(current_universe);
	let dirty_identities = dirty_identities(
		previous_inventory,
		current_inventory,
		&previous_dirty,
		&current_dirty,
	);
	let dirty_evaluation =
		super::evaluate_workspace_rules_in(current_inventory, &current_dirty, compiled, false);
	let violation_sets = merge_violation_sets(
		compiled,
		previous,
		&dirty_evaluation,
		&previous_dirty,
		current_universe,
	);
	let mut violations = preserved_symbol_violations(
		current_inventory,
		previous,
		&dirty_identities,
		current_universe,
	);
	violations.extend(
		dirty_evaluation
			.violations
			.into_iter()
			.filter(|violation| violation.source_suppression),
	);
	let (groups, group_violations, affected_groups) =
		group::evaluate_groups_incremental(group::GroupIncrementalInput {
			previous_inventory,
			current_inventory,
			previous_universe,
			current_universe,
			previous_dirty: &previous_dirty,
			current_dirty: &current_dirty,
			compiled,
			previous,
		});
	violations.extend(group_violations);
	super::sort_workspace_violations(&mut violations);
	WorkspaceIncrementalEvaluation {
		evaluation: WorkspaceEvaluation {
			violations,
			violation_sets,
			groups,
			reports: Vec::new(),
		},
		dirty_symbols: previous_dirty.union(&current_dirty).len(),
		evaluated_symbols: current_dirty.len(),
		affected_groups,
	}
}

fn merge_violation_sets(
	compiled: &CompiledWorkspaceRules,
	previous: &WorkspaceEvaluation,
	dirty: &WorkspaceEvaluation,
	previous_dirty: &SymbolSet,
	current_universe: &SymbolSet,
) -> std::collections::BTreeMap<String, SymbolSet> {
	compiled
		.symbol
		.iter()
		.map(|rule| {
			let mut violations = previous
				.violation_sets
				.get(&rule.rule_id)
				.cloned()
				.unwrap_or_default();
			violations.remove_all(previous_dirty);
			violations.intersect_with(current_universe);
			if let Some(changed) = dirty.violation_sets.get(&rule.rule_id) {
				violations.union_with(changed);
			}
			(rule.rule_id.clone(), violations)
		})
		.collect()
}

fn dirty_symbol_sets(
	previous: &SymbolInventoryIndex,
	current: &SymbolInventoryIndex,
	diff: &CodeIndexGraphDiff,
) -> (SymbolSet, SymbolSet) {
	let mut previous_dirty = SymbolSet::new();
	let mut current_dirty = SymbolSet::new();
	for id in &diff.removed_symbols {
		if let Some(ordinal) = previous.catalog().ordinal(id) {
			previous_dirty.insert(ordinal);
		}
	}
	for identity in &diff.modified_symbol_identities {
		if let Some(ordinal) = previous.catalog().ordinal_by_identity(identity) {
			previous_dirty.insert(ordinal);
		}
	}
	for (before, after) in &diff.symbol_id_remaps {
		if let Some(ordinal) = previous.catalog().ordinal(before) {
			previous_dirty.insert(ordinal);
		}
		if let Some(ordinal) = current.catalog().ordinal(after) {
			current_dirty.insert(ordinal);
		}
	}
	for id in diff.added_symbols.iter().chain(&diff.modified_symbols) {
		if let Some(ordinal) = current.catalog().ordinal(id) {
			current_dirty.insert(ordinal);
		}
	}
	(previous_dirty, current_dirty)
}

fn dirty_identities(
	previous: &SymbolInventoryIndex,
	current: &SymbolInventoryIndex,
	previous_dirty: &SymbolSet,
	current_dirty: &SymbolSet,
) -> HashSet<String> {
	previous_dirty
		.iter()
		.filter_map(|ordinal| previous.record(ordinal))
		.chain(
			current_dirty
				.iter()
				.filter_map(|ordinal| current.record(ordinal)),
		)
		.map(|record| record.identity.to_string())
		.collect()
}

fn preserved_symbol_violations(
	current: &SymbolInventoryIndex,
	previous: &WorkspaceEvaluation,
	dirty_identities: &HashSet<String>,
	current_universe: &SymbolSet,
) -> Vec<WorkspaceSymbolViolation> {
	previous
		.violations
		.iter()
		.filter(|violation| violation.source_suppression)
		.filter(|violation| !dirty_identities.contains(&violation.violation.moniker))
		.filter_map(|violation| {
			let record = current
				.catalog()
				.ordinal_by_identity(&violation.violation.moniker)
				.filter(|ordinal| current_universe.contains(*ordinal))
				.and_then(|ordinal| current.record(ordinal))?;
			let mut violation = violation.clone();
			violation.source = record.source;
			violation.symbol = Some(record.id);
			violation.violation.lines = record.line_range.unwrap_or((0, 0));
			Some(violation)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;
	use std::sync::Arc;

	use code_moniker_workspace::snapshot::{
		RecordTable, ResourceGeneration, SourceFileRecord, SourceId, SymbolId, SymbolRecord,
	};

	use super::*;

	fn source(file: usize) -> SourceFileRecord {
		let path = format!("src/main/java/com/acme/Type{file}.java");
		SourceFileRecord {
			id: SourceId::at(file),
			uri: path.clone(),
			source_root: 0,
			path: path.clone(),
			rel_path: path.clone(),
			anchor: path,
			language: "java".to_string(),
			text: String::new(),
		}
	}

	fn symbol(file: usize, def: usize, name: &str, visibility: &str) -> SymbolRecord {
		let mut symbol =
			SymbolRecord::new(SymbolId::at(file, def), SourceId::at(file), name, "class");
		symbol.identity = Arc::from(format!(
			"code+moniker://./lang:java/srcset:main/package:com/package:acme/class:{name}"
		));
		symbol.visibility = visibility.to_string();
		symbol.line_range = Some((file as u32 + 1, file as u32 + 1));
		symbol
	}

	fn inventory(
		generation: u64,
		shards: Vec<Vec<SymbolRecord>>,
		previous: Option<&SymbolInventoryIndex>,
	) -> SymbolInventoryIndex {
		let sources = (0..shards.len()).map(source).collect::<Vec<_>>();
		let records =
			RecordTable::from_shards(shards.into_iter().map(Arc::from).collect::<Vec<_>>());
		previous.map_or_else(
			|| SymbolInventoryIndex::build(ResourceGeneration::new(generation), &sources, &records),
			|previous| {
				previous.refresh(
					ResourceGeneration::new(generation),
					&sources,
					&records,
					&(0..sources.len()).collect::<BTreeSet<_>>(),
				)
			},
		)
	}

	fn assert_equivalent(actual: &WorkspaceEvaluation, expected: &WorkspaceEvaluation) {
		assert_eq!(actual.violation_sets, expected.violation_sets);
		assert_eq!(actual.groups, expected.groups);
		let summarize = |evaluation: &WorkspaceEvaluation| {
			evaluation
				.violations
				.iter()
				.map(|violation| {
					(
						violation.violation.rule_id.clone(),
						violation.violation.moniker.clone(),
						violation.symbol,
						violation.source,
						violation.violation.lines,
					)
				})
				.collect::<Vec<_>>()
		};
		assert_eq!(summarize(actual), summarize(expected));
	}

	#[test]
	fn incremental_symbol_bitmaps_cover_add_modify_remove_and_remap() {
		let before = inventory(
			1,
			vec![
				vec![symbol(0, 0, "Stable", "public")],
				vec![symbol(1, 0, "Modified", "private")],
				vec![symbol(2, 0, "Removed", "private")],
				vec![symbol(3, 0, "Remapped", "private")],
				Vec::new(),
			],
			None,
		);
		let after = inventory(
			2,
			vec![
				vec![symbol(0, 0, "Stable", "public")],
				vec![symbol(1, 0, "Modified", "public")],
				Vec::new(),
				vec![symbol(3, 1, "Remapped", "private")],
				vec![symbol(4, 0, "Added", "private")],
			],
			Some(&before),
		);
		let cfg = crate::check::config::load_from_str(
			r#"
			[[workspace.symbol.where]]
			id = "public-only"
			expr = "visibility = 'public'"
			"#,
			"<test>",
			Some(false),
		)
		.expect("workspace symbol config");
		let compiled =
			super::super::compile_workspace_rules(&cfg, "code+moniker://").expect("symbol plan");
		let previous = super::super::evaluate_workspace_rules(&before, &compiled, false);
		let diff = CodeIndexGraphDiff {
			added_symbols: vec![SymbolId::at(4, 0)],
			modified_symbols: vec![SymbolId::at(1, 0)],
			removed_symbols: vec![SymbolId::at(2, 0)],
			modified_symbol_identities: vec![
				"code+moniker://./lang:java/srcset:main/package:com/package:acme/class:Modified"
					.to_string(),
			],
			symbol_id_remaps: vec![(SymbolId::at(3, 0), SymbolId::at(3, 1))],
			..CodeIndexGraphDiff::default()
		};
		let incremental = evaluate_workspace_rules_incremental(WorkspaceIncrementalInput {
			previous_inventory: &before,
			current_inventory: &after,
			previous_universe: before.all_symbols(),
			current_universe: after.all_symbols(),
			diff: &diff,
			compiled: &compiled,
			previous: &previous,
		});
		let cold = super::super::evaluate_workspace_rules(&after, &compiled, false);

		assert_equivalent(&incremental.evaluation, &cold);
		assert_eq!(incremental.dirty_symbols, 4);
		assert_eq!(incremental.evaluated_symbols, 3);
		let violating_ids = incremental
			.evaluation
			.violations
			.iter()
			.filter_map(|violation| violation.symbol)
			.collect::<BTreeSet<_>>();
		assert_eq!(
			violating_ids,
			BTreeSet::from([SymbolId::at(3, 1), SymbolId::at(4, 0)])
		);
	}
}
