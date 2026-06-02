use crate::session::SessionStats;
use crate::ui::app::{
	App, ChangePanelMode, CheckState, FocusRegion, View, app_profile_name, app_rules_path,
	filter_label, is_filtered, selected, selected_change_detail,
};
use crate::ui::panel::{
	PanelVm, ReferenceGroupVm, SourceLineVm, panel_blank, panel_bullet, panel_component_section,
	panel_danger, panel_kv, panel_muted, panel_reference_groups, panel_section,
	panel_source_snippet, panel_table, panel_tree_rows,
};
use crate::ui::render::component::ComponentId;
use crate::ui::render::text::{Column, FitMode};
use crate::ui::render::tree::TreeRowVm;
use crate::ui::store::navigation::{NavigationPane, navigation_pane_view};
use crate::ui::store::navigation_tree::NavNodeKind;
use crate::ui::workspace_read::{
	self, ReferenceGroup, ReferenceSet, UnresolvedLinkageReport, UsageFocus,
};
use code_moniker_workspace::snapshot::SymbolId;

type DefLocation = SymbolId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ActivePanelNav {
	pub(in crate::ui) component: ComponentId,
	pub(in crate::ui) navigation_len: usize,
}

pub(super) fn active_panel(app: &App) -> PanelVm {
	match crate::ui::app::view(app) {
		View::Overview => overview_panel(app),
		View::Tree => outline_panel(app),
		View::Refs => refs_panel(app),
		View::Unresolved => unresolved_panel(app),
		View::Check => check_panel(app),
		View::Change => change_panel(app),
		View::Views => views_panel(app),
	}
}

pub(super) fn active_panel_nav(app: &App) -> ActivePanelNav {
	match crate::ui::app::view(app) {
		View::Overview => overview_panel_nav(app),
		View::Tree => outline_panel_nav(app),
		View::Refs => refs_panel_nav(app),
		View::Unresolved => unresolved_panel_nav(app),
		View::Check => ActivePanelNav {
			component: ComponentId::PanelCheck,
			navigation_len: 0,
		},
		View::Change => change_panel_nav(app),
		View::Views => ActivePanelNav {
			component: ComponentId::PanelViews,
			navigation_len: 0,
		},
	}
}

pub(super) fn active_panel_tree_rows(app: &App) -> Vec<TreeRowVm> {
	active_panel_tree_rows_with_expanded(app, &app.app_store.shell().panel_navigation.expanded)
}

pub(super) fn active_panel_tree_rows_with_expanded(
	app: &App,
	expanded: &std::collections::BTreeSet<String>,
) -> Vec<TreeRowVm> {
	match crate::ui::app::view(app) {
		View::Unresolved => unresolved_panel_tree_rows(app, expanded),
		_ => Vec::new(),
	}
}

pub(super) fn active_panel_default_expanded(app: &App) -> std::collections::BTreeSet<String> {
	match crate::ui::app::view(app) {
		View::Unresolved => unresolved_panel_default_expanded(app),
		_ => std::collections::BTreeSet::new(),
	}
}

fn views_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("views", ComponentId::PanelViews);
	let views = match crate::views::load_views(&crate::ui::app::store_options(app).paths) {
		Ok(views) => views,
		Err(error) => {
			panel_section(&mut vm, "views");
			panel_danger(&mut vm, format!("cannot load views: {error}"));
			return vm;
		}
	};
	if views.is_empty() {
		panel_section(&mut vm, "views");
		panel_muted(&mut vm, "no project views found");
		return vm;
	}
	let selected_view = selected_view_id(app);
	let view = selected_view
		.and_then(|id| views.iter().find(|view| view.spec.id == id))
		.unwrap_or(&views[0]);
	panel_section(&mut vm, "view lens");
	panel_kv(&mut vm, "id", view.spec.id.clone(), FitMode::Middle);
	panel_kv(
		&mut vm,
		"title",
		view.spec.title.clone().unwrap_or_default(),
		FitMode::Tail,
	);
	panel_kv(&mut vm, "fragment", view.fragment.clone(), FitMode::Tail);
	panel_kv(
		&mut vm,
		"scope",
		if view.scope_path.is_empty() {
			".".to_string()
		} else {
			view.scope_path.clone()
		},
		FitMode::Middle,
	);
	panel_kv(
		&mut vm,
		"moniker",
		format!("workspace/views/{}", view.spec.id),
		FitMode::Middle,
	);
	if let Some(intent) = &view.spec.intent {
		panel_blank(&mut vm);
		panel_section(&mut vm, "intent");
		panel_muted(&mut vm, intent.trim());
	}
	if let Some(summary) = &view.spec.summary {
		panel_blank(&mut vm);
		panel_section(&mut vm, "summary");
		panel_muted(&mut vm, summary.trim());
	}
	push_view_boundaries(&mut vm, &view.spec.boundaries);
	push_view_gotchas(&mut vm, &view.spec.gotchas);
	vm
}

