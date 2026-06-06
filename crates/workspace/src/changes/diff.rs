use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::code::{def_kind, is_navigable_def, last_name};
use crate::environment::{self, ExtractContext};
use crate::gitignore::GitignoreStack;
use crate::snapshot::SymbolLocation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeStatus {
	Added,
	Modified,
	Removed,
}

impl ChangeStatus {
	pub fn label(self) -> &'static str {
		match self {
			Self::Added => "added",
			Self::Modified => "modified",
			Self::Removed => "removed",
		}
	}

	pub fn marker(self) -> &'static str {
		match self {
			Self::Added => "+",
			Self::Modified => "~",
			Self::Removed => "-",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitResourceStatus {
	pub label: String,
	pub git_root: Option<PathBuf>,
	pub message: String,
}

impl GitResourceStatus {
	pub fn available(&self) -> bool {
		self.git_root.is_some()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeEntry {
	pub loc: Option<SymbolLocation>,
	pub status: ChangeStatus,
	pub lang: Lang,
	pub file_path: PathBuf,
	pub kind: String,
	pub name: String,
	pub moniker: Moniker,
	pub hunk_count: usize,
	pub line_range: Option<(u32, u32)>,
}

pub struct ChangeRoot<'a> {
	pub label: &'a str,
	pub path: &'a Path,
	pub ctx: &'a ExtractContext,
}

pub struct ChangeFile<'a> {
	pub file_idx: usize,
	pub source_root: usize,
	pub path: &'a Path,
	pub rel_path: &'a Path,
	pub anchor: &'a Path,
	pub lang: Lang,
	pub graph: &'a CodeGraph,
	pub source: &'a str,
}

pub struct ChangeScan<'a> {
	pub roots: Vec<ChangeRoot<'a>>,
	pub files: Vec<ChangeFile<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeIndex {
	pub scope: String,
	pub entries: Vec<ChangeEntry>,
	pub resources: Vec<GitResourceStatus>,
	pub diagnostics: Vec<String>,
	entries_by_symbol: FxHashMap<SymbolLocation, usize>,
	count_by_file: FxHashMap<usize, usize>,
}

impl Default for ChangeIndex {
	fn default() -> Self {
		Self {
			scope: "HEAD..worktree".to_string(),
			entries: Vec::new(),
			resources: Vec::new(),
			diagnostics: Vec::new(),
			entries_by_symbol: FxHashMap::default(),
			count_by_file: FxHashMap::default(),
		}
	}
}

impl ChangeIndex {
	pub fn entry_for(&self, loc: &SymbolLocation) -> Option<&ChangeEntry> {
		self.entries_by_symbol
			.get(loc)
			.and_then(|idx| self.entries.get(*idx))
	}

	pub fn changed_symbols(&self) -> Vec<SymbolLocation> {
		self.entries.iter().filter_map(|entry| entry.loc).collect()
	}

	pub fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.count_by_file.get(&file_idx).copied().unwrap_or(0)
	}

	pub fn changed_file_count(&self) -> usize {
		self.entries
			.iter()
			.map(|entry| entry.file_path.clone())
			.collect::<HashSet<_>>()
			.len()
	}

	fn rebuild_lookups(&mut self) {
		self.entries_by_symbol.clear();
		self.count_by_file.clear();
		for (idx, entry) in self.entries.iter().enumerate() {
			let Some(loc) = entry.loc else {
				continue;
			};
			self.entries_by_symbol.insert(loc, idx);
			*self.count_by_file.entry(loc.file).or_default() += 1;
		}
	}
}

