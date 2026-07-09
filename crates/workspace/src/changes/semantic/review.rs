use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::lang::Lang;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::environment;

use super::super::diff::{
	ChangeFile, ChangeScan, DiffHunk, DiffScope, FileDiff, FileDiffStatus, GitWorktree, HeadSide,
	anchor_for, collect_changed_files, diff_path, display_rel_path, git_show, normalize_path,
	resolve_base_rev, source_root_for_path,
};
use super::model::{HunkCoverage, RefChange, SymbolChange};
use super::pairing::{FilePairing, FileSide, PairInputs, finish_files, pair_file};
use super::refpairs::{CoverageInputs, RenameContext, hunk_coverage, pair_refs};
use super::rollup::{FileDisposition, FileRollup, moved_file_rollup};

#[derive(Clone, Debug, Default)]
pub struct SemanticReview {
	pub scope: String,
	pub symbol_changes: Vec<SymbolChange>,
	pub ref_changes: Vec<RefChange>,
	pub files: Vec<FileFacts>,
	pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileFacts {
	pub rollup: FileRollup,
	pub coverage: HunkCoverage,
	pub analyzable: bool,
}

pub struct ReviewDiffs {
	diffs: Vec<FileDiff>,
	pub diagnostics: Vec<String>,
	scope_label: String,
	head_rev: Option<String>,
	base_by_root: FxHashMap<PathBuf, String>,
}

impl ReviewDiffs {
	pub fn current_paths(&self) -> Vec<PathBuf> {
		self.diffs
			.iter()
			.filter(|diff| diff.status != FileDiffStatus::Deleted)
			.map(|diff| normalize_path(&diff_path(diff)))
			.collect()
	}

	pub fn current_rows(&self) -> Vec<(&Path, &Path)> {
		self.diffs
			.iter()
			.filter(|diff| diff.status != FileDiffStatus::Deleted)
			.map(|diff| (diff.repo_root.as_path(), diff.repo_rel.as_path()))
			.collect()
	}

	pub fn head_rev(&self) -> Option<&str> {
		self.head_rev.as_deref()
	}

	pub fn any_root_resolved(&self) -> bool {
		!self.base_by_root.is_empty()
	}