fn selected_view_id(app: &App) -> Option<&str> {
	crate::ui::app::selected_nav_row(app).and_then(|row| match &row.kind {
		NavNodeKind::View { id, .. } => Some(id.as_str()),
		_ => row.view_ids.first().map(String::as_str),
	})
}

fn push_view_boundaries(vm: &mut PanelVm, boundaries: &[crate::views::BoundarySpec]) {
	if boundaries.is_empty() {
		return;
	}
	panel_blank(vm);
	panel_section(vm, "boundaries");
	for boundary in boundaries {
		panel_bullet(vm, format!("{} owns {}", boundary.id, boundary.owns.len()));
		for owns in boundary.owns.iter().take(3) {
			panel_muted(vm, format!("  owns {owns}"));
		}
		for forbids in &boundary.forbids {
			let status = if boundary.forbid_rules.is_empty() {
				"advisory"
			} else {
				"enforced"
			};
			panel_muted(vm, format!("  forbids {forbids} ({status})"));
		}
	}
}

fn push_view_gotchas(vm: &mut PanelVm, gotchas: &[crate::views::GotchaSpec]) {
	if gotchas.is_empty() {
		return;
	}
	panel_blank(vm);
	panel_section(vm, "gotchas");
	for gotcha in gotchas {
		panel_bullet(vm, gotcha.id.clone());
		if let Some(check) = &gotcha.check {
			panel_muted(vm, format!("  check {check}"));
		}
	}
}

fn overview_panel_nav(app: &App) -> ActivePanelNav {
	let stats = workspace_read::stats(crate::ui::app::store(app));
	ActivePanelNav {
		component: ComponentId::PanelOverview,
		navigation_len: stats.by_lang.len() + stats.by_shape.len(),
	}
}

fn overview_panel(app: &App) -> PanelVm {
	let stats = workspace_read::stats(crate::ui::app::store(app));
	let mut vm = PanelVm::new("overview", ComponentId::PanelOverview);
	overview_summary_section(&mut vm, app, &stats);
	overview_timing_section(&mut vm, &stats);
	overview_linkage_section(&mut vm, app);
	overview_languages_section(&mut vm, &stats);
	overview_shapes_section(&mut vm, &stats);
	vm
}

fn overview_summary_section(vm: &mut PanelVm, app: &App, stats: &SessionStats) {
	panel_section(vm, "summary");
	panel_kv(
		vm,
		"root",
		crate::ui::app::store_root_label(app),
		FitMode::Tail,
	);
	panel_kv(vm, "files", stats.files.to_string(), FitMode::Tail);
	panel_kv(vm, "defs", stats.defs.to_string(), FitMode::Tail);
	panel_kv(vm, "refs", stats.refs.to_string(), FitMode::Tail);
}

fn overview_timing_section(vm: &mut PanelVm, stats: &SessionStats) {
	let total_ms =
		stats.scan_ms + stats.extract_ms + stats.index_ms + stats.linkage_ms + stats.changes_ms;
	panel_kv(vm, "time", format!("{total_ms} ms"), FitMode::Tail);
	panel_kv(vm, "scan", format!("{} ms", stats.scan_ms), FitMode::Tail);
	panel_kv(
		vm,
		"extract",
		format!("{} ms", stats.extract_ms),
		FitMode::Tail,
	);
	panel_kv(vm, "index", format!("{} ms", stats.index_ms), FitMode::Tail);
	panel_kv(
		vm,
		"linkage",
		format!("{} ms", stats.linkage_ms),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"changes",
		format!("{} ms", stats.changes_ms),
		FitMode::Tail,
	);
}

