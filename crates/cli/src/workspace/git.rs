use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord};
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;
use rustc_hash::FxHashMap;

use crate::extract;
use crate::lang::path_to_lang;
use crate::lines::line_range;
use crate::workspace::index::DefLocation;
use crate::workspace::symbols::{def_kind, is_navigable_def, last_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeStatus {
	Added,
	Modified,
	Removed,
}

impl ChangeStatus {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Added => "added",
			Self::Modified => "modified",
			Self::Removed => "removed",
		}
	}

	pub(crate) fn marker(self) -> &'static str {
		match self {
			Self::Added => "+",
			Self::Modified => "~",
			Self::Removed => "-",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitResourceStatus {
	pub(crate) label: String,
	pub(crate) git_root: Option<PathBuf>,
	pub(crate) message: String,
}

impl GitResourceStatus {
	pub(crate) fn available(&self) -> bool {
		self.git_root.is_some()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeEntry {
	pub(crate) loc: Option<DefLocation>,
	pub(crate) status: ChangeStatus,
	pub(crate) lang: Lang,
	pub(crate) file_path: PathBuf,
	pub(crate) kind: String,
	pub(crate) name: String,
	pub(crate) moniker: Moniker,
	pub(crate) hunk_count: usize,
	pub(crate) line_range: Option<(u32, u32)>,
}

pub(crate) struct ChangeRoot<'a> {
	pub(crate) label: &'a str,
	pub(crate) path: &'a Path,
	pub(crate) ctx: &'a extract::Context,
}

pub(crate) struct ChangeFile<'a> {
	pub(crate) file_idx: usize,
	pub(crate) source_root: usize,
	pub(crate) path: &'a Path,
	pub(crate) rel_path: &'a Path,
	pub(crate) anchor: &'a Path,
	pub(crate) lang: Lang,
	pub(crate) graph: &'a CodeGraph,
	pub(crate) source: &'a str,
}

pub(crate) struct ChangeScan<'a> {
	pub(crate) roots: Vec<ChangeRoot<'a>>,
	pub(crate) files: Vec<ChangeFile<'a>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeIndex {
	pub(crate) scope: String,
	pub(crate) entries: Vec<ChangeEntry>,
	pub(crate) resources: Vec<GitResourceStatus>,
	pub(crate) diagnostics: Vec<String>,
	entries_by_def: FxHashMap<DefLocation, usize>,
	count_by_file: FxHashMap<usize, usize>,
}

impl Default for ChangeIndex {
	fn default() -> Self {
		Self {
			scope: "HEAD..worktree".to_string(),
			entries: Vec::new(),
			resources: Vec::new(),
			diagnostics: Vec::new(),
			entries_by_def: FxHashMap::default(),
			count_by_file: FxHashMap::default(),
		}
	}
}

impl ChangeIndex {
	pub(crate) fn entry_for(&self, loc: &DefLocation) -> Option<&ChangeEntry> {
		self.entries_by_def
			.get(loc)
			.and_then(|idx| self.entries.get(*idx))
	}

	pub(crate) fn changed_defs(&self) -> Vec<DefLocation> {
		self.entries.iter().filter_map(|entry| entry.loc).collect()
	}

	pub(crate) fn change_count_for_file(&self, file_idx: usize) -> usize {
		self.count_by_file.get(&file_idx).copied().unwrap_or(0)
	}

	pub(crate) fn changed_file_count(&self) -> usize {
		self.entries
			.iter()
			.map(|entry| entry.file_path.clone())
			.collect::<HashSet<_>>()
			.len()
	}

	fn rebuild_lookups(&mut self) {
		self.entries_by_def.clear();
		self.count_by_file.clear();
		for (idx, entry) in self.entries.iter().enumerate() {
			let Some(loc) = entry.loc else {
				continue;
			};
			self.entries_by_def.insert(loc, idx);
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

pub(crate) fn build_change_index(scan: ChangeScan<'_>) -> ChangeIndex {
	let mut changes = ChangeIndex::default();
	let mut diffs = Vec::new();
	for root in &scan.roots {
		match git_root_for(root.path) {
			Ok(git_root) => {
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
	for file in &scan.files {
		let file_path = normalize_path(file.path);
		let Some(diff) = diffs
			.iter()
			.find(|diff| normalize_path(&diff_path(diff)) == file_path)
		else {
			continue;
		};
		entries.extend(changed_entries_for_file(&scan, file, diff));
	}
	for diff in diffs
		.iter()
		.filter(|diff| diff.status == FileDiffStatus::Deleted)
	{
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
			Some(DefLocation {
				file: file.file_idx,
				def: def_idx,
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
					candidate != loc && is_descendant(file.graph, loc.def, candidate.def)
				})
		})
		.map(|loc| {
			let def = file.graph.def_at(loc.def);
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
					.map(|(start, end)| line_range(file.source, start, end)),
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
	let (start_line, end_line) = line_range(source, start, end);
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
	let graph = extract::extract_with(file.lang, &source, file.anchor, root.ctx);
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
					line_range: line_range(&source, start, end),
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
	let lang = path_to_lang(&path)?;
	let anchor = anchor_for(scan, root, &rel_path);
	let graph = extract::extract_with(lang, &source, &anchor, root.ctx);
	let mut entries = Vec::new();
	for def in graph.defs().filter(|def| is_navigable_def(lang, def)) {
		let Some((start, end)) = def.position else {
			continue;
		};
		let range = line_range(&source, start, end);
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
	for row in git_lines(
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
		let diff = git_text(
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
	for rel in git_lines(
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

fn git_root_for(path: &Path) -> Result<PathBuf, String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(path)
		.args(["rev-parse", "--show-toplevel"])
		.output()
		.map_err(|e| format!("cannot run git for {}: {e}", path.display()))?;
	if !output.status.success() {
		return Err(format!("{} is not inside a Git repository", path.display()));
	}
	let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
	if raw.is_empty() {
		return Err(format!("{} is not inside a Git repository", path.display()));
	}
	Ok(PathBuf::from(raw))
}

fn git_lines(git_root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
	Ok(git_text(git_root, args)?
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(ToOwned::to_owned)
		.collect())
}

fn git_show(git_root: &Path, repo_rel: &Path) -> anyhow::Result<String> {
	let spec = format!("HEAD:{}", path_to_git(repo_rel));
	git_text(git_root, &["show", &spec]).map_err(anyhow::Error::msg)
}

fn git_text(git_root: &Path, args: &[&str]) -> Result<String, String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(git_root)
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