	fn base_rev_for(&self, repo_root: &Path) -> &str {
		self.base_by_root
			.get(repo_root)
			.map(String::as_str)
			.unwrap_or("HEAD")
	}
}

pub fn read_blob(repo_root: &Path, rev: &str, repo_rel: &Path) -> anyhow::Result<String> {
	git_show(repo_root, rev, repo_rel)
}

pub fn collect_review_diffs(roots: &[(String, PathBuf)]) -> ReviewDiffs {
	collect_review_diffs_scoped(roots, &DiffScope::worktree())
}

pub fn collect_review_diffs_scoped(roots: &[(String, PathBuf)], scope: &DiffScope) -> ReviewDiffs {
	let mut review_diffs = ReviewDiffs {
		diffs: Vec::new(),
		diagnostics: Vec::new(),
		scope_label: scope.label(),
		head_rev: match &scope.head {
			HeadSide::Rev(rev) => Some(rev.clone()),
			HeadSide::Worktree => None,
		},
		base_by_root: FxHashMap::default(),
	};
	for (label, path) in roots {
		collect_root_diffs(&mut review_diffs, label, path, scope);
	}
	review_diffs
}

fn collect_root_diffs(out: &mut ReviewDiffs, label: &str, path: &Path, scope: &DiffScope) {
	let repo = match GitWorktree::discover(path) {
		Ok(repo) => repo,
		Err(message) => {
			out.diagnostics.push(message);
			return;
		}
	};
	let base_rev = match resolve_base_rev(repo.root(), &scope.base) {
		Ok(base_rev) => base_rev,
		Err(message) => {
			out.diagnostics.push(format!("{label}: {message}"));
			return;
		}
	};
	match collect_changed_files(repo.root(), path, &base_rev, &scope.head) {
		Ok(mut root_diffs) => out.diffs.append(&mut root_diffs),
		Err(error) => out
			.diagnostics
			.push(format!("{label}: cannot inspect git changes: {error}")),
	}
	out.base_by_root.insert(repo.root().to_path_buf(), base_rev);
}

pub fn build_semantic_review(scan: &ChangeScan<'_>) -> SemanticReview {
	let roots: Vec<(String, PathBuf)> = scan
		.roots
		.iter()
		.map(|root| (root.label.to_string(), root.path.to_path_buf()))
		.collect();
	build_semantic_review_from(scan, &collect_review_diffs(&roots))
}

pub fn build_semantic_review_from(scan: &ChangeScan<'_>, diffs: &ReviewDiffs) -> SemanticReview {
	let mut review = SemanticReview {
		scope: diffs.scope_label.clone(),
		diagnostics: diffs.diagnostics.clone(),
		..SemanticReview::default()
	};
	let pairs = review_pairs(scan, diffs, &mut review);
	let pairings: Vec<FilePairing> = pairs
		.iter()
		.map(|pair| {
			pair_file(PairInputs {
				base: pair.base_side(),
				current: pair.current_side(),
				file_moved: pair.file_moved,
			})
		})
		.collect();
	review.symbol_changes = finish_files(pairings);
	let ctx = rename_context(&review.symbol_changes, &pairs);
	for pair in &pairs {
		let refs = pair_refs(&pair.base_side(), &pair.current_side(), &ctx);
		review
			.files
			.push(pair_facts(pair, &review.symbol_changes, &refs));
		review.ref_changes.extend(refs);
	}
	review.files.sort_by_key(facts_order);
	review
}

type LineSpans = Vec<(u32, u32)>;

fn facts_order(facts: &FileFacts) -> (Option<PathBuf>, Option<PathBuf>) {
	(
		facts
			.rollup
			.new_path
			.clone()
			.or(facts.rollup.old_path.clone()),
		facts.rollup.old_path.clone(),
	)
}

enum CurrentRef<'scan> {
	Scan(&'scan ChangeFile<'scan>),
	Empty { source: String, graph: CodeGraph },
}

struct SidePair<'scan> {
	old_rel: PathBuf,
	new_rel: PathBuf,
	lang: Lang,
	base_source: String,
	base_graph: CodeGraph,
	current: CurrentRef<'scan>,
	old_hunks: Vec<(u32, u32)>,
	new_hunks: Vec<(u32, u32)>,
	file_moved: bool,
	disposition: FileDisposition,
}

impl SidePair<'_> {
	fn base_side(&self) -> FileSide<'_> {
		FileSide {
			lang: self.lang,
			graph: &self.base_graph,
			source: &self.base_source,
			file_path: &self.old_rel,
		}
	}

	fn current_side(&self) -> FileSide<'_> {
		match &self.current {
			CurrentRef::Scan(file) => FileSide {
				lang: self.lang,
				graph: file.graph,
				source: file.source,
				file_path: &self.new_rel,
			},
			CurrentRef::Empty { source, graph } => FileSide {
				lang: self.lang,
				graph,
				source,
				file_path: &self.new_rel,
			},
		}
	}
}