fn overview_linkage_section(vm: &mut PanelVm, app: &App) {
	let linkage = workspace_read::linkage_stats(crate::ui::app::store(app));
	panel_blank(vm);
	panel_section(vm, "linkage");
	panel_kv(
		vm,
		"score",
		linkage
			.score_percent()
			.map(|score| format!("{score}%"))
			.unwrap_or_else(|| "n/a".to_string()),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"eligible",
		linkage.eligible_refs().to_string(),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"resolved",
		linkage.resolved_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"external",
		linkage.external_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"blocked",
		linkage.manifest_blocked_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"unresolved",
		linkage.unresolved_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"ambiguous",
		linkage.ambiguous_refs.to_string(),
		FitMode::Tail,
	);
}

fn overview_languages_section(vm: &mut PanelVm, stats: &SessionStats) {
	panel_blank(vm);
	panel_section(vm, "languages");
	panel_table(
		vm,
		vec![
			Column::left("lang", 10),
			Column::right("files", 7),
			Column::right("defs", 8),
			Column::right("refs", 8),
		],
		stats
			.by_lang
			.iter()
			.map(|(lang, totals)| {
				vec![
					lang.to_string(),
					totals.files.to_string(),
					totals.defs.to_string(),
					totals.refs.to_string(),
				]
			})
			.collect(),
	);
}

fn overview_shapes_section(vm: &mut PanelVm, stats: &SessionStats) {
	panel_blank(vm);
	panel_section(vm, "shapes");
	panel_table(
		vm,
		vec![Column::left("shape", 12), Column::right("count", 8)],
		stats
			.by_shape
			.iter()
			.map(|(shape, count)| vec![shape.to_string(), count.to_string()])
			.collect(),
	);
}

fn outline_panel_nav(app: &App) -> ActivePanelNav {
	let navigation_len = selected(app).map_or(0, |loc| {
		let detail = workspace_read::symbol_detail(crate::ui::app::store(app), &loc);
		let children = detail.children.len().min(40);
		let source = workspace_read::source_snippet(crate::ui::app::store(app), &loc, 3).len();
		children + source
	});
	ActivePanelNav {
		component: ComponentId::PanelOutline,
		navigation_len,
	}
}

fn outline_panel(app: &App) -> PanelVm {
	let Some(loc) = selected(app) else {
		return nav_selection_panel(app);
	};
	let detail = workspace_read::symbol_detail(crate::ui::app::store(app), &loc);
	let symbol = &detail.symbol;
	let mut vm = PanelVm::new("outline", ComponentId::PanelOutline).unwrapped();
	panel_section(&mut vm, "selected");
	panel_kv(&mut vm, "kind", symbol.kind.clone(), FitMode::Tail);
	panel_kv(&mut vm, "name", symbol.name.clone(), FitMode::Middle);
	panel_kv(
		&mut vm,
		"file",
		symbol.file_path.display().to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"moniker",
		symbol.compact_moniker.clone(),
		FitMode::Middle,
	);
	if let Some(change) = workspace_read::change_detail_for_symbol(crate::ui::app::store(app), &loc)
	{
		panel_blank(&mut vm);
		push_change_summary(&mut vm, &change);
	}
	panel_blank(&mut vm);
	panel_section(&mut vm, "children");
	if detail.children.is_empty() {
		panel_muted(&mut vm, "none");
	} else {
		panel_table(
			&mut vm,
			vec![Column::left("kind", 12), Column::left("name", 40)],
			detail
				.children
				.iter()
				.take(40)
				.map(|child| vec![child.kind.clone(), child.name.clone()])
				.collect(),
		);
		if detail.children.len() > 40 {
			panel_muted(&mut vm, format!("... {} more", detail.children.len() - 40));
		}
	}
	panel_blank(&mut vm);
	panel_component_section(&mut vm, "source", ComponentId::SourceSnippet);
	let snippet = source_snippet(app, &loc, 3);
	if snippet.is_empty() {
		panel_muted(&mut vm, "no source position");
	} else {
		panel_source_snippet(&mut vm, snippet);
	}
	vm
}

