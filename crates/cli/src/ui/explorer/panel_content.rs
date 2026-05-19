use crate::ui::app::{App, ChangePanelMode, CheckState, FocusRegion, View};
use crate::ui::panel::{PanelVm, ReferenceGroupVm, SourceLineVm};
use crate::ui::render::component::ComponentId;
use crate::ui::render::text::{Column, FitMode};
use crate::ui::store::navigation::NavigationPane;
use crate::ui::store::navigation_tree::NavNodeKind;
use crate::workspace::{DefLocation, IndexStore, ReferenceGroup, ReferenceSet, UsageFocus};

pub(super) fn active_panel(app: &App) -> PanelVm {
	match app.view() {
		View::Overview => overview_panel(app),
		View::Tree => outline_panel(app),
		View::Refs => refs_panel(app),
		View::Check => check_panel(app),
		View::Change => change_panel(app),
	}
}

fn overview_panel(app: &App) -> PanelVm {
	let stats = app.store().stats();
	let total_ms = stats.scan_ms + stats.extract_ms + stats.index_ms;
	let mut vm = PanelVm::new("overview", ComponentId::PanelOverview);
	vm.section("summary");
	vm.kv("root", app.store().root(), FitMode::Tail);
	vm.kv("files", stats.files.to_string(), FitMode::Tail);
	vm.kv("defs", stats.defs.to_string(), FitMode::Tail);
	vm.kv("refs", stats.refs.to_string(), FitMode::Tail);
	vm.kv("time", format!("{total_ms} ms"), FitMode::Tail);
	vm.kv("scan", format!("{} ms", stats.scan_ms), FitMode::Tail);
	vm.kv("extract", format!("{} ms", stats.extract_ms), FitMode::Tail);
	vm.kv("index", format!("{} ms", stats.index_ms), FitMode::Tail);
	vm.blank();
	vm.section("languages");
	vm.table(
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
	vm.blank();
	vm.section("shapes");
	vm.table(
		vec![Column::left("shape", 12), Column::right("count", 8)],
		stats
			.by_shape
			.iter()
			.map(|(shape, count)| vec![shape.to_string(), count.to_string()])
			.collect(),
	);
	vm
}

fn outline_panel(app: &App) -> PanelVm {
	let Some(loc) = app.selected() else {
		return nav_selection_panel(app);
	};
	let detail = app.store().symbol_detail(&loc);
	let symbol = &detail.symbol;
	let mut vm = PanelVm::new("outline", ComponentId::PanelOutline).unwrapped();
	vm.section("selected");
	vm.kv("kind", symbol.kind.clone(), FitMode::Tail);
	vm.kv("name", symbol.name.clone(), FitMode::Middle);
	vm.kv(
		"file",
		symbol.file_path.display().to_string(),
		FitMode::Tail,
	);
	vm.kv("moniker", symbol.compact_moniker.clone(), FitMode::Middle);
	if let Some(change) = app.store().change_detail_for_symbol(&loc) {
		vm.blank();
		push_change_summary(&mut vm, &change);
	}
	vm.blank();
	vm.section("children");
	if detail.children.is_empty() {
		vm.muted("none");
	} else {
		vm.table(
			vec![Column::left("kind", 12), Column::left("name", 40)],
			detail
				.children
				.iter()
				.take(40)
				.map(|child| vec![child.kind.clone(), child.name.clone()])
				.collect(),
		);
		if detail.children.len() > 40 {
			vm.muted(format!("... {} more", detail.children.len() - 40));
		}
	}
	vm.blank();
	vm.component_section("source", ComponentId::SourceSnippet);
	let snippet = source_snippet(app, &loc, 3);
	if snippet.is_empty() {
		vm.muted("no source position");
	} else {
		vm.source_snippet(snippet);
	}
	vm
}

fn nav_selection_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("outline", ComponentId::PanelOutline).unwrapped();
	let pane = if app.focus_region() == FocusRegion::UsageLens {
		NavigationPane::UsageLens
	} else {
		NavigationPane::Primary
	};
	let Some(selection) = app
		.navigation()
		.pane_view(pane)
		.and_then(|pane| pane.selected_context())
	else {
		if app.is_filtered() {
			vm.section("filtered navigator");
			vm.kv("filter", app.filter_label(), FitMode::Tail);
			vm.kv("matches", "0", FitMode::Tail);
			vm.blank();
			vm.muted("x clears the filter");
		} else {
			vm.muted("navigator is empty");
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
		NavNodeKind::Change(_) => "change",
	};
	vm.section("navigator");
	vm.kv("kind", kind, FitMode::Tail);
	vm.kv("name", row.label.clone(), FitMode::Middle);
	vm.kv("files", row.file_count.to_string(), FitMode::Tail);
	vm.kv("defs", row.def_count.to_string(), FitMode::Tail);
	vm.blank();
	if row.has_children {
		let state = if selection.expanded {
			"opened"
		} else {
			"closed"
		};
		vm.kv("state", state, FitMode::Tail);
		vm.muted("Enter toggles, right opens, left closes");
	} else {
		vm.muted("no child node");
	}
	vm
}

fn refs_panel(app: &App) -> PanelVm {
	if let Some(focus) = app.usage_lens()
		&& (app.focus_region() != FocusRegion::UsageLens || app.selected().is_none())
	{
		return usage_focus_panel(focus);
	}
	let Some(loc) = app.selected() else {
		let mut vm = PanelVm::new("refs", ComponentId::PanelRefs);
		vm.muted("select a declaration to inspect refs");
		return vm;
	};
	refs_for_symbol_panel(app, loc)
}

fn source_snippet(app: &App, loc: &DefLocation, context: u32) -> Vec<SourceLineVm> {
	let snippet = app.store().source_snippet(loc, context);
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
	let refs = app.store().symbol_references(&loc);
	let mut vm = PanelVm::new("refs", ComponentId::PanelRefs);
	vm.section("selected");
	vm.kv("kind", refs.symbol.kind, FitMode::Tail);
	vm.kv("name", refs.symbol.name, FitMode::Middle);
	vm.kv(
		"file",
		refs.symbol.file_path.display().to_string(),
		FitMode::Tail,
	);
	vm.kv("moniker", refs.symbol.compact_moniker, FitMode::Middle);
	vm.blank();
	vm.section("incoming impact");
	vm.muted(reference_summary(&refs.incoming));
	vm.reference_groups(reference_group_vms(&refs.incoming.groups), 30);
	vm.blank();
	vm.section("outgoing dependencies");
	vm.muted(reference_summary(&refs.outgoing));
	vm.reference_groups(reference_group_vms(&refs.outgoing.groups), 30);
	vm
}

fn change_panel(app: &App) -> PanelVm {
	let Some(change) = app.selected_change_detail() else {
		return change_overview_panel(app);
	};
	match app.change_panel() {
		ChangePanelMode::Diff => change_diff_panel(&change),
		ChangePanelMode::Usages => change_usage_panel(&change),
	}
}

fn change_overview_panel(app: &App) -> PanelVm {
	let changes = app.store().change_overview();
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	vm.section("change scope");
	vm.kv("scope", changes.scope, FitMode::Tail);
	vm.kv("changes", changes.change_count.to_string(), FitMode::Tail);
	vm.kv("files", changes.file_count.to_string(), FitMode::Tail);
	vm.blank();
	vm.section("git resources");
	if changes.resources.is_empty() {
		vm.muted("none");
	} else {
		for resource in changes.resources {
			let status = if resource.available { "git" } else { "no git" };
			vm.kv(
				status,
				format!("{}: {}", resource.label, resource.message),
				FitMode::Middle,
			);
		}
	}
	if !changes.diagnostics.is_empty() {
		vm.blank();
		vm.danger("diagnostics");
		for diagnostic in changes.diagnostics {
			vm.bullet(diagnostic);
		}
	}
	vm
}

fn change_diff_panel(change: &crate::workspace::ChangeDetail) -> PanelVm {
	let summary = &change.summary;
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	vm.section("changed symbol");
	vm.kv("status", summary.status.label(), FitMode::Tail);
	vm.kv("kind", summary.kind.clone(), FitMode::Tail);
	vm.kv("symbol", summary.name.clone(), FitMode::Middle);
	vm.kv(
		"file",
		summary.file_path.display().to_string(),
		FitMode::Tail,
	);
	vm.kv("moniker", summary.compact_moniker.clone(), FitMode::Middle);
	if let Some((start, end)) = summary.line_range {
		let range = if start == end {
			format!("L{start}")
		} else {
			format!("L{start}-L{end}")
		};
		vm.kv("range", range, FitMode::Tail);
	}
	vm.kv("hunks", summary.hunk_count.to_string(), FitMode::Tail);
	vm.blank();
	push_blast_radius_summary(&mut vm, &change.blast_radius);
	vm.blank();
	vm.muted("u toggles blast radius details");
	vm
}

fn change_usage_panel(change: &crate::workspace::ChangeDetail) -> PanelVm {
	let mut vm = PanelVm::new("change", ComponentId::PanelChange);
	push_blast_radius_summary(&mut vm, &change.blast_radius);
	vm.blank();
	vm.section("references");
	if change.blast_radius.summary.refs == 0 {
		vm.muted("none");
	} else {
		vm.reference_groups(reference_group_vms(&change.blast_radius.groups), 40);
	}
	vm
}

fn usage_focus_panel(focus: &UsageFocus) -> PanelVm {
	let mut vm = PanelVm::new("usages", ComponentId::PanelUsages);
	vm.section("usage focus");
	vm.kv("symbol", focus.label.clone(), FitMode::Middle);
	vm.kv("moniker", focus.compact_moniker.clone(), FitMode::Middle);
	vm.kv("refs", focus.refs.len().to_string(), FitMode::Tail);
	vm.kv("contexts", focus.contexts.len().to_string(), FitMode::Tail);
	vm.blank();
	vm.section("references");
	if focus.refs.is_empty() {
		vm.muted("none");
	} else {
		vm.reference_groups(reference_group_vms(&focus.references.groups), 40);
	}
	vm
}

fn check_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("check", ComponentId::PanelCheck);
	match app.check_state() {
		CheckState::Pending => {
			vm.section("check");
			vm.muted("press c to run .code-moniker.toml rules on the loaded graph");
			vm.kv(
				"rules",
				app.rules_path().display().to_string(),
				FitMode::Tail,
			);
			vm.kv(
				"profile",
				app.profile_name().unwrap_or("<none>"),
				FitMode::Tail,
			);
		}
		CheckState::Ready(summary) => {
			vm.section("check summary");
			vm.kv("files", summary.files_scanned.to_string(), FitMode::Tail);
			vm.kv(
				"flagged",
				summary.files_with_violations.to_string(),
				FitMode::Tail,
			);
			vm.kv(
				"violations",
				summary.total_violations.to_string(),
				FitMode::Tail,
			);
		}
		CheckState::Error(error) => {
			vm.danger("check failed");
			vm.bullet(error.clone());
		}
	}
	vm
}

fn push_change_summary(vm: &mut PanelVm, change: &crate::workspace::ChangeDetail) {
	vm.section("change");
	vm.kv("status", change.summary.status.label(), FitMode::Tail);
	vm.kv(
		"usages",
		change.summary.usage_count.to_string(),
		FitMode::Tail,
	);
}

fn push_blast_radius_summary(vm: &mut PanelVm, refs: &ReferenceSet) {
	vm.section("blast radius");
	vm.kv(
		"direct",
		format!("{} direct usage(s)", refs.summary.refs),
		FitMode::Tail,
	);
	vm.kv("contexts", refs.summary.contexts.to_string(), FitMode::Tail);
}

fn reference_summary(refs: &ReferenceSet) -> String {
	match (refs.summary.refs, refs.summary.files) {
		(0, _) => "0 reference(s)".to_string(),
		(count, 1) => format!("{count} reference(s) from 1 file"),
		(count, files) => format!("{count} reference(s) from {files} files"),
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