fn review_pairs<'scan>(
	scan: &'scan ChangeScan<'scan>,
	review_diffs: &ReviewDiffs,
	review: &mut SemanticReview,
) -> Vec<SidePair<'scan>> {
	let diffs = &review_diffs.diffs;
	let relevant = scan.relevant_diffs(diffs);
	let matched: FxHashSet<PathBuf> = relevant.by_path.keys().cloned().collect();
	let deleted: FxHashSet<PathBuf> = relevant
		.deleted
		.iter()
		.map(|diff| normalize_path(&diff_path(diff)))
		.collect();
	let rename_origins: FxHashSet<PathBuf> = diffs
		.iter()
		.filter_map(|diff| diff.origin.as_ref())
		.map(|origin| origin.repo_rel.clone())
		.collect();
	let mut pairs = Vec::new();
	for diff in diffs {
		if diff.status == FileDiffStatus::Deleted && rename_origins.contains(&diff.repo_rel) {
			continue;
		}
		let path = normalize_path(&diff_path(diff));
		let base_rev = review_diffs.base_rev_for(&diff.repo_root);
		let outcome = if matched.contains(&path) {
			scan_file_for(scan, &path)
				.ok_or_else(|| format!("changed file left the catalog: {}", path.display()))
				.and_then(|file| matched_pair(scan, diff, file, base_rev))
		} else if deleted.contains(&path) {
			deleted_pair(scan, diff, base_rev)
		} else {
			Err(String::new())
		};
		match outcome {
			Ok(pair) => pairs.push(pair),
			Err(message) => {
				if !message.is_empty() {
					review.diagnostics.push(message);
				}
				review.files.push(opaque_facts(scan, diff));
			}
		}
	}
	pairs
}

fn scan_file_for<'scan>(
	scan: &'scan ChangeScan<'scan>,
	path: &Path,
) -> Option<&'scan ChangeFile<'scan>> {
	scan.files
		.iter()
		.find(|file| normalize_path(file.path) == path)
}

fn matched_pair<'scan>(
	scan: &ChangeScan<'_>,
	diff: &FileDiff,
	file: &'scan ChangeFile<'scan>,
	base_rev: &str,
) -> Result<SidePair<'scan>, String> {
	let (old_hunks, new_hunks) = hunk_spans(&diff.hunks);
	match diff.status {
		FileDiffStatus::Added => Ok(SidePair {
			old_rel: file.rel_path.to_path_buf(),
			new_rel: file.rel_path.to_path_buf(),
			lang: file.lang,
			base_source: String::new(),
			base_graph: extract_at(scan, file, file.anchor, ""),
			current: CurrentRef::Scan(file),
			old_hunks,
			new_hunks,
			file_moved: false,
			disposition: FileDisposition::Added,
		}),
		FileDiffStatus::Renamed => renamed_pair(scan, diff, file, (old_hunks, new_hunks), base_rev),
		_ => {
			let base_source =
				git_show(&diff.repo_root, base_rev, &diff.repo_rel).map_err(|error| {
					format!(
						"{}: cannot read base blob: {error}",
						file.rel_path.display()
					)
				})?;
			let base_graph = extract_at(scan, file, file.anchor, &base_source);
			Ok(SidePair {
				old_rel: file.rel_path.to_path_buf(),
				new_rel: file.rel_path.to_path_buf(),
				lang: file.lang,
				base_source,
				base_graph,
				current: CurrentRef::Scan(file),
				old_hunks,
				new_hunks,
				file_moved: false,
				disposition: FileDisposition::Modified,
			})
		}
	}
}

fn renamed_pair<'scan>(
	scan: &ChangeScan<'_>,
	diff: &FileDiff,
	file: &'scan ChangeFile<'scan>,
	hunks: (LineSpans, LineSpans),
	base_rev: &str,
) -> Result<SidePair<'scan>, String> {
	let origin = diff.origin.as_ref().expect("renamed rows carry an origin");
	let old_abs = diff.repo_root.join(&origin.repo_rel);
	let Some((source_root, root, old_rel)) = source_root_for_path(scan, &old_abs) else {
		return Err(format!(
			"rename origin {} is outside the scanned source roots",
			origin.repo_rel.display()
		));
	};
	let old_anchor = anchor_for(scan, root, &old_rel);
	let old_display = display_rel_path(scan, source_root, root, &old_rel);
	let base_source = if origin.score == 100 {
		file.source.to_string()
	} else {
		git_show(&diff.repo_root, base_rev, &origin.repo_rel)
			.map_err(|error| format!("{}: cannot read base blob: {error}", old_display.display()))?
	};
	let base_graph =
		environment::extract_source_with(file.lang, &base_source, &old_anchor, root.ctx);
	Ok(SidePair {
		old_rel: old_display,
		new_rel: file.rel_path.to_path_buf(),
		lang: file.lang,
		base_source,
		base_graph,
		current: CurrentRef::Scan(file),
		old_hunks: hunks.0,
		new_hunks: hunks.1,
		file_moved: true,
		disposition: FileDisposition::Moved { pure: true },
	})
}