fn nav_selection_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("outline", ComponentId::PanelOutline).unwrapped();
	let pane = if crate::ui::app::focus_region(app) == FocusRegion::UsageLens {
		NavigationPane::UsageLens
	} else {
		NavigationPane::Primary
	};
	let Some(selection) = navigation_pane_view(crate::ui::app::navigation(app), pane)
		.and_then(|pane| pane.selected_context())
	else {
		if is_filtered(app) {
			panel_section(&mut vm, "filtered navigator");
			panel_kv(&mut vm, "filter", filter_label(app), FitMode::Tail);
			panel_kv(&mut vm, "matches", "0", FitMode::Tail);
			panel_blank(&mut vm);
			panel_muted(&mut vm, "x clears the filter");
		} else {
			panel_muted(&mut vm, "navigator is empty");
		}
		return vm;
	};
	let row = selection.row;
	let kind = match row.kind {
		NavNodeKind::Root => "root",
		NavNodeKind::Lang => "language",
		NavNodeKind::Dir => "directory",
		NavNodeKind::File(_) | NavNodeKind::ChangeFile => "file",
		NavNodeKind::Def(_) => "declaration",
		NavNodeKind::View { .. } => "view",
		NavNodeKind::ViewError => "view error",
		NavNodeKind::Change(_) => "change",
	};
	panel_section(&mut vm, "navigator");
	panel_kv(&mut vm, "kind", kind, FitMode::Tail);
	panel_kv(&mut vm, "name", row.label.clone(), FitMode::Middle);
	panel_kv(&mut vm, "files", row.file_count.to_string(), FitMode::Tail);
	panel_kv(&mut vm, "defs", row.def_count.to_string(), FitMode::Tail);
	panel_blank(&mut vm);
	if row.has_children {
		let state = if selection.expanded {
			"opened"
		} else {
			"closed"
		};
		panel_kv(&mut vm, "state", state, FitMode::Tail);
		panel_muted(&mut vm, "Enter toggles, right opens, left closes");
	} else {
		panel_muted(&mut vm, "no child node");
	}
	vm
}

fn refs_panel(app: &App) -> PanelVm {
	if let Some(focus) = crate::ui::app::usage_lens(app)
		&& (crate::ui::app::focus_region(app) != FocusRegion::UsageLens || selected(app).is_none())
	{
		return usage_focus_panel(focus);
	}
	let Some(loc) = selected(app) else {
		let mut vm = PanelVm::new("refs", ComponentId::PanelRefs);
		panel_muted(&mut vm, "select a declaration to inspect refs");
		return vm;
	};
	refs_for_symbol_panel(app, loc)
}

fn refs_panel_nav(app: &App) -> ActivePanelNav {
	if let Some(focus) = crate::ui::app::usage_lens(app)
		&& (crate::ui::app::focus_region(app) != FocusRegion::UsageLens || selected(app).is_none())
	{
		return ActivePanelNav {
			component: ComponentId::PanelUsages,
			navigation_len: reference_group_nav_len(&focus.references, 40),
		};
	}
	let Some(loc) = selected(app) else {
		return ActivePanelNav {
			component: ComponentId::PanelRefs,
			navigation_len: 0,
		};
	};
	let refs = workspace_read::symbol_references(crate::ui::app::store(app), &loc);
	ActivePanelNav {
		component: ComponentId::PanelRefs,
		navigation_len: reference_group_nav_len(&refs.incoming, 30)
			+ reference_group_nav_len(&refs.outgoing, 30),
	}
}

const UNRESOLVED_FILE_LIMIT: usize = 40;
const UNRESOLVED_SAMPLES_PER_FILE: usize = 3;

fn unresolved_panel_nav(app: &App) -> ActivePanelNav {
	ActivePanelNav {
		component: ComponentId::PanelUnresolved,
		navigation_len: unresolved_panel_tree_rows(
			app,
			&app.app_store.shell().panel_navigation.expanded,
		)
		.len(),
	}
}