#[derive(Clone, Debug)]
struct FileDiff {
	repo_root: PathBuf,
	repo_rel: PathBuf,
	status: FileDiffStatus,
	hunks: Vec<DiffHunk>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileDiffStatus {
	Tracked,
	Added,
	Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiffHunk {
	old: Option<LineSpan>,
	new: Option<LineSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LineSpan {
	start: u32,
	end: u32,
}

impl LineSpan {
	fn intersects(self, other: Self) -> bool {
		self.start <= other.end && other.start <= self.end
	}
}

pub fn build_change_index(scan: ChangeScan<'_>) -> ChangeIndex {
	let mut changes = ChangeIndex::default();
	let mut diffs = Vec::new();
	for root in &scan.roots {
		match GitWorktree::discover(root.path) {
			Ok(repo) => {
				let git_root = repo.root().to_path_buf();
				changes.resources.push(GitResourceStatus {
					label: root.label.to_string(),
					git_root: Some(git_root.clone()),
					message: format!("git root {}", git_root.display()),
				});
				match collect_changed_files(&git_root, root.path) {
					Ok(mut root_diffs) => diffs.append(&mut root_diffs),
					Err(error) => changes.diagnostics.push(format!(
						"{}: cannot inspect git changes: {error}",
						root.label
					)),
				}
			}
			Err(message) => {
				changes.resources.push(GitResourceStatus {
					label: root.label.to_string(),
					git_root: None,
					message: message.clone(),
				});
				changes.diagnostics.push(message);
			}
		}
	}
	let mut entries = Vec::new();
	let relevant_diffs = scan.relevant_diffs(&diffs);
	for file in &scan.files {
		let Some(diff) = relevant_diffs.for_file(file.path) else {
			continue;
		};
		entries.extend(changed_entries_for_file(&scan, file, diff));
	}
	for diff in relevant_diffs.deleted {
		match removed_entries_for_deleted_file(&scan, diff) {
			Ok(mut removed) => entries.append(&mut removed),
			Err(error) => changes.diagnostics.push(error.to_string()),
		}
	}
	entries.sort_by(|a, b| {
		a.file_path
			.cmp(&b.file_path)
			.then_with(|| a.moniker.cmp(&b.moniker))
	});
	entries.dedup_by_key(|entry| entry.moniker.clone());
	changes.entries = entries;
	changes.rebuild_lookups();
	changes
}

struct RelevantDiffs<'a> {
	by_path: FxHashMap<PathBuf, &'a FileDiff>,
	deleted: Vec<&'a FileDiff>,
}

struct SourceVisibility {
	current_paths: FxHashSet<PathBuf>,
	deleted_roots: Vec<DeletedSourceRoot>,
}

struct DeletedSourceRoot {
	path: PathBuf,
	gitignore: GitignoreStack,
}

impl<'scan> ChangeScan<'scan> {
	fn relevant_diffs<'diff>(&self, diffs: &'diff [FileDiff]) -> RelevantDiffs<'diff> {
		let visibility = self.source_visibility();
		let mut by_path = FxHashMap::default();
		let mut deleted = Vec::new();
		for diff in diffs {
			let path = normalize_path(&diff_path(diff));
			match diff.status {
				FileDiffStatus::Deleted => {
					if visibility.accepts_deleted_path(&path) {
						deleted.push(diff);
					}
				}
				FileDiffStatus::Tracked | FileDiffStatus::Added => {
					if visibility.current_paths.contains(&path) {
						by_path.insert(path, diff);
					}
				}
			}
		}
		RelevantDiffs { by_path, deleted }
	}

	fn source_visibility(&self) -> SourceVisibility {
		SourceVisibility {
			current_paths: self
				.files
				.iter()
				.map(|file| normalize_path(file.path))
				.collect(),
			deleted_roots: self
				.roots
				.iter()
				.map(|root| DeletedSourceRoot::new(root.path))
				.collect(),
		}
	}
}

impl SourceVisibility {
	fn accepts_deleted_path(&self, path: &Path) -> bool {
		environment::language_for_path(path).is_ok()
			&& self.deleted_roots.iter().any(|root| root.accepts(path))
	}
}

impl DeletedSourceRoot {
	fn new(path: &Path) -> Self {
		let path = normalize_path(path);
		Self {
			gitignore: GitignoreStack::for_root(&path),
			path,
		}
	}

	fn accepts(&self, path: &Path) -> bool {
		let Ok(rel) = path.strip_prefix(&self.path) else {
			return false;
		};
		!has_hidden_component(rel) && !self.gitignore.is_ignored(path, false)
	}
}

impl<'a> RelevantDiffs<'a> {
	fn for_file(&self, path: &Path) -> Option<&'a FileDiff> {
		self.by_path.get(&normalize_path(path)).copied()
	}
}

