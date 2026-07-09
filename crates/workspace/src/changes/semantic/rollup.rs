use std::path::PathBuf;

use super::model::{ChangeFacets, SemanticKind, SymbolChange};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileDisposition {
	Added,
	Removed,
	Moved { pure: bool },
	Modified,
}

impl FileDisposition {
	pub fn label(self) -> &'static str {
		match self {
			Self::Added => "added",
			Self::Removed => "removed",
			Self::Moved { pure: true } => "moved",
			Self::Moved { pure: false } => "moved-and-modified",
			Self::Modified => "modified",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRollup {
	pub old_path: Option<PathBuf>,
	pub new_path: Option<PathBuf>,
	pub disposition: FileDisposition,
	pub symbol_changes: usize,
	pub moved_symbols: usize,
}

impl FileRollup {
	pub fn pure_move(old_path: PathBuf, new_path: PathBuf, moved_symbols: usize) -> Self {
		Self {
			old_path: Some(old_path),
			new_path: Some(new_path),
			disposition: FileDisposition::Moved { pure: true },
			symbol_changes: 0,
			moved_symbols,
		}
	}
}

pub fn moved_file_rollup(
	old_path: PathBuf,
	new_path: PathBuf,
	changes: &[SymbolChange],
) -> FileRollup {
	let moved_symbols = changes
		.iter()
		.filter(|change| change.kind == SemanticKind::Moved)
		.count();
	let symbol_changes = changes
		.iter()
		.filter(|change| !is_pure_move(change))
		.count();
	FileRollup {
		old_path: Some(old_path),
		new_path: Some(new_path),
		disposition: FileDisposition::Moved {
			pure: symbol_changes == 0,
		},
		symbol_changes,
		moved_symbols,
	}
}

fn is_pure_move(change: &SymbolChange) -> bool {
	change.kind == SemanticKind::Moved
		&& change.facets
			== ChangeFacets {
				file_moved: change.facets.file_moved,
				..ChangeFacets::default()
			}
}

#[cfg(test)]
mod tests {
	use super::super::model::Confidence;
	use super::*;

	fn moved_change(facets: ChangeFacets) -> SymbolChange {
		SymbolChange {
			kind: SemanticKind::Moved,
			confidence: Confidence::Certain,
			facets,
			old: None,
			new: None,
		}
	}

	#[test]
	fn all_pure_moves_roll_up_as_a_pure_file_move() {
		let facets = ChangeFacets {
			file_moved: true,
			..ChangeFacets::default()
		};
		let rollup = moved_file_rollup(
			PathBuf::from("src/old.rs"),
			PathBuf::from("src/new.rs"),
			&[moved_change(facets), moved_change(facets)],
		);

		assert_eq!(rollup.disposition, FileDisposition::Moved { pure: true });
		assert_eq!(rollup.moved_symbols, 2);
		assert_eq!(rollup.symbol_changes, 0);
	}

	#[test]
	fn an_edited_symbol_makes_the_move_impure() {
		let pure = ChangeFacets {
			file_moved: true,
			..ChangeFacets::default()
		};
		let edited = ChangeFacets {
			file_moved: true,
			body_changed: true,
			..ChangeFacets::default()
		};
		let rollup = moved_file_rollup(
			PathBuf::from("src/old.rs"),
			PathBuf::from("src/new.rs"),
			&[moved_change(pure), moved_change(edited)],
		);

		assert_eq!(rollup.disposition, FileDisposition::Moved { pure: false });
		assert_eq!(rollup.symbol_changes, 1);
	}
}