fn unresolved_panel(app: &App) -> PanelVm {
	let report = workspace_read::unresolved_linkage_report(
		crate::ui::app::store(app),
		UNRESOLVED_FILE_LIMIT,
		UNRESOLVED_SAMPLES_PER_FILE,
	);
	let mut vm = PanelVm::new("unresolved", ComponentId::PanelUnresolved);
	panel_section(&mut vm, "summary");
	panel_kv(
		&mut vm,
		"unresolved",
		report.unresolved_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"blocked",
		report.manifest_blocked_refs.to_string(),
		FitMode::Tail,
	);
	panel_kv(&mut vm, "files", report.files.to_string(), FitMode::Tail);
	panel_kv(
		&mut vm,
		"shown",
		report.shown_files.to_string(),
		FitMode::Tail,
	);
	panel_blank(&mut vm);
	panel_section(&mut vm, "by file");
	if report.groups.is_empty() {
		panel_muted(&mut vm, "none");
		return vm;
	}
	panel_tree_rows(
		&mut vm,
		unresolved_tree_rows(&report, &app.app_store.shell().panel_navigation.expanded),
	);
	if report.files > report.shown_files {
		panel_blank(&mut vm);
		panel_muted(
			&mut vm,
			format!(
				"... {} more file group(s)",
				report.files - report.shown_files
			),
		);
	}
	vm
}

fn unresolved_panel_tree_rows(
	app: &App,
	expanded: &std::collections::BTreeSet<String>,
) -> Vec<TreeRowVm> {
	let report = workspace_read::unresolved_linkage_report(
		crate::ui::app::store(app),
		UNRESOLVED_FILE_LIMIT,
		UNRESOLVED_SAMPLES_PER_FILE,
	);
	unresolved_tree_rows(&report, expanded)
}

fn unresolved_panel_default_expanded(app: &App) -> std::collections::BTreeSet<String> {
	let report = workspace_read::unresolved_linkage_report(
		crate::ui::app::store(app),
		UNRESOLVED_FILE_LIMIT,
		UNRESOLVED_SAMPLES_PER_FILE,
	);
	let mut expanded = std::collections::BTreeSet::new();
	for group in report.groups {
		expanded.insert(unresolved_lang_key(group.lang.tag()));
		expanded.insert(unresolved_file_key(group.lang.tag(), &group.file_path));
	}
	expanded
}

fn unresolved_tree_rows(
	report: &UnresolvedLinkageReport,
	expanded: &std::collections::BTreeSet<String>,
) -> Vec<TreeRowVm> {
	let mut groups = report.groups.iter().collect::<Vec<_>>();
	groups.sort_by(|left, right| {
		left.lang.tag().cmp(right.lang.tag()).then_with(|| {
			let left_total = left.unresolved_refs + left.manifest_blocked_refs;
			let right_total = right.unresolved_refs + right.manifest_blocked_refs;
			right_total
				.cmp(&left_total)
				.then_with(|| left.file_path.cmp(&right.file_path))
		})
	});
	let mut rows = Vec::new();
	let mut current_lang = None;
	for group in groups {
		if current_lang != Some(group.lang) {
			current_lang = Some(group.lang);
			let lang_groups = report
				.groups
				.iter()
				.filter(|candidate| candidate.lang == group.lang)
				.collect::<Vec<_>>();
			let unresolved = lang_groups
				.iter()
				.map(|candidate| candidate.unresolved_refs)
				.sum::<usize>();
			let blocked = lang_groups
				.iter()
				.map(|candidate| candidate.manifest_blocked_refs)
				.sum::<usize>();
			let key = unresolved_lang_key(group.lang.tag());
			let is_expanded = expanded.contains(&key);
			rows.push(
				TreeRowVm::new(key, 0, format!("{}/", group.lang.tag()))
					.branch(is_expanded)
					.meta(format!(
						"{} files  unresolved {}  blocked {}",
						lang_groups.len(),
						unresolved,
						blocked
					)),
			);
		}
		if !expanded.contains(&unresolved_lang_key(group.lang.tag())) {
			continue;
		}
		let file_key = unresolved_file_key(group.lang.tag(), &group.file_path);
		let file_expanded = expanded.contains(&file_key);
		rows.push(
			TreeRowVm::new(file_key, 1, group.file_path.display().to_string())
				.branch(file_expanded)
				.meta(format!(
					"unresolved {}  blocked {}",
					group.unresolved_refs, group.manifest_blocked_refs
				)),
		);
		if !file_expanded {
			continue;
		}
		for sample in &group.samples {
			rows.push(
				TreeRowVm::new(
					format!(
						"unresolved:{}:{}:{}:{}",
						group.lang.tag(),
						group.file_path.display(),
						sample.reason,
						sample.target
					),
					2,
					format!("{} {}", sample.reason, short_target(&sample.target)),
				)
				.detail(format!(
					"from {} at {} ({})",
					sample.source, sample.location, sample.kind
				)),
			);
		}
	}
	rows
}