fn has_hidden_component(path: &Path) -> bool {
	path.components().any(|component| {
		let name = component.as_os_str();
		name != OsStr::new(".")
			&& name != OsStr::new("..")
			&& name.as_encoded_bytes().first() == Some(&b'.')
	})
}

fn changed_entries_for_file(
	scan: &ChangeScan<'_>,
	file: &ChangeFile<'_>,
	diff: &FileDiff,
) -> Vec<ChangeEntry> {
	let base = if diff.status == FileDiffStatus::Added {
		BaseFile::default()
	} else {
		base_file(scan, file, diff).unwrap_or_default()
	};
	let base_monikers: HashSet<_> = base.defs.iter().map(|def| def.moniker.clone()).collect();
	let current_monikers: HashSet<_> = file
		.graph
		.defs()
		.filter(|def| is_navigable_def(file.lang, def))
		.map(|def| def.moniker.clone())
		.collect();
	let candidates: Vec<_> = file
		.graph
		.defs()
		.enumerate()
		.filter_map(|(def_idx, def)| {
			if !is_navigable_def(file.lang, def) {
				return None;
			}
			let status = if base_monikers.contains(&def.moniker) {
				ChangeStatus::Modified
			} else {
				ChangeStatus::Added
			};
			if status == ChangeStatus::Modified && !def_intersects_hunks(def, file.source, diff) {
				return None;
			}
			Some(SymbolLocation {
				file: file.file_idx,
				symbol: def_idx,
			})
		})
		.collect();
	let keep_ancestors = diff.status == FileDiffStatus::Added;
	let mut entries: Vec<_> = candidates
		.iter()
		.copied()
		.filter(|loc| {
			keep_ancestors
				|| !candidates.iter().any(|candidate| {
					candidate != loc && is_descendant(file.graph, loc.symbol, candidate.symbol)
				})
		})
		.map(|loc| {
			let def = file.graph.def_at(loc.symbol);
			let status = if base_monikers.contains(&def.moniker) {
				ChangeStatus::Modified
			} else {
				ChangeStatus::Added
			};
			ChangeEntry {
				loc: Some(loc),
				status,
				lang: file.lang,
				file_path: file.rel_path.to_path_buf(),
				kind: def_kind(def),
				name: last_name(&def.moniker),
				moniker: def.moniker.clone(),
				hunk_count: diff.hunks.len(),
				line_range: def
					.position
					.map(|(start, end)| environment::line_range(file.source, start, end)),
			}
		})
		.collect();
	entries.extend(
		base.defs
			.iter()
			.filter(|def| !current_monikers.contains(&def.moniker))
			.filter(|def| old_span_intersects_hunks(def.line_range, diff))
			.map(|def| ChangeEntry {
				loc: None,
				status: ChangeStatus::Removed,
				lang: file.lang,
				file_path: file.rel_path.to_path_buf(),
				kind: def.kind.clone(),
				name: def.name.clone(),
				moniker: def.moniker.clone(),
				hunk_count: diff.hunks.len(),
				line_range: Some(def.line_range),
			}),
	);
	entries
}

fn def_intersects_hunks(def: &DefRecord, source: &str, diff: &FileDiff) -> bool {
	let Some((start, end)) = def.position else {
		return false;
	};
	let (start_line, end_line) = environment::line_range(source, start, end);
	let def_span = LineSpan {
		start: start_line,
		end: end_line,
	};
	diff.hunks
		.iter()
		.filter_map(|hunk| hunk.new)
		.any(|hunk| def_span.intersects(hunk))
}

fn old_span_intersects_hunks(line_range: (u32, u32), diff: &FileDiff) -> bool {
	let def_span = LineSpan {
		start: line_range.0,
		end: line_range.1,
	};
	diff.hunks
		.iter()
		.filter_map(|hunk| hunk.old)
		.any(|hunk| def_span.intersects(hunk))
}

fn is_descendant(graph: &CodeGraph, ancestor: usize, mut child: usize) -> bool {
	while let Some(parent) = graph.def_at(child).parent {
		if parent == ancestor {
			return true;
		}
		child = parent;
	}
	false
}

#[derive(Clone, Debug, Default)]
struct BaseFile {
	defs: Vec<BaseDef>,
}

