use std::collections::BTreeMap;

use code_moniker_workspace::snapshot::{SourceFileRecord, SymbolRecord, WorkspaceSnapshot};

use super::model::{Note, NoteResolution, ResolvedNote};

pub(crate) fn resolve_notes(notes: &[Note], snapshot: &WorkspaceSnapshot) -> Vec<ResolvedNote> {
	let sources = source_by_id(snapshot);
	let symbols = symbol_by_identity(snapshot);
	notes
		.iter()
		.cloned()
		.map(|note| {
			let resolution = resolve_note(&note, &symbols, &sources);
			ResolvedNote { note, resolution }
		})
		.collect()
}

fn resolve_note(
	note: &Note,
	symbols: &BTreeMap<&str, &SymbolRecord>,
	sources: &BTreeMap<&str, &SourceFileRecord>,
) -> NoteResolution {
	let Some(symbol) = symbols.get(note.moniker.as_str()).copied() else {
		return NoteResolution::Orphan;
	};
	let Some(source) = sources.get(symbol.source.as_str()) else {
		return NoteResolution::Orphan;
	};
	NoteResolution::Resolved {
		target_label: format!("{} {}", symbol.kind, symbol.name),
		target_file: source.rel_path.clone(),
		target_slice: symbol.line_range,
	}
}

fn source_by_id(snapshot: &WorkspaceSnapshot) -> BTreeMap<&str, &SourceFileRecord> {
	snapshot
		.index
		.sources
		.iter()
		.map(|source| (source.id.as_str(), source))
		.collect()
}

fn symbol_by_identity(snapshot: &WorkspaceSnapshot) -> BTreeMap<&str, &SymbolRecord> {
	snapshot
		.index
		.symbols
		.iter()
		.map(|symbol| (symbol.identity.as_str(), symbol))
		.collect()
}

#[cfg(test)]
mod tests {
	use code_moniker_workspace::snapshot::{
		ChangeOverlay, CodeIndex, CodeIndexFields, CodeIndexTimings, LinkageGraph,
		ResourceGeneration, SourceCatalog, SourceFileRecord, SourceFileRecordFields, SourceId,
		SymbolId, SymbolRecord, SymbolRecordFields, WorkspaceSnapshot, WorkspaceTimings,
	};

	use super::*;
	use crate::notes::model::{NoteAuthor, NoteId, NoteKind, NoteStatus};

	#[test]
	fn resolves_matching_moniker_and_keeps_orphans() {
		let moniker = "code+moniker://./lang:rs/module:example/fn:run()";
		let notes = vec![
			sample_note("note_1", moniker),
			sample_note("note_2", "code+moniker://./lang:rs/module:missing"),
		];
		let snapshot = snapshot_with_symbol(moniker);

		let resolved = resolve_notes(&notes, &snapshot);

		assert_eq!(resolved.len(), 2);
		assert_eq!(
			resolved[0].resolution,
			NoteResolution::Resolved {
				target_label: "fn run()".to_string(),
				target_file: "src/lib.rs".to_string(),
				target_slice: Some((3, 7)),
			}
		);
		assert!(resolved[1].resolution.is_orphan());
	}

	#[test]
	fn treats_symbol_without_source_as_orphan() {
		let moniker = "code+moniker://./lang:rs/module:example/fn:run()";
		let notes = vec![sample_note("note_1", moniker)];
		let snapshot = snapshot_with_orphaned_symbol(moniker);

		let resolved = resolve_notes(&notes, &snapshot);

		assert!(resolved[0].resolution.is_orphan());
	}

	fn sample_note(id: &str, moniker: &str) -> Note {
		Note {
			id: NoteId::new(id),
			moniker: moniker.to_string(),
			kind: NoteKind::Todo,
			status: NoteStatus::Pending,
			title: "Title".to_string(),
			body: "Body".to_string(),
			created_by: NoteAuthor::User,
			created_at: "2026-06-02T00:00:00Z".to_string(),
			updated_at: "2026-06-02T00:00:00Z".to_string(),
		}
	}

	fn snapshot_with_symbol(moniker: &str) -> WorkspaceSnapshot {
		let source = SourceFileRecord::from_fields(SourceFileRecordFields {
			id: SourceId::new("source:1"),
			uri: "code+moniker://./lang:rs/path:src/path:lib.rs".to_string(),
			source_root: 0,
			path: "/tmp/src/lib.rs".to_string(),
			rel_path: "src/lib.rs".to_string(),
			anchor: ".".to_string(),
			language: "rs".to_string(),
			text: "pub fn run() {}\n".to_string(),
		});
		let symbol = SymbolRecord::from_fields(SymbolRecordFields {
			id: SymbolId::new("symbol:1"),
			source: source.id.clone(),
			identity: moniker.to_string(),
			name: "run()".to_string(),
			kind: "fn".to_string(),
			visibility: "public".to_string(),
			signature: "run()".to_string(),
			navigable: true,
			line_range: Some((3, 7)),
			parent: None,
		});
		WorkspaceSnapshot {
			generation: ResourceGeneration::new(1),
			catalog: SourceCatalog::new(ResourceGeneration::new(1), Vec::new()),
			index: CodeIndex::from_fields(CodeIndexFields {
				generation: ResourceGeneration::new(1),
				catalog_generation: ResourceGeneration::new(1),
				identity_scheme: "code+moniker://".to_string(),
				sources: vec![source],
				symbols: vec![symbol],
				references: Vec::new(),
				timings: CodeIndexTimings::default(),
			}),
			linkage: LinkageGraph::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				0,
				0,
			),
			changes: ChangeOverlay::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				Vec::new(),
			),
			timings: WorkspaceTimings::default(),
		}
	}

	fn snapshot_with_orphaned_symbol(moniker: &str) -> WorkspaceSnapshot {
		let symbol = SymbolRecord::from_fields(SymbolRecordFields {
			id: SymbolId::new("symbol:1"),
			source: SourceId::new("missing-source"),
			identity: moniker.to_string(),
			name: "run()".to_string(),
			kind: "fn".to_string(),
			visibility: "public".to_string(),
			signature: "run()".to_string(),
			navigable: true,
			line_range: Some((3, 7)),
			parent: None,
		});
		WorkspaceSnapshot {
			generation: ResourceGeneration::new(1),
			catalog: SourceCatalog::new(ResourceGeneration::new(1), Vec::new()),
			index: CodeIndex::from_fields(CodeIndexFields {
				generation: ResourceGeneration::new(1),
				catalog_generation: ResourceGeneration::new(1),
				identity_scheme: "code+moniker://".to_string(),
				sources: Vec::new(),
				symbols: vec![symbol],
				references: Vec::new(),
				timings: CodeIndexTimings::default(),
			}),
			linkage: LinkageGraph::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				0,
				0,
			),
			changes: ChangeOverlay::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				Vec::new(),
			),
			timings: WorkspaceTimings::default(),
		}
	}
}