fn unresolved_lang_key(lang: &str) -> String {
	format!("unresolved:{lang}")
}

fn unresolved_file_key(lang: &str, path: &std::path::Path) -> String {
	format!("unresolved:{lang}:{}", path.display())
}

fn short_target(target: &str) -> &str {
	target
		.rsplit(['/', ':'])
		.find(|segment| !segment.is_empty())
		.unwrap_or(target)
}

fn source_snippet(app: &App, loc: &DefLocation, context: u32) -> Vec<SourceLineVm> {
	let snippet = workspace_read::source_snippet(crate::ui::app::store(app), loc, context);
	let width = snippet
		.iter()
		.map(|line| line.number.to_string().len())
		.max()
		.unwrap_or(4)
		.max(4);
	snippet
		.into_iter()
		.map(|line| SourceLineVm {
			number: line.number,
			number_width: width,
			text: line.text,
			active: line.active,
		})
		.collect()
}

pub(super) fn refs_for_symbol_panel(app: &App, loc: DefLocation) -> PanelVm {
	let refs = workspace_read::symbol_references(crate::ui::app::store(app), &loc);
	let mut vm = PanelVm::new("refs", ComponentId::PanelRefs);
	panel_section(&mut vm, "selected");
	panel_kv(&mut vm, "kind", refs.symbol.kind, FitMode::Tail);
	panel_kv(&mut vm, "name", refs.symbol.name, FitMode::Middle);
	panel_kv(
		&mut vm,
		"file",
		refs.symbol.file_path.display().to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"moniker",
		refs.symbol.compact_moniker,
		FitMode::Middle,
	);
	panel_blank(&mut vm);
	panel_section(&mut vm, "incoming impact");
	panel_muted(&mut vm, reference_summary(&refs.incoming));
	panel_reference_groups(&mut vm, reference_group_vms(&refs.incoming.groups), 30);
	panel_blank(&mut vm);
	panel_section(&mut vm, "outgoing dependencies");
	panel_muted(&mut vm, reference_summary(&refs.outgoing));
	panel_reference_groups(&mut vm, reference_group_vms(&refs.outgoing.groups), 30);
	vm
}

fn change_panel_nav(app: &App) -> ActivePanelNav {
	let navigation_len = selected_change_detail(app).map_or(0, |change| {
		if crate::ui::app::change_panel(app) == ChangePanelMode::Usages {
			reference_group_nav_len(&change.blast_radius, 40)
		} else {
			0
		}
	});
	ActivePanelNav {
		component: ComponentId::PanelChange,
		navigation_len,
	}
}

fn change_panel(app: &App) -> PanelVm {
	let Some(change) = selected_change_detail(app) else {
		return change_overview_panel(app);
	};
	match crate::ui::app::change_panel(app) {
		ChangePanelMode::Diff => change_diff_panel(&change),
		ChangePanelMode::Usages => change_usage_panel(&change),
	}
}