#[derive(Clone, Debug)]
struct BaseDef {
	moniker: Moniker,
	kind: String,
	name: String,
	line_range: (u32, u32),
}

fn base_file(
	scan: &ChangeScan<'_>,
	file: &ChangeFile<'_>,
	diff: &FileDiff,
) -> anyhow::Result<BaseFile> {
	let source = git_show(&diff.repo_root, &diff.repo_rel)?;
	let root = &scan.roots[file.source_root];
	let graph = environment::extract_source_with(file.lang, &source, file.anchor, root.ctx);
	Ok(BaseFile {
		defs: graph
			.defs()
			.filter(|def| is_navigable_def(file.lang, def))
			.filter_map(|def| {
				let (start, end) = def.position?;
				Some(BaseDef {
					moniker: def.moniker.clone(),
					kind: def_kind(def),
					name: last_name(&def.moniker),
					line_range: environment::line_range(&source, start, end),
				})
			})
			.collect(),
	})
}

fn removed_entries_for_deleted_file(
	scan: &ChangeScan<'_>,
	diff: &FileDiff,
) -> anyhow::Result<Vec<ChangeEntry>> {
	let source = git_show(&diff.repo_root, &diff.repo_rel)?;
	let path = diff_path(diff);
	let Some((source_root, root, rel_path)) = source_root_for_path(scan, &path) else {
		return Ok(Vec::new());
	};
	let lang = environment::language_for_path(&path)?;
	let anchor = anchor_for(scan, root, &rel_path);
	let graph = environment::extract_source_with(lang, &source, &anchor, root.ctx);
	let mut entries = Vec::new();
	for def in graph.defs().filter(|def| is_navigable_def(lang, def)) {
		let Some((start, end)) = def.position else {
			continue;
		};
		let range = environment::line_range(&source, start, end);
		entries.push(ChangeEntry {
			loc: None,
			status: ChangeStatus::Removed,
			lang,
			file_path: display_rel_path(scan, source_root, root, &rel_path),
			kind: def_kind(def),
			name: last_name(&def.moniker),
			moniker: def.moniker.clone(),
			hunk_count: diff.hunks.len(),
			line_range: Some(range),
		});
	}
	Ok(entries)
}

fn source_root_for_path<'a>(
	scan: &'a ChangeScan<'_>,
	path: &Path,
) -> Option<(usize, &'a ChangeRoot<'a>, PathBuf)> {
	let path = normalize_path(path);
	scan.roots.iter().enumerate().find_map(|(idx, root)| {
		let root_path = normalize_path(root.path);
		path.strip_prefix(&root_path)
			.ok()
			.map(|rel| (idx, root, rel.to_path_buf()))
	})
}

fn anchor_for(scan: &ChangeScan<'_>, root: &ChangeRoot<'_>, rel_path: &Path) -> PathBuf {
	if scan.roots.len() > 1 {
		PathBuf::from(root.label).join(rel_path)
	} else {
		rel_path.to_path_buf()
	}
}

fn display_rel_path(
	scan: &ChangeScan<'_>,
	source_root: usize,
	root: &ChangeRoot<'_>,
	rel_path: &Path,
) -> PathBuf {
	if scan.roots.len() > 1 {
		PathBuf::from(root.label).join(rel_path)
	} else {
		let _ = source_root;
		rel_path.to_path_buf()
	}
}

fn collect_changed_files(git_root: &Path, source_root: &Path) -> Result<Vec<FileDiff>, String> {
	let pathspec = git_pathspec(git_root, source_root);
	let mut out = Vec::new();
	for row in git_cli_lines(
		git_root,
		&[
			"diff",
			"--name-status",
			"--diff-filter=ACMRD",
			"HEAD",
			"--",
			&pathspec,
		],
	)? {
		let (status, repo_rel) = parse_name_status(&row)?;
		let diff = git_cli_text(
			git_root,
			&["diff", "--unified=0", "HEAD", "--", &path_to_git(&repo_rel)],
		)?;
		out.push(FileDiff {
			repo_root: git_root.to_path_buf(),
			repo_rel,
			status,
			hunks: parse_diff_hunks(&diff),
		});
	}
	for rel in git_cli_lines(
		git_root,
		&[
			"ls-files",
			"--others",
			"--exclude-standard",
			"--",
			&pathspec,
		],
	)? {
		out.push(FileDiff {
			repo_root: git_root.to_path_buf(),
			repo_rel: PathBuf::from(rel),
			status: FileDiffStatus::Added,
			hunks: Vec::new(),
		});
	}
	Ok(out)
}