fn deleted_pair<'scan>(
	scan: &ChangeScan<'_>,
	diff: &FileDiff,
	base_rev: &str,
) -> Result<SidePair<'scan>, String> {
	let path = diff_path(diff);
	let lang = environment::language_for_path(&path).map_err(|error| error.to_string())?;
	let Some((source_root, root, old_rel)) = source_root_for_path(scan, &path) else {
		return Err(format!(
			"deleted file {} is outside the scanned source roots",
			path.display()
		));
	};
	let anchor = anchor_for(scan, root, &old_rel);
	let old_display = display_rel_path(scan, source_root, root, &old_rel);
	let base_source = git_show(&diff.repo_root, base_rev, &diff.repo_rel)
		.map_err(|error| format!("{}: cannot read base blob: {error}", old_display.display()))?;
	let base_graph = environment::extract_source_with(lang, &base_source, &anchor, root.ctx);
	let empty_graph = environment::extract_source_with(lang, "", &anchor, root.ctx);
	let (old_hunks, new_hunks) = hunk_spans(&diff.hunks);
	Ok(SidePair {
		new_rel: old_display.clone(),
		old_rel: old_display,
		lang,
		base_source,
		base_graph,
		current: CurrentRef::Empty {
			source: String::new(),
			graph: empty_graph,
		},
		old_hunks,
		new_hunks,
		file_moved: false,
		disposition: FileDisposition::Removed,
	})
}

fn extract_at(
	scan: &ChangeScan<'_>,
	file: &ChangeFile<'_>,
	anchor: &Path,
	source: &str,
) -> CodeGraph {
	let root = &scan.roots[file.source_root];
	environment::extract_source_with(file.lang, source, anchor, root.ctx)
}

fn hunk_spans(hunks: &[DiffHunk]) -> (LineSpans, LineSpans) {
	let old = hunks
		.iter()
		.filter_map(|hunk| hunk.old)
		.map(|span| (span.start, span.end))
		.collect();
	let new = hunks
		.iter()
		.filter_map(|hunk| hunk.new)
		.map(|span| (span.start, span.end))
		.collect();
	(old, new)
}

fn opaque_facts(scan: &ChangeScan<'_>, diff: &FileDiff) -> FileFacts {
	let path = diff_path(diff);
	let rel = source_root_for_path(scan, &path)
		.map(|(source_root, root, rel)| display_rel_path(scan, source_root, root, &rel))
		.unwrap_or_else(|| diff.repo_rel.clone());
	let (old_path, new_path, disposition) = match diff.status {
		FileDiffStatus::Added => (None, Some(rel), FileDisposition::Added),
		FileDiffStatus::Deleted => (Some(rel), None, FileDisposition::Removed),
		_ => (Some(rel.clone()), Some(rel), FileDisposition::Modified),
	};
	FileFacts {
		rollup: FileRollup {
			old_path,
			new_path,
			disposition,
			symbol_changes: 0,
			moved_symbols: 0,
		},
		coverage: HunkCoverage::default(),
		analyzable: false,
	}
}

