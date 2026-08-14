use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::lang::Lang;

use crate::environment::{self, ExtractContext};

use super::model::{HunkCoverage, RefChange, SymbolChange};
use super::pairing::{FilePairing, FileSide, PairInputs, finish_files, pair_file};
use super::refpairs::{CoverageInputs, RenameContext, hunk_coverage, pair_refs};
use super::review::{FileFacts, SemanticReview};
use super::rollup::{FileDisposition, FileRollup, moved_file_rollup};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtualDiffImpactFileStatus {
	Added,
	Modified,
	Deleted,
	Renamed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualDiffImpactDocument {
	pub uri: String,
	pub lang: Lang,
	pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualDiffImpactFile {
	pub status: VirtualDiffImpactFileStatus,
	pub old_uri: Option<String>,
	pub new_uri: Option<String>,
	pub old_hunks: Vec<(u32, u32)>,
	pub new_hunks: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualDiffImpactInput {
	pub scope: String,
	pub project: Option<String>,
	pub srcset: String,
	pub base: Vec<VirtualDiffImpactDocument>,
	pub head: Vec<VirtualDiffImpactDocument>,
	pub files: Vec<VirtualDiffImpactFile>,
}

struct ExtractedDocument {
	path: PathBuf,
	lang: Lang,
	content: String,
	graph: CodeGraph,
}

struct DiffImpactPair<'a> {
	old: &'a ExtractedDocument,
	new: &'a ExtractedDocument,
	status: VirtualDiffImpactFileStatus,
	old_hunks: &'a [(u32, u32)],
	new_hunks: &'a [(u32, u32)],
}

impl DiffImpactPair<'_> {
	fn old_side(&self) -> FileSide<'_> {
		FileSide {
			lang: self.old.lang,
			graph: &self.old.graph,
			source: &self.old.content,
			file_path: &self.old.path,
		}
	}

	fn new_side(&self) -> FileSide<'_> {
		FileSide {
			lang: self.new.lang,
			graph: &self.new.graph,
			source: &self.new.content,
			file_path: &self.new.path,
		}
	}

	fn moved(&self) -> bool {
		self.status == VirtualDiffImpactFileStatus::Renamed || self.old.path != self.new.path
	}
}

pub fn build_virtual_diff_impact(input: VirtualDiffImpactInput) -> Result<SemanticReview, String> {
	let context = ExtractContext {
		project: input.project,
		srcset: Some(input.srcset),
		..ExtractContext::default()
	};
	let mut base = extract_documents(input.base, &context)?;
	let mut head = extract_documents(input.head, &context)?;
	let mut empties = Vec::new();
	for file in &input.files {
		ensure_empty_sides(file, &base, &head, &context, &mut empties)?;
	}
	let pairs = pair_documents(&input.files, &base, &head, &empties)?;
	let pairings: Vec<FilePairing> = pairs
		.iter()
		.map(|pair| {
			pair_file(PairInputs {
				base: pair.old_side(),
				current: pair.new_side(),
				file_moved: pair.moved(),
			})
		})
		.collect();
	let symbol_changes = finish_files(pairings);
	let mut rename_context = RenameContext::from_changes(&symbol_changes);
	for pair in pairs.iter().filter(|pair| pair.moved()) {
		rename_context.push_pair(pair.old.graph.root().clone(), pair.new.graph.root().clone());
	}
	let mut impact = SemanticReview {
		scope: input.scope,
		symbol_changes,
		..SemanticReview::default()
	};
	for pair in &pairs {
		let refs = pair_refs(&pair.old_side(), &pair.new_side(), &rename_context);
		impact
			.files
			.push(file_facts(pair, &impact.symbol_changes, &refs));
		impact.ref_changes.extend(refs);
	}
	impact.files.sort_by_key(|facts| {
		facts
			.rollup
			.new_path
			.clone()
			.or_else(|| facts.rollup.old_path.clone())
	});
	base.clear();
	head.clear();
	Ok(impact)
}

