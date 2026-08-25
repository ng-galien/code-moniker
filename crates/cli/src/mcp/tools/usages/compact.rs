use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use code_moniker_query::{SymbolUsagesResult, UsageDto};
use serde::Serialize;

use super::{EvidenceMode, TechnicalMode, UsageRequest, usage_kind_priority};
use crate::mcp::context::McpContext;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum UsageClass {
	Production,
	Test,
	Technical,
}

impl UsageClass {
	fn as_str(self) -> &'static str {
		match self {
			Self::Production => "production",
			Self::Test => "test",
			Self::Technical => "technical",
		}
	}
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct UsageGroupKey<'a> {
	direction: &'static str,
	class: UsageClass,
	kind: &'a str,
	root: &'a str,
	file: &'a str,
	actor: &'a str,
	context: &'a str,
	endpoint: &'a str,
	via: Option<&'a str>,
}

struct UsageGroup<'a> {
	key: UsageGroupKey<'a>,
	rows: Vec<&'a UsageDto>,
}

#[derive(Serialize)]
pub(super) struct CompactUsageMap {
	pub(super) page_refs: usize,
	pub(super) technical_label: &'static str,
	pub(super) technical_refs: usize,
	pub(super) technical_groups: usize,
	pub(super) groups: Vec<CompactUsageGroup>,
}

#[derive(Serialize)]
pub(super) struct CompactUsageGroup {
	pub(super) direction: &'static str,
	pub(super) class: &'static str,
	pub(super) kind: String,
	pub(super) label: String,
	pub(super) location: String,
	pub(super) ref_count: usize,
	pub(super) context: Option<String>,
	pub(super) endpoint: Option<String>,
	pub(super) via: Option<String>,
	pub(super) evidence: Option<UsageSourceSnippet>,
}

impl UsageGroup<'_> {
	fn primary_kind(&self) -> &str {
		self.key.kind
	}

	fn representative(&self) -> &UsageDto {
		self.rows[0]
	}

	fn evidence_eligible(&self) -> bool {
		self.key.class != UsageClass::Technical
			&& self.rows.iter().any(|row| row.line_range.is_some())
	}
}

pub(super) fn compact_usage_map(
	context: &McpContext,
	result: &SymbolUsagesResult,
	request: &UsageRequest,
) -> CompactUsageMap {
	let type_target = is_type_symbol_kind(&result.target.kind);
	let groups = group_daemon_usages(&result.rows, type_target);
	let technical_refs = groups
		.iter()
		.filter(|group| group.key.class == UsageClass::Technical)
		.map(|group| group.rows.len())
		.sum::<usize>();
	let technical_groups = groups
		.iter()
		.filter(|group| group.key.class == UsageClass::Technical)
		.count();
	let visible = groups
		.iter()
		.filter(|group| {
			request.technical == TechnicalMode::Include || group.key.class != UsageClass::Technical
		})
		.collect::<Vec<_>>();
	let evidence = if request.evidence == EvidenceMode::Representative {
		representative_evidence_indices(&visible, request.max_evidence)
	} else {
		BTreeSet::new()
	};

	let groups = visible
		.iter()
		.enumerate()
		.map(|(index, group)| {
			compact_usage_group(context, group, evidence.contains(&index), request)
		})
		.collect();
	CompactUsageMap {
		page_refs: result.rows.len(),
		technical_label: if request.technical == TechnicalMode::Include {
			"technical included"
		} else {
			"technical omitted"
		},
		technical_refs,
		technical_groups,
		groups,
	}
}

fn group_daemon_usages(rows: &[UsageDto], type_target: bool) -> Vec<UsageGroup<'_>> {
	let mut grouped = BTreeMap::<UsageGroupKey<'_>, Vec<&UsageDto>>::new();
	for row in rows {
		let direction = row.direction.as_str();
		let incoming = direction == "incoming";
		let key = UsageGroupKey {
			direction,
			class: usage_class(row, type_target),
			kind: &row.kind,
			root: &row.root,
			file: &row.file,
			actor: if incoming { &row.actor } else { "" },
			context: if incoming { &row.context } else { "" },
			endpoint: if incoming { "" } else { &row.endpoint },
			via: row.via.as_deref(),
		};
		grouped.entry(key).or_default().push(row);
	}
	let mut groups = grouped
		.into_iter()
		.map(|(key, mut rows)| {
			rows.sort_by(|left, right| {
				usage_kind_priority(&left.kind)
					.cmp(&usage_kind_priority(&right.kind))
					.then_with(|| left.line_range.cmp(&right.line_range))
					.then_with(|| left.reference.cmp(&right.reference))
			});
			UsageGroup { key, rows }
		})
		.collect::<Vec<_>>();
	groups.sort_by(|left, right| {
		direction_priority(left.key.direction)
			.cmp(&direction_priority(right.key.direction))
			.then_with(|| left.key.class.cmp(&right.key.class))
			.then_with(|| {
				usage_kind_priority(left.primary_kind())
					.cmp(&usage_kind_priority(right.primary_kind()))
			})
			.then_with(|| Reverse(left.rows.len()).cmp(&Reverse(right.rows.len())))
			.then_with(|| left.key.root.cmp(right.key.root))
			.then_with(|| left.key.file.cmp(right.key.file))
			.then_with(|| left.key.context.cmp(right.key.context))
			.then_with(|| left.key.endpoint.cmp(right.key.endpoint))
	});
	groups
}