fn rename_context(changes: &[SymbolChange], pairs: &[SidePair<'_>]) -> RenameContext {
	let mut ctx = RenameContext::from_changes(changes);
	for pair in pairs.iter().filter(|pair| pair.file_moved) {
		ctx.push_pair(
			pair.base_graph.root().clone(),
			pair.current_side().graph.root().clone(),
		);
	}
	ctx
}

fn pair_facts(pair: &SidePair<'_>, changes: &[SymbolChange], refs: &[RefChange]) -> FileFacts {
	let file_changes: Vec<SymbolChange> = changes
		.iter()
		.filter(|change| {
			change
				.old
				.as_ref()
				.is_some_and(|side| side.file_path == pair.old_rel)
				|| change
					.new
					.as_ref()
					.is_some_and(|side| side.file_path == pair.new_rel)
		})
		.cloned()
		.collect();
	let coverage = pair_coverage(pair, &file_changes, refs);
	let mut rollup = match pair.disposition {
		FileDisposition::Moved { .. } => {
			moved_file_rollup(pair.old_rel.clone(), pair.new_rel.clone(), &file_changes)
		}
		disposition => plain_rollup(pair, disposition, &file_changes),
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

fn plain_rollup(
	pair: &SidePair<'_>,
	disposition: FileDisposition,
	file_changes: &[SymbolChange],
) -> FileRollup {
	let keep_old = !matches!(disposition, FileDisposition::Added);
	let keep_new = !matches!(disposition, FileDisposition::Removed);
	FileRollup {
		old_path: keep_old.then(|| pair.old_rel.clone()),
		new_path: keep_new.then(|| pair.new_rel.clone()),
		disposition,
		symbol_changes: file_changes.len(),
		moved_symbols: 0,
	}
}

fn pair_coverage(
	pair: &SidePair<'_>,
	file_changes: &[SymbolChange],
	refs: &[RefChange],
) -> HunkCoverage {
	let mut old_explained: Vec<(u32, u32)> = Vec::new();
	let mut new_explained: Vec<(u32, u32)> = Vec::new();
	for change in file_changes {
		if let Some(range) = change
			.old
			.as_ref()
			.filter(|side| side.file_path == pair.old_rel)
			.and_then(|side| side.line_range)
		{
			old_explained.push(range);
		}
		if let Some(range) = change
			.new
			.as_ref()
			.filter(|side| side.file_path == pair.new_rel)
			.and_then(|side| side.line_range)
		{
			new_explained.push(range);
		}
	}
	for reference in refs {
		old_explained.extend(reference.old_line_range);
		new_explained.extend(reference.new_line_range);
	}
	hunk_coverage(CoverageInputs {
		old_hunks: &pair.old_hunks,
		new_hunks: &pair.new_hunks,
		old_explained: &old_explained,
		new_explained: &new_explained,
	})
}

#[cfg(test)]
mod tests {
	use super::super::super::diff::ChangeRoot;
	use super::super::model::{RefChangeKind, SemanticKind};
	use super::*;
	use crate::environment::ExtractContext;
	use std::process::Command;

	fn write(root: &Path, rel: &str, body: &str) {
		let path = root.join(rel);
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(path, body).unwrap();
	}

	fn git(root: &Path, args: &[&str]) {
		let output = Command::new("git")
			.arg("-C")
			.arg(root)
			.args(args)
			.output()
			.unwrap_or_else(|e| panic!("cannot run git {args:?}: {e}"));
		assert!(
			output.status.success(),
			"git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
	}

	struct ScanFixture {
		root: PathBuf,
		files: Vec<(PathBuf, String, String)>,
		graphs: Vec<CodeGraph>,
	}

	impl ScanFixture {
		fn new(root: &Path, rels: &[&str]) -> Self {
			let files: Vec<(PathBuf, String, String)> = rels
				.iter()
				.map(|rel| {
					let path = root.join(rel);
					let source = std::fs::read_to_string(&path).unwrap();
					(path, rel.to_string(), source)
				})
				.collect();
			let graphs = files
				.iter()
				.map(|(_, rel, source)| {
					environment::extract_source(Lang::Rs, source, Path::new(rel))
				})
				.collect();
			Self {
				root: root.to_path_buf(),
				files,
				graphs,
			}
		}

		fn scan<'a>(&'a self, ctx: &'a ExtractContext) -> ChangeScan<'a> {
			ChangeScan {
				roots: vec![ChangeRoot {
					label: "repo",
					path: &self.root,
					ctx,
				}],
				files: self
					.files
					.iter()
					.zip(&self.graphs)
					.enumerate()
					.map(|(file_idx, ((path, rel, source), graph))| ChangeFile {
						file_idx,
						source_root: 0,
						path,
						rel_path: Path::new(rel),
						anchor: Path::new(rel),
						lang: Lang::Rs,
						graph,
						source,
					})
					.collect(),
			}
		}
	}

	#[test]
	fn review_reports_move_edit_retarget_and_opaque_facts_together() {
		let tmp = tempfile::tempdir().unwrap();
		git(tmp.path(), &["init"]);
		git(tmp.path(), &["config", "user.email", "cm@example.test"]);
		git(tmp.path(), &["config", "user.name", "Code Moniker"]);
		write(tmp.path(), "Cargo.toml", "[package]\nname = \"demo\"\n");
		write(tmp.path(), "src/lib.rs", "mod util;\nmod consumer;\n");
		write(
			tmp.path(),
			"src/util.rs",
			"pub fn assist() { work(); }\npub fn sidekick() { rest(); }\n",
		);
		write(
			tmp.path(),
			"src/consumer.rs",
			"use crate::util::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 1 }\n",
		);
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		git(tmp.path(), &["mv", "src/util.rs", "src/support.rs"]);
		write(
			tmp.path(),
			"Cargo.toml",
			"[package]\nname = \"demo\"\nedition = \"2024\"\n",
		);
		write(tmp.path(), "src/lib.rs", "mod support;\nmod consumer;\n");
		write(
			tmp.path(),
			"src/consumer.rs",
			"use crate::support::assist;\n\npub fn caller() { assist(); }\npub fn edited() -> u32 { 2 }\n",
		);
		let fixture = ScanFixture::new(
			tmp.path(),
			&["src/lib.rs", "src/consumer.rs", "src/support.rs"],
		);
		let ctx = ExtractContext::default();

		let review = build_semantic_review(&fixture.scan(&ctx));

		assert!(review.diagnostics.is_empty(), "{:?}", review.diagnostics);
		let moved = review
			.files
			.iter()
			.find(|facts| facts.rollup.old_path.as_deref() == Some(Path::new("src/util.rs")))
			.expect("moved file facts");
		assert_eq!(
			moved.rollup.new_path.as_deref(),
			Some(Path::new("src/support.rs"))
		);
		assert_eq!(
			moved.rollup.disposition,
			FileDisposition::Moved { pure: true },
			"{moved:?}"
		);
		let opaque = review
			.files
			.iter()
			.find(|facts| facts.rollup.new_path.as_deref() == Some(Path::new("Cargo.toml")))
			.expect("manifest facts");
		assert!(!opaque.analyzable);
		let kinds: Vec<SemanticKind> = review
			.symbol_changes
			.iter()
			.map(|change| change.kind)
			.collect();
		assert!(kinds.contains(&SemanticKind::BodyModified), "{kinds:?}");
		assert!(kinds.contains(&SemanticKind::Moved), "{kinds:?}");
		assert!(
			!kinds.contains(&SemanticKind::Added) && !kinds.contains(&SemanticKind::Removed),
			"everything must pair: {:?}",
			review.symbol_changes
		);
		assert!(
			review
				.ref_changes
				.iter()
				.any(|change| change.kind == RefChangeKind::ImportRetargeted),
			"{:?}",
			review.ref_changes
		);
		let consumer = review
			.files
			.iter()
			.find(|facts| facts.rollup.new_path.as_deref() == Some(Path::new("src/consumer.rs")))
			.expect("consumer facts");
		assert!(
			consumer.coverage.explained(),
			"import retarget and body edit must explain every hunk: {consumer:?}"
		);
	}

	#[test]
	fn scoped_review_classifies_a_committed_rename_between_revisions() {
		let tmp = tempfile::tempdir().unwrap();
		git(tmp.path(), &["init"]);
		git(tmp.path(), &["config", "user.email", "cm@example.test"]);
		git(tmp.path(), &["config", "user.name", "Code Moniker"]);
		write(
			tmp.path(),
			"src/util.rs",
			"pub fn assist() { work(); }\npub fn sidekick() { rest(); }\n",
		);
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		git(tmp.path(), &["mv", "src/util.rs", "src/support.rs"]);
		git(tmp.path(), &["commit", "-am", "move"]);
		let fixture = ScanFixture::new(tmp.path(), &["src/support.rs"]);
		let ctx = ExtractContext::default();
		let roots = vec![("repo".to_string(), tmp.path().to_path_buf())];

		for range in ["HEAD~1..HEAD", "HEAD~1...HEAD"] {
			let scope = DiffScope::parse_range(range).unwrap();
			let diffs = collect_review_diffs_scoped(&roots, &scope);
			assert!(diffs.any_root_resolved(), "{:?}", diffs.diagnostics);
			let review = build_semantic_review_from(&fixture.scan(&ctx), &diffs);

			assert!(review.scope.contains(".."), "{}", review.scope);
			let moved = review
				.files
				.iter()
				.find(|facts| facts.rollup.old_path.as_deref() == Some(Path::new("src/util.rs")))
				.unwrap_or_else(|| panic!("moved facts for {range}: {:?}", review.files));
			assert_eq!(
				moved.rollup.disposition,
				FileDisposition::Moved { pure: true },
				"{range}: {moved:?}"
			);
		}
	}

	#[test]
	fn scoped_review_reports_unresolvable_revisions() {
		let tmp = tempfile::tempdir().unwrap();
		git(tmp.path(), &["init"]);
		git(tmp.path(), &["config", "user.email", "cm@example.test"]);
		git(tmp.path(), &["config", "user.name", "Code Moniker"]);
		write(tmp.path(), "src/lib.rs", "fn lone() {}\n");
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		let roots = vec![("repo".to_string(), tmp.path().to_path_buf())];

		let scope = DiffScope::parse_range("no-such-rev..HEAD").unwrap();
		let diffs = collect_review_diffs_scoped(&roots, &scope);

		assert!(!diffs.any_root_resolved());
		assert!(
			diffs
				.diagnostics
				.iter()
				.any(|message| message.contains("no-such-rev")),
			"{:?}",
			diffs.diagnostics
		);
	}

	#[test]
	fn review_flags_unattributed_edits_as_residual() {
		let tmp = tempfile::tempdir().unwrap();
		git(tmp.path(), &["init"]);
		git(tmp.path(), &["config", "user.email", "cm@example.test"]);
		git(tmp.path(), &["config", "user.name", "Code Moniker"]);
		write(
			tmp.path(),
			"src/lib.rs",
			"fn steady() { work(); }\n// note\n",
		);
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		write(
			tmp.path(),
			"src/lib.rs",
			"fn steady() { work(); }\n// reworded note\n",
		);
		let fixture = ScanFixture::new(tmp.path(), &["src/lib.rs"]);
		let ctx = ExtractContext::default();

		let review = build_semantic_review(&fixture.scan(&ctx));

		let facts = review.files.first().expect("file facts");
		assert!(
			!facts.coverage.explained(),
			"a comment-only edit has no symbolic fact and must stay residual: {facts:?}"
		);
		assert!(
			review.symbol_changes.is_empty(),
			"{:?}",
			review.symbol_changes
		);
	}
}