fn change_overview_panel(app: &App) -> PanelVm {
	let changes = workspace_read::change_overview(crate::ui::app::store(app));
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	panel_section(&mut vm, "change scope");
	panel_kv(&mut vm, "scope", changes.scope, FitMode::Tail);
	panel_kv(
		&mut vm,
		"changes",
		changes.change_count.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"files",
		changes.file_count.to_string(),
		FitMode::Tail,
	);
	panel_blank(&mut vm);
	panel_section(&mut vm, "git resources");
	if changes.resources.is_empty() {
		panel_muted(&mut vm, "none");
	} else {
		for resource in changes.resources {
			let status = if resource.available { "git" } else { "no git" };
			panel_kv(
				&mut vm,
				status,
				format!("{}: {}", resource.label, resource.message),
				FitMode::Middle,
			);
		}
	}
	if !changes.diagnostics.is_empty() {
		panel_blank(&mut vm);
		panel_danger(&mut vm, "diagnostics");
		for diagnostic in changes.diagnostics {
			panel_bullet(&mut vm, diagnostic);
		}
	}
	vm
}

fn change_diff_panel(change: &crate::ui::workspace_read::ChangeDetail) -> PanelVm {
	let summary = &change.summary;
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	panel_section(&mut vm, "changed symbol");
	panel_kv(&mut vm, "status", summary.status.label(), FitMode::Tail);
	panel_kv(&mut vm, "kind", summary.kind.clone(), FitMode::Tail);
	panel_kv(&mut vm, "symbol", summary.name.clone(), FitMode::Middle);
	panel_kv(
		&mut vm,
		"file",
		summary.file_path.display().to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"moniker",
		summary.compact_moniker.clone(),
		FitMode::Middle,
	);
	if let Some((start, end)) = summary.line_range {
		let range = if start == end {
			format!("L{start}")
		} else {
			format!("L{start}-L{end}")
		};
		panel_kv(&mut vm, "range", range, FitMode::Tail);
	}
	panel_kv(
		&mut vm,
		"hunks",
		summary.hunk_count.to_string(),
		FitMode::Tail,
	);
	panel_blank(&mut vm);
	push_blast_radius_summary(&mut vm, &change.blast_radius);
	panel_blank(&mut vm);
	panel_muted(&mut vm, "u toggles blast radius details");
	vm
}

fn change_usage_panel(change: &crate::ui::workspace_read::ChangeDetail) -> PanelVm {
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	push_blast_radius_summary(&mut vm, &change.blast_radius);
	panel_blank(&mut vm);
	panel_section(&mut vm, "references");
	if change.blast_radius.summary.refs == 0 {
		panel_muted(&mut vm, "none");
	} else {
		panel_reference_groups(
			&mut vm,
			reference_group_vms(&change.blast_radius.groups),
			40,
		);
	}
	vm
}

fn usage_focus_panel(focus: &UsageFocus) -> PanelVm {
	let mut vm = PanelVm::new("usages", ComponentId::PanelUsages);
	panel_section(&mut vm, "usage focus");
	panel_kv(&mut vm, "symbol", focus.label.clone(), FitMode::Middle);
	panel_kv(
		&mut vm,
		"moniker",
		focus.compact_moniker.clone(),
		FitMode::Middle,
	);
	panel_kv(&mut vm, "refs", focus.refs.len().to_string(), FitMode::Tail);
	panel_kv(
		&mut vm,
		"contexts",
		focus.contexts.len().to_string(),
		FitMode::Tail,
	);
	panel_blank(&mut vm);
	panel_section(&mut vm, "references");
	if focus.refs.is_empty() {
		panel_muted(&mut vm, "none");
	} else {
		panel_reference_groups(&mut vm, reference_group_vms(&focus.references.groups), 40);
	}
	vm
}