fn representative_evidence_indices(
	groups: &[&UsageGroup<'_>],
	max_evidence: usize,
) -> BTreeSet<usize> {
	let mut selected = BTreeSet::new();
	if max_evidence == 0 {
		return selected;
	}
	for direction in ["incoming", "outgoing"] {
		if let Some((index, _)) = groups
			.iter()
			.enumerate()
			.find(|(_, group)| group.key.direction == direction && group.evidence_eligible())
		{
			selected.insert(index);
			if selected.len() == max_evidence {
				return selected;
			}
		}
	}
	let mut candidates = groups
		.iter()
		.enumerate()
		.filter(|(index, group)| !selected.contains(index) && group.evidence_eligible())
		.collect::<Vec<_>>();
	candidates.sort_by(|(left_index, left), (right_index, right)| {
		left.key
			.class
			.cmp(&right.key.class)
			.then_with(|| {
				usage_kind_priority(left.primary_kind())
					.cmp(&usage_kind_priority(right.primary_kind()))
			})
			.then_with(|| Reverse(left.rows.len()).cmp(&Reverse(right.rows.len())))
			.then_with(|| left_index.cmp(right_index))
	});
	for (index, _) in candidates {
		selected.insert(index);
		if selected.len() == max_evidence {
			break;
		}
	}
	selected
}

fn compact_usage_group(
	context: &McpContext,
	group: &UsageGroup<'_>,
	include_evidence: bool,
	request: &UsageRequest,
) -> CompactUsageGroup {
	let row = group.representative();
	let label = if group.key.direction == "incoming" {
		row.actor.to_string()
	} else {
		short_identity(&row.endpoint).to_string()
	};
	CompactUsageGroup {
		direction: if group.key.direction == "incoming" {
			"in"
		} else {
			"out"
		},
		class: group.key.class.as_str(),
		kind: group.primary_kind().to_string(),
		label,
		location: row.location.to_string(),
		ref_count: group.rows.len(),
		context: (group.key.direction == "incoming").then(|| row.context.to_string()),
		endpoint: (group.key.direction != "incoming").then(|| row.endpoint.to_string()),
		via: group.key.via.map(str::to_string),
		evidence: include_evidence
			.then(|| usage_source_snippet(context, row, request.context_lines))
			.flatten(),
	}
}

fn usage_class(row: &UsageDto, type_target: bool) -> UsageClass {
	if is_technical_usage(&row.kind, type_target) {
		UsageClass::Technical
	} else if is_test_file(&row.file) || is_test_context(&row.context) {
		UsageClass::Test
	} else {
		UsageClass::Production
	}
}

fn is_test_context(context: &str) -> bool {
	context
		.split('/')
		.any(|segment| segment.starts_with("test:") || segment == "module:tests")
}

fn is_technical_usage(kind: &str, type_target: bool) -> bool {
	let kind = kind.to_ascii_lowercase();
	if kind == "imports" || kind.starts_with("imports_") || kind == "annotates" {
		return true;
	}
	!type_target && matches!(kind.as_str(), "uses_type" | "returns_type")
}

fn is_type_symbol_kind(kind: &str) -> bool {
	matches!(
		kind.to_ascii_lowercase().as_str(),
		"class" | "struct" | "enum" | "interface" | "trait" | "type" | "union" | "object"
	)
}

fn is_test_file(file: &str) -> bool {
	let normalized = file.replace('\\', "/").to_ascii_lowercase();
	normalized
		.split(['/', '.'])
		.any(|segment| matches!(segment, "test" | "tests" | "spec" | "specs" | "__tests__"))
}

fn direction_priority(direction: &str) -> u8 {
	match direction {
		"incoming" => 0,
		"outgoing" => 1,
		_ => 2,
	}
}

fn short_identity(identity: &str) -> &str {
	identity.rsplit('/').next().unwrap_or(identity)
}

#[derive(Serialize)]
pub(super) struct UsageSourceSnippet {
	pub(super) code: String,
}

fn usage_source_snippet(
	context: &McpContext,
	row: &UsageDto,
	context_lines: usize,
) -> Option<UsageSourceSnippet> {
	let (start, end) = row.line_range?;
	let path = usage_source_path(context, row)?;
	let text = std::fs::read_to_string(path).ok()?;
	let context_lines = context_lines as u32;
	let active_end = end.min(start.saturating_add(4));
	let first = start.saturating_sub(context_lines).max(1);
	let last = active_end.saturating_add(context_lines);
	let lines = text
		.lines()
		.enumerate()
		.filter_map(|(index, text)| {
			let number = index as u32 + 1;
			(number >= first && number <= last).then(|| {
				format!(
					"{} {number:>4} | {text}",
					if number >= start && number <= active_end {
						">"
					} else {
						" "
					}
				)
			})
		})
		.collect::<Vec<_>>();
	(!lines.is_empty()).then(|| UsageSourceSnippet {
		code: lines.join("\n"),
	})
}

fn usage_source_path(context: &McpContext, row: &UsageDto) -> Option<PathBuf> {
	confined_source_path(&context.opts().paths, &row.root, &row.file)
}

fn confined_source_path(roots: &[PathBuf], row_root: &str, file: &str) -> Option<PathBuf> {
	let file = Path::new(file);
	if file.as_os_str().is_empty()
		|| file.is_absolute()
		|| file
			.components()
			.any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
	{
		return None;
	}
	let preferred_root = std::fs::canonicalize(row_root).ok();
	let mut canonical_roots = roots
		.iter()
		.filter_map(|root| std::fs::canonicalize(root).ok())
		.collect::<Vec<_>>();
	canonical_roots.sort_by_key(|root| preferred_root.as_ref() != Some(root));
	canonical_roots.dedup();
	for root in canonical_roots {
		let Ok(candidate) = std::fs::canonicalize(root.join(file)) else {
			continue;
		};
		if candidate.starts_with(&root) && candidate.is_file() {
			return Some(candidate);
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use code_moniker_query::{UsageDirection, UsageDto};

	use super::{UsageClass, confined_source_path, is_technical_usage, is_test_file, usage_class};

	#[test]
	fn technical_usage_filter_preserves_type_semantics_for_type_targets() {
		assert!(is_technical_usage("imports_symbol", false));
		assert!(is_technical_usage("imports_module", true));
		assert!(is_technical_usage("annotates", true));
		assert!(is_technical_usage("uses_type", false));
		assert!(is_technical_usage("returns_type", false));
		assert!(!is_technical_usage("uses_type", true));
		assert!(!is_technical_usage("returns_type", true));
		assert!(!is_technical_usage("calls", false));
	}

	#[test]
	fn test_file_detection_handles_directories_and_common_suffixes() {
		assert!(is_test_file("src/test/java/AppTest.java"));
		assert!(is_test_file("src/widgets/button.spec.ts"));
		assert!(is_test_file("src/__tests__/button.ts"));
		assert!(is_test_file("crates/cli/src/mcp/tests.rs"));
		assert!(!is_test_file("src/contest/Statistics.java"));
	}

	#[test]
	fn inline_cfg_test_context_classifies_as_test() {
		let row = |context: &str| UsageDto {
			root: String::new(),
			direction: UsageDirection::Incoming,
			reference: String::new(),
			kind: "method_call".to_string(),
			actor: "caller()".to_string(),
			context: context.to_string(),
			endpoint: String::new(),
			file: "crates/check/src/check/command.rs".to_string(),
			prefix: "crates".to_string(),
			location: String::new(),
			line_range: None,
			via: None,
		};

		let inline_test = row(concat!(
			"code+moniker://./lang:rs/dir:crates/module:command/module:tests",
			"/test:false_lazy_rule_does_not_build_the_source_catalog()"
		));
		assert_eq!(usage_class(&inline_test, false), UsageClass::Test);

		let production = row("code+moniker://./lang:rs/dir:crates/module:command/fn:run()");
		assert_eq!(usage_class(&production, false), UsageClass::Production);
	}

	#[test]
	fn source_evidence_stays_inside_configured_roots() {
		let workspace = tempfile::tempdir().expect("workspace");
		let outside = tempfile::tempdir().expect("outside");
		let source = workspace.path().join("src/lib.rs");
		std::fs::create_dir_all(source.parent().expect("source parent")).expect("mkdir");
		std::fs::write(&source, "fn inside() {}\n").expect("source");
		let secret = outside.path().join("secret.rs");
		std::fs::write(&secret, "fn secret() {}\n").expect("secret");
		let roots = vec![workspace.path().to_path_buf()];

		assert_eq!(
			confined_source_path(
				&roots,
				&workspace.path().display().to_string(),
				"src/lib.rs"
			),
			std::fs::canonicalize(source).ok()
		);
		assert!(
			confined_source_path(
				&roots,
				&workspace.path().display().to_string(),
				&secret.display().to_string(),
			)
			.is_none()
		);
		assert!(
			confined_source_path(
				&roots,
				&workspace.path().display().to_string(),
				"../secret.rs",
			)
			.is_none()
		);
	}

	#[cfg(unix)]
	#[test]
	fn source_evidence_rejects_symlinks_leaving_the_workspace() {
		let workspace = tempfile::tempdir().expect("workspace");
		let outside = tempfile::tempdir().expect("outside");
		let secret = outside.path().join("secret.rs");
		std::fs::write(&secret, "fn secret() {}\n").expect("secret");
		std::os::unix::fs::symlink(&secret, workspace.path().join("leak.rs")).expect("symlink");

		assert!(
			confined_source_path(
				&[workspace.path().to_path_buf()],
				&workspace.path().display().to_string(),
				"leak.rs",
			)
			.is_none()
		);
	}
}
