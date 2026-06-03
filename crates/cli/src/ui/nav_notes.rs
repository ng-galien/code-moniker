use crate::ui::store::navigation_tree::{NavNodeKind, NavRow};
use crate::ui::workspace_read::{self, LocalWorkspaceRegistry};

pub(in crate::ui) struct NavNoteTarget {
	pub(in crate::ui) moniker: String,
	pub(in crate::ui) label: String,
}

pub(in crate::ui) fn nav_row_note_target(
	workspace: &LocalWorkspaceRegistry,
	row: &NavRow,
	scheme: &str,
) -> Option<NavNoteTarget> {
	match &row.kind {
		NavNodeKind::Def(loc) => {
			let summary = workspace_read::symbol_summary(workspace, loc);
			if summary.identity.is_empty() {
				Some(navigation_row_target(row, scheme))
			} else {
				Some(NavNoteTarget {
					moniker: summary.identity,
					label: format!("{} {}", summary.kind, summary.name),
				})
			}
		}
		NavNodeKind::File(file_idx) => workspace
			.queries()
			.snapshot()?
			.index
			.sources
			.get(*file_idx)
			.map(|source| NavNoteTarget {
				moniker: source.uri.clone(),
				label: row.label.clone(),
			}),
		NavNodeKind::View { id, .. } => Some(NavNoteTarget {
			moniker: format!("{scheme}workspace/views/{id}"),
			label: format!("view {}", row.label),
		}),
		_ => Some(navigation_row_target(row, scheme)),
	}
}

fn navigation_row_target(row: &NavRow, scheme: &str) -> NavNoteTarget {
	NavNoteTarget {
		moniker: format!(
			"{scheme}workspace/navigation/{}",
			encode_navigation_id(&row.key.to_string())
		),
		label: nav_row_note_label(row),
	}
}

fn nav_row_note_label(row: &NavRow) -> String {
	let kind = match row.kind {
		NavNodeKind::Root => "workspace",
		NavNodeKind::Lang => "language",
		NavNodeKind::Dir => "directory",
		NavNodeKind::ChangeFile => "changed file",
		NavNodeKind::Change(_) => "change",
		NavNodeKind::ViewError => "views error",
		NavNodeKind::File(_) | NavNodeKind::Def(_) | NavNodeKind::View { .. } => "node",
	};
	format!("{kind} {}", row.label)
}

fn encode_navigation_id(value: &str) -> String {
	let mut encoded = String::new();
	for byte in value.bytes() {
		match byte {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' | b'~' | b'/' => {
				encoded.push(byte as char);
			}
			_ => encoded.push_str(&format!("%{byte:02X}")),
		}
	}
	encoded
}