fn check_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("check", ComponentId::PanelCheck);
	match crate::ui::app::check_state(app) {
		CheckState::Pending => {
			panel_section(&mut vm, "check");
			panel_muted(
				&mut vm,
				"press c to run .code-moniker.toml rules on the loaded graph",
			);
			panel_kv(
				&mut vm,
				"rules",
				app_rules_path(app).display().to_string(),
				FitMode::Tail,
			);
			panel_kv(
				&mut vm,
				"profile",
				app_profile_name(app).unwrap_or("<none>"),
				FitMode::Tail,
			);
		}
		CheckState::Ready(summary) => {
			panel_section(&mut vm, "check summary");
			panel_kv(
				&mut vm,
				"files",
				summary.files_scanned.to_string(),
				FitMode::Tail,
			);
			panel_kv(
				&mut vm,
				"flagged",
				summary.files_with_violations.to_string(),
				FitMode::Tail,
			);
			panel_kv(
				&mut vm,
				"violations",
				summary.total_violations.to_string(),
				FitMode::Tail,
			);
		}
		CheckState::Error(error) => {
			panel_danger(&mut vm, "check failed");
			panel_bullet(&mut vm, error.clone());
		}
	}
	vm
}

fn push_change_summary(vm: &mut PanelVm, change: &crate::ui::workspace_read::ChangeDetail) {
	panel_section(vm, "change");
	panel_kv(vm, "status", change.summary.status.label(), FitMode::Tail);
	panel_kv(
		vm,
		"usages",
		change.summary.usage_count.to_string(),
		FitMode::Tail,
	);
}

fn push_blast_radius_summary(vm: &mut PanelVm, refs: &ReferenceSet) {
	panel_section(vm, "blast radius");
	panel_kv(
		vm,
		"direct",
		format!("{} direct usage(s)", refs.summary.refs),
		FitMode::Tail,
	);
	panel_kv(
		vm,
		"contexts",
		refs.summary.contexts.to_string(),
		FitMode::Tail,
	);
}

fn reference_summary(refs: &ReferenceSet) -> String {
	match (refs.summary.refs, refs.summary.files) {
		(0, _) => "0 reference(s)".to_string(),
		(count, 1) => format!("{count} reference(s) from 1 file"),
		(count, files) => format!("{count} reference(s) from {files} files"),
	}
}

fn reference_group_nav_len(refs: &ReferenceSet, limit: usize) -> usize {
	if refs.summary.refs == 0 {
		0
	} else {
		refs.groups.len().min(limit)
	}
}

fn reference_group_vms(groups: &[ReferenceGroup]) -> Vec<ReferenceGroupVm> {
	groups
		.iter()
		.map(|group| ReferenceGroupVm {
			kinds: group.kinds.clone(),
			actor: group.actor.clone(),
			location: group.location.clone(),
			endpoint_label: group.endpoint_label,
			endpoint: group.endpoint.clone(),
			confidence: group.confidence.clone(),
			receiver: group.receiver.clone(),
			alias: group.alias.clone(),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use super::*;
	use crate::session::SessionOptions;
	use crate::ui::app::{App, AppConfig};
	use crate::ui::workspace_read::load_local_workspace;

	fn write(root: &Path, rel: &str, body: &str) {
		let path = root.join(rel);
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).unwrap();
		}
		std::fs::write(path, body).unwrap();
	}

	fn fixture_app() -> App {
		let tmp = tempfile::tempdir().unwrap();
		write(
			tmp.path(),
			"src/services.ts",
			"export class AlphaService { run() { return 1; } }\nexport function betaFactory() { return new AlphaService(); }\n",
		);
		let opts = SessionOptions {
			paths: vec![tmp.path().to_path_buf()],
			project: Some("app".into()),
			cache_dir: None,
		};
		let (store, cache) = load_local_workspace(&opts).unwrap();
		crate::ui::app::new_app(
			store,
			cache,
			opts,
			AppConfig {
				scheme: "default".to_string(),
				rules: tmp.path().join("rules.toml"),
				profile: None,
				debug: false,
			},
		)
	}

	#[test]
	fn active_panel_nav_matches_render_panel_navigation_metadata() {
		let mut app = fixture_app();

		for view in [
			View::Overview,
			View::Tree,
			View::Refs,
			View::Unresolved,
			View::Check,
			View::Change,
			View::Views,
		] {
			crate::ui::app::set_view(&mut app, view, crate::ui::app::PanelPolicy::Manual);
			let panel = active_panel(&app);
			let nav = active_panel_nav(&app);
			assert_eq!(nav.component, panel.component(), "{view:?}");
			assert_eq!(nav.navigation_len, panel.navigation_len(), "{view:?}");
		}
	}
}