fn extract_documents(
	documents: Vec<VirtualDiffImpactDocument>,
	context: &ExtractContext,
) -> Result<BTreeMap<String, ExtractedDocument>, String> {
	let mut out = BTreeMap::new();
	for document in documents {
		if out.contains_key(&document.uri) {
			return Err(format!(
				"duplicate virtual diff impact URI `{}`",
				document.uri
			));
		}
		let path = PathBuf::from(&document.uri);
		let graph =
			environment::extract_source_with(document.lang, &document.content, &path, context);
		out.insert(
			document.uri,
			ExtractedDocument {
				path,
				lang: document.lang,
				content: document.content,
				graph,
			},
		);
	}
	Ok(out)
}

fn ensure_empty_sides(
	file: &VirtualDiffImpactFile,
	base: &BTreeMap<String, ExtractedDocument>,
	head: &BTreeMap<String, ExtractedDocument>,
	context: &ExtractContext,
	empties: &mut Vec<ExtractedDocument>,
) -> Result<(), String> {
	let (missing_uri, reference) = match file.status {
		VirtualDiffImpactFileStatus::Added => (
			file.old_uri.as_deref().or(file.new_uri.as_deref()),
			lookup(head, file.new_uri.as_deref())?,
		),
		VirtualDiffImpactFileStatus::Deleted => (
			file.new_uri.as_deref().or(file.old_uri.as_deref()),
			lookup(base, file.old_uri.as_deref())?,
		),
		_ => return Ok(()),
	};
	let uri =
		missing_uri.ok_or_else(|| "virtual diff impact file is missing its path".to_string())?;
	let path = PathBuf::from(uri);
	empties.push(ExtractedDocument {
		path: path.clone(),
		lang: reference.lang,
		content: String::new(),
		graph: environment::extract_source_with(reference.lang, "", &path, context),
	});
	Ok(())
}

fn pair_documents<'a>(
	files: &'a [VirtualDiffImpactFile],
	base: &'a BTreeMap<String, ExtractedDocument>,
	head: &'a BTreeMap<String, ExtractedDocument>,
	empties: &'a [ExtractedDocument],
) -> Result<Vec<DiffImpactPair<'a>>, String> {
	let mut empty_idx = 0usize;
	let pairs: Vec<DiffImpactPair<'a>> = files
		.iter()
		.map(|file| {
			let (old, new) = match file.status {
				VirtualDiffImpactFileStatus::Added => {
					let empty = empties
						.get(empty_idx)
						.ok_or_else(|| "missing empty base side".to_string())?;
					empty_idx += 1;
					(empty, lookup(head, file.new_uri.as_deref())?)
				}
				VirtualDiffImpactFileStatus::Deleted => {
					let empty = empties
						.get(empty_idx)
						.ok_or_else(|| "missing empty head side".to_string())?;
					empty_idx += 1;
					(lookup(base, file.old_uri.as_deref())?, empty)
				}
				VirtualDiffImpactFileStatus::Modified | VirtualDiffImpactFileStatus::Renamed => (
					lookup(base, file.old_uri.as_deref())?,
					lookup(head, file.new_uri.as_deref())?,
				),
			};
			if old.lang != new.lang {
				return Err(format!(
					"language changed between `{}` and `{}`",
					old.path.display(),
					new.path.display()
				));
			}
			Ok(DiffImpactPair {
				old,
				new,
				status: file.status,
				old_hunks: &file.old_hunks,
				new_hunks: &file.new_hunks,
			})
		})
		.collect::<Result<_, _>>()?;
	validate_document_coverage(files, base, head)?;
	Ok(pairs)
}

fn validate_document_coverage(
	files: &[VirtualDiffImpactFile],
	base: &BTreeMap<String, ExtractedDocument>,
	head: &BTreeMap<String, ExtractedDocument>,
) -> Result<(), String> {
	let mut old_uris = BTreeSet::new();
	let mut new_uris = BTreeSet::new();
	for file in files {
		if let Some(uri) = &file.old_uri
			&& !old_uris.insert(uri.as_str())
		{
			return Err(format!("duplicate old diff-impact URI `{uri}`"));
		}
		if let Some(uri) = &file.new_uri
			&& !new_uris.insert(uri.as_str())
		{
			return Err(format!("duplicate new diff-impact URI `{uri}`"));
		}
	}
	if let Some(uri) = base.keys().find(|uri| !old_uris.contains(uri.as_str())) {
		return Err(format!(
			"base diff-impact document `{uri}` is absent from the file inventory"
		));
	}
	if let Some(uri) = head.keys().find(|uri| !new_uris.contains(uri.as_str())) {
		return Err(format!(
			"head diff-impact document `{uri}` is absent from the file inventory"
		));
	}
	Ok(())
}