fn parse_name_status(row: &str) -> Result<(FileDiffStatus, PathBuf), String> {
	let parts: Vec<&str> = row.split('\t').collect();
	let Some(status) = parts.first().and_then(|part| part.chars().next()) else {
		return Err(format!("cannot parse git name-status row {row:?}"));
	};
	let path = match status {
		'R' => parts.get(2).copied(),
		_ => parts.get(1).copied(),
	}
	.ok_or_else(|| format!("cannot parse git name-status row {row:?}"))?;
	let status = match status {
		'A' => FileDiffStatus::Added,
		'D' => FileDiffStatus::Deleted,
		'M' | 'C' | 'R' => FileDiffStatus::Tracked,
		_ => FileDiffStatus::Tracked,
	};
	Ok((status, PathBuf::from(path)))
}

struct GitWorktree {
	root: PathBuf,
}

impl GitWorktree {
	fn discover(path: &Path) -> Result<Self, String> {
		let output = git_cli_command(path)
			.args(["rev-parse", "--show-toplevel"])
			.output()
			.map_err(|e| format!("cannot run git rev-parse in {}: {e}", path.display()))?;
		if !output.status.success() {
			return Err(format!("{} is not inside a Git repository", path.display()));
		}
		let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
		if root.is_empty() {
			return Err(format!("{} is not inside a Git worktree", path.display()));
		}
		Ok(Self {
			root: normalize_path(Path::new(&root)),
		})
	}

	fn root(&self) -> &Path {
		&self.root
	}
}

fn git_cli_lines(git_root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
	Ok(git_cli_text(git_root, args)?
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(ToOwned::to_owned)
		.collect())
}

fn git_show(git_root: &Path, repo_rel: &Path) -> anyhow::Result<String> {
	git_cli_text(
		git_root,
		&["show", &format!("HEAD:{}", path_to_git(repo_rel))],
	)
	.map_err(anyhow::Error::msg)
}