fn lookup<'a>(
	documents: &'a BTreeMap<String, ExtractedDocument>,
	uri: Option<&str>,
) -> Result<&'a ExtractedDocument, String> {
	let uri = uri.ok_or_else(|| "virtual diff impact file is missing its path".to_string())?;
	documents
		.get(uri)
		.ok_or_else(|| format!("virtual diff impact document `{uri}` is missing"))
}

fn file_facts(
	pair: &DiffImpactPair<'_>,
	changes: &[SymbolChange],
	refs: &[RefChange],
) -> FileFacts {
	let file_changes: Vec<SymbolChange> = changes
		.iter()
		.filter(|change| {
			change
				.old
				.as_ref()
				.is_some_and(|side| side.file_path == pair.old.path)
				|| change
					.new
					.as_ref()
					.is_some_and(|side| side.file_path == pair.new.path)
		})
		.cloned()
		.collect();
	let coverage = coverage(pair, &file_changes, refs);
	let disposition = match pair.status {
		VirtualDiffImpactFileStatus::Added => FileDisposition::Added,
		VirtualDiffImpactFileStatus::Deleted => FileDisposition::Removed,
		VirtualDiffImpactFileStatus::Modified => FileDisposition::Modified,
		VirtualDiffImpactFileStatus::Renamed => FileDisposition::Moved { pure: true },
	};
	let mut rollup = if pair.moved() {
		moved_file_rollup(pair.old.path.clone(), pair.new.path.clone(), &file_changes)
	} else {
		FileRollup {
			old_path: (pair.status != VirtualDiffImpactFileStatus::Added)
				.then(|| pair.old.path.clone()),
			new_path: (pair.status != VirtualDiffImpactFileStatus::Deleted)
				.then(|| pair.new.path.clone()),
			disposition,
			symbol_changes: file_changes.len(),
			moved_symbols: 0,
		}
	};
	if rollup.disposition == (FileDisposition::Moved { pure: true }) && !coverage.explained() {
		rollup.disposition = FileDisposition::Moved { pure: false };
	}
	FileFacts {
		rollup,
		coverage,
		analyzable: true,
	}
}

fn coverage(
	pair: &DiffImpactPair<'_>,
	changes: &[SymbolChange],
	refs: &[RefChange],
) -> HunkCoverage {
	let old_explained = explained_ranges(changes, refs, true, &pair.old.path);
	let new_explained = explained_ranges(changes, refs, false, &pair.new.path);
	hunk_coverage(CoverageInputs {
		old_hunks: pair.old_hunks,
		new_hunks: pair.new_hunks,
		old_explained: &old_explained,
		new_explained: &new_explained,
	})
}

fn explained_ranges(
	changes: &[SymbolChange],
	refs: &[RefChange],
	old: bool,
	path: &Path,
) -> Vec<(u32, u32)> {
	let mut ranges: Vec<(u32, u32)> = changes
		.iter()
		.filter_map(|change| {
			if old {
				change.old.as_ref()
			} else {
				change.new.as_ref()
			}
		})
		.filter(|side| side.file_path == path)
		.filter_map(|side| side.line_range)
		.collect();
	for reference in refs.iter().filter(|reference| reference.file_path == path) {
		if let Some(range) = if old {
			reference.old_line_range
		} else {
			reference.new_line_range
		} {
			ranges.push(range);
		}
	}
	ranges
}

#[cfg(test)]
mod tests {
	use super::super::model::SemanticKind;
	use super::*;

	fn document(uri: &str, content: &str) -> VirtualDiffImpactDocument {
		VirtualDiffImpactDocument {
			uri: uri.to_string(),
			lang: Lang::Rs,
			content: content.to_string(),
		}
	}