fn git_cli_text(git_root: &Path, args: &[&str]) -> Result<String, String> {
	let output = git_cli_command(git_root)
		.args(args)
		.output()
		.map_err(|e| format!("cannot run git {:?}: {e}", args))?;
	if !output.status.success() {
		return Err(format!(
			"git {:?} failed: {}",
			args,
			String::from_utf8_lossy(&output.stderr).trim()
		));
	}
	Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_cli_command(cwd: &Path) -> Command {
	let mut command = Command::new("git");
	command.env("GIT_OPTIONAL_LOCKS", "0").arg("-C").arg(cwd);
	command
}

fn git_pathspec(git_root: &Path, source_root: &Path) -> String {
	let root = normalize_path(git_root);
	let source = normalize_path(source_root);
	let rel = source.strip_prefix(&root).unwrap_or(source.as_path());
	if rel.as_os_str().is_empty() {
		".".to_string()
	} else {
		path_to_git(rel)
	}
}

fn diff_path(diff: &FileDiff) -> PathBuf {
	diff.repo_root.join(&diff.repo_rel)
}

fn normalize_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn path_to_git(path: &Path) -> String {
	path.components()
		.filter_map(|component| component.as_os_str().to_str())
		.collect::<Vec<_>>()
		.join("/")
}

fn parse_diff_hunks(diff: &str) -> Vec<DiffHunk> {
	diff.lines()
		.filter_map(|line| line.strip_prefix("@@ "))
		.filter_map(parse_hunk_header)
		.collect()
}

fn parse_hunk_header(header: &str) -> Option<DiffHunk> {
	let mut parts = header.split_whitespace();
	let old = parse_hunk_side(parts.next()?)?;
	let new = parse_hunk_side(parts.next()?)?;
	Some(DiffHunk { old, new })
}

fn parse_hunk_side(raw: &str) -> Option<Option<LineSpan>> {
	let raw = raw.strip_prefix(['-', '+'])?;
	let (start, count) = raw
		.split_once(',')
		.map(|(start, count)| Some((start.parse::<u32>().ok()?, count.parse::<u32>().ok()?)))
		.unwrap_or_else(|| Some((raw.parse::<u32>().ok()?, 1)))?;
	if count == 0 {
		return Some(None);
	}
	Some(Some(LineSpan {
		start,
		end: start + count - 1,
	}))
}

#[cfg(test)]
mod tests {
	use super::*;

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

	fn committed_repo() -> tempfile::TempDir {
		let tmp = tempfile::tempdir().unwrap();
		init_git(tmp.path());
		write(tmp.path(), "src/Foo.java", "class Foo {}\n");
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		tmp
	}

	fn init_git(root: &Path) {
		git(root, &["init"]);
		git(root, &["config", "user.email", "code-moniker@example.test"]);
		git(root, &["config", "user.name", "Code Moniker"]);
	}

	fn rust_file<'a>(
		file_idx: usize,
		source_root: usize,
		path: &'a Path,
		rel: &'a str,
		source: &'a str,
		graph: &'a CodeGraph,
	) -> ChangeFile<'a> {
		ChangeFile {
			file_idx,
			source_root,
			path,
			rel_path: Path::new(rel),
			anchor: Path::new(rel),
			lang: Lang::Rs,
			graph,
			source,
		}
	}

	fn rust_scan<'a>(
		root: &'a Path,
		ctx: &'a ExtractContext,
		path: &'a Path,
		rel: &'a str,
		source: &'a str,
		graph: &'a CodeGraph,
	) -> ChangeScan<'a> {
		ChangeScan {
			roots: vec![ChangeRoot {
				label: "repo",
				path: root,
				ctx,
			}],
			files: vec![rust_file(0, 0, path, rel, source, graph)],
		}
	}

	#[test]
	fn git_discovers_worktree_root() {
		let tmp = committed_repo();
		let nested = tmp.path().join("src");

		let repo = GitWorktree::discover(&nested).unwrap();

		assert_eq!(repo.root(), normalize_path(tmp.path()).as_path());
	}

	#[test]
	fn git_reads_blob_from_head_not_worktree() {
		let tmp = committed_repo();
		write(tmp.path(), "src/Foo.java", "class Foo { int changed; }\n");

		let text = git_show(tmp.path(), Path::new("src/Foo.java")).unwrap();

		assert_eq!(text, "class Foo {}\n");
	}

	#[test]
	fn build_change_index_reports_modified_symbols() {
		let tmp = tempfile::tempdir().unwrap();
		init_git(tmp.path());
		write(tmp.path(), "src/lib.rs", "fn kept() {}\nfn changed() {}\n");
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		let source = "fn kept() {}\nfn changed() { kept(); }\n";
		write(tmp.path(), "src/lib.rs", source);
		let path = tmp.path().join("src/lib.rs");
		let ctx = ExtractContext::default();
		let graph = environment::extract_source(Lang::Rs, source, Path::new("src/lib.rs"));

		let index = build_change_index(rust_scan(
			tmp.path(),
			&ctx,
			&path,
			"src/lib.rs",
			source,
			&graph,
		));

		assert!(index.entries.iter().any(
			|entry| entry.status == ChangeStatus::Modified && entry.name.starts_with("changed")
		));
	}

	#[test]
	fn build_change_index_reports_untracked_source_files_as_added() {
		let tmp = committed_repo();
		let source = "fn added() {}\n";
		write(tmp.path(), "src/new.rs", source);
		let path = tmp.path().join("src/new.rs");
		let ctx = ExtractContext::default();
		let graph = environment::extract_source(Lang::Rs, source, Path::new("src/new.rs"));

		let index = build_change_index(rust_scan(
			tmp.path(),
			&ctx,
			&path,
			"src/new.rs",
			source,
			&graph,
		));

		assert!(
			index
				.entries
				.iter()
				.any(|entry| entry.status == ChangeStatus::Added && entry.name.starts_with("added"))
		);
	}

	#[test]
	fn build_change_index_ignores_untracked_sources_absent_from_catalog() {
		let tmp = committed_repo();
		write(tmp.path(), ".code-moniker/generated.rs", "fn cached() {}\n");
		let ctx = ExtractContext::default();
		let scan = ChangeScan {
			roots: vec![ChangeRoot {
				label: "repo",
				path: tmp.path(),
				ctx: &ctx,
			}],
			files: Vec::new(),
		};

		let index = build_change_index(scan);

		assert!(
			index.entries.is_empty(),
			"unexpected changes: {:?}",
			index.entries
		);
	}

	#[test]
	fn build_change_index_limits_diffs_to_the_changed_source_root() {
		let tmp = tempfile::tempdir().unwrap();
		init_git(tmp.path());
		write(tmp.path(), "a/src/lib.rs", "fn changed() {}\n");
		write(tmp.path(), "b/src/lib.rs", "fn unchanged() {}\n");
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		let source_a = "fn changed() { changed(); }\n";
		let source_b = "fn unchanged() {}\n";
		write(tmp.path(), "a/src/lib.rs", source_a);
		let path_a = tmp.path().join("a/src/lib.rs");
		let path_b = tmp.path().join("b/src/lib.rs");
		let root_a = tmp.path().join("a");
		let root_b = tmp.path().join("b");
		let ctx = ExtractContext::default();
		let graph_a = environment::extract_source(Lang::Rs, source_a, Path::new("src/lib.rs"));
		let graph_b = environment::extract_source(Lang::Rs, source_b, Path::new("src/lib.rs"));
		let scan = ChangeScan {
			roots: vec![
				ChangeRoot {
					label: "a",
					path: &root_a,
					ctx: &ctx,
				},
				ChangeRoot {
					label: "b",
					path: &root_b,
					ctx: &ctx,
				},
			],
			files: vec![
				rust_file(0, 0, &path_a, "src/lib.rs", source_a, &graph_a),
				rust_file(1, 1, &path_b, "src/lib.rs", source_b, &graph_b),
			],
		};

		let index = build_change_index(scan);

		assert!(index.entries.iter().any(
			|entry| entry.status == ChangeStatus::Modified && entry.name.starts_with("changed")
		));
		assert!(
			index
				.entries
				.iter()
				.all(|entry| !entry.name.starts_with("unchanged")),
			"unexpected changes: {:?}",
			index.entries
		);
	}

	#[test]
	fn build_change_index_reports_removed_symbols_from_head() {
		let tmp = tempfile::tempdir().unwrap();
		init_git(tmp.path());
		write(tmp.path(), "src/lib.rs", "fn removed() {}\n");
		git(tmp.path(), &["add", "."]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		std::fs::remove_file(tmp.path().join("src/lib.rs")).expect("remove source");
		let ctx = ExtractContext::default();
		let scan = ChangeScan {
			roots: vec![ChangeRoot {
				label: "repo",
				path: tmp.path(),
				ctx: &ctx,
			}],
			files: Vec::new(),
		};

		let index = build_change_index(scan);

		assert!(index.entries.iter().any(
			|entry| entry.status == ChangeStatus::Removed && entry.name.starts_with("removed")
		));
	}

	#[test]
	fn build_change_index_ignores_removed_sources_excluded_from_catalog() {
		let tmp = tempfile::tempdir().unwrap();
		init_git(tmp.path());
		write(tmp.path(), ".gitignore", "target/\n");
		write(tmp.path(), "target/generated.rs", "fn generated() {}\n");
		git(tmp.path(), &["add", ".gitignore"]);
		git(tmp.path(), &["add", "-f", "target/generated.rs"]);
		git(tmp.path(), &["commit", "-m", "initial"]);
		std::fs::remove_file(tmp.path().join("target/generated.rs")).expect("remove source");
		let ctx = ExtractContext::default();
		let scan = ChangeScan {
			roots: vec![ChangeRoot {
				label: "repo",
				path: tmp.path(),
				ctx: &ctx,
			}],
			files: Vec::new(),
		};

		let index = build_change_index(scan);

		assert!(
			index.entries.is_empty(),
			"unexpected changes: {:?}",
			index.entries
		);
	}
}