	#[test]
	fn compares_virtual_documents_without_a_workspace() {
		let impact = build_virtual_diff_impact(VirtualDiffImpactInput {
			scope: "base..head".to_string(),
			project: Some("sample".to_string()),
			srcset: "diff-impact".to_string(),
			base: vec![document(
				"src/lib.rs",
				"pub fn kept() { old(); }\npub fn removed() { obsolete(); }\n",
			)],
			head: vec![document(
				"src/lib.rs",
				"pub fn kept() { fresh(); }\npub fn added() { created(); }\n",
			)],
			files: vec![VirtualDiffImpactFile {
				status: VirtualDiffImpactFileStatus::Modified,
				old_uri: Some("src/lib.rs".to_string()),
				new_uri: Some("src/lib.rs".to_string()),
				old_hunks: vec![(1, 2)],
				new_hunks: vec![(1, 2)],
			}],
		})
		.expect("virtual diff impact");

		assert!(
			impact
				.symbol_changes
				.iter()
				.any(|change| change.kind == SemanticKind::BodyModified)
		);
		assert!(
			impact
				.symbol_changes
				.iter()
				.any(|change| change.kind == SemanticKind::Removed)
		);
		assert!(
			impact
				.symbol_changes
				.iter()
				.any(|change| change.kind == SemanticKind::Added)
		);
		assert!(!impact.ref_changes.is_empty());
	}

	#[test]
	fn compares_added_and_deleted_virtual_files() {
		let impact = build_virtual_diff_impact(VirtualDiffImpactInput {
			scope: "base..head".to_string(),
			project: Some("sample".to_string()),
			srcset: "diff-impact".to_string(),
			base: vec![document(
				"src/removed.rs",
				"pub fn removed_file_symbol() {}\n",
			)],
			head: vec![document("src/added.rs", "pub fn added_file_symbol() {}\n")],
			files: vec![
				VirtualDiffImpactFile {
					status: VirtualDiffImpactFileStatus::Deleted,
					old_uri: Some("src/removed.rs".to_string()),
					new_uri: None,
					old_hunks: vec![(1, 1)],
					new_hunks: vec![],
				},
				VirtualDiffImpactFile {
					status: VirtualDiffImpactFileStatus::Added,
					old_uri: None,
					new_uri: Some("src/added.rs".to_string()),
					old_hunks: vec![],
					new_hunks: vec![(1, 1)],
				},
			],
		})
		.expect("virtual diff impact");

		assert_eq!(impact.files.len(), 2);
		assert!(
			impact
				.files
				.iter()
				.any(|file| file.rollup.disposition == FileDisposition::Added)
		);
		assert!(
			impact
				.files
				.iter()
				.any(|file| file.rollup.disposition == FileDisposition::Removed)
		);
		assert!(
			impact
				.symbol_changes
				.iter()
				.any(|change| change.kind == SemanticKind::Added)
		);
		assert!(
			impact
				.symbol_changes
				.iter()
				.any(|change| change.kind == SemanticKind::Removed)
		);
	}

	#[test]
	fn rejects_documents_missing_from_the_authoritative_file_inventory() {
		let error = build_virtual_diff_impact(VirtualDiffImpactInput {
			scope: "base..head".to_string(),
			project: Some("sample".to_string()),
			srcset: "diff-impact".to_string(),
			base: vec![document("src/lib.rs", "pub fn hidden() {}\n")],
			head: vec![document("src/lib.rs", "pub fn hidden() {}\n")],
			files: vec![],
		})
		.expect_err("unlisted documents must fail closed");
		assert!(error.contains("absent from the file inventory"), "{error}");
	}

	#[test]
	fn a_pure_file_rename_is_not_reported_as_add_remove() {
		let source = "pub struct Service;\nimpl Service { pub fn run(&self) {} }\n";
		let impact = build_virtual_diff_impact(VirtualDiffImpactInput {
			scope: "base..head".to_string(),
			project: Some("sample".to_string()),
			srcset: "diff-impact".to_string(),
			base: vec![document("src/old.rs", source)],
			head: vec![document("src/new.rs", source)],
			files: vec![VirtualDiffImpactFile {
				status: VirtualDiffImpactFileStatus::Renamed,
				old_uri: Some("src/old.rs".to_string()),
				new_uri: Some("src/new.rs".to_string()),
				old_hunks: vec![],
				new_hunks: vec![],
			}],
		})
		.expect("virtual diff impact");

		assert!(
			impact
				.symbol_changes
				.iter()
				.all(|change| change.kind == SemanticKind::Moved)
		);
		assert_eq!(
			impact.files[0].rollup.disposition,
			FileDisposition::Moved { pure: true }
		);
	}
}
