// code-moniker: ignore-file[smell-clone-reflex]
// Panel content builds owned render view models from borrowed app/workspace state.
use crate::session::SessionStats;
use crate::ui::app::{
	App, ChangePanelMode, CheckState, FocusRegion, View, app_profile_name, app_rules_path,
	filter_label, is_filtered, notes_error, selected, selected_change_detail, sort_notes_for_lens,
};
use crate::ui::panel::{
	PanelVm, ReferenceGroupVm, SourceLineVm, panel_blank, panel_bullet, panel_component_section,
	panel_danger, panel_evidence, panel_info, panel_kv, panel_muted, panel_reference_groups,
	panel_section, panel_selector, panel_source_snippet, panel_table, panel_text_editor,
	panel_tree_rows, panel_warning,
};
use crate::ui::render::component::ComponentId;
use crate::ui::render::text::{Column, FitMode};
use crate::ui::render::tree::TreeRowVm;
use crate::ui::store::navigation::{NavigationPane, navigation_pane_view};
use crate::ui::store::navigation_tree::NavNodeKind;
use crate::ui::workspace_read::{
	self, ReferenceGroup, ReferenceSet, UnresolvedLinkageReport, UsageFocus,
};
use code_moniker_workspace::notes::{NoteResolution, resolve_notes};
use code_moniker_workspace::snapshot::SymbolId;

type DefLocation = SymbolId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ActivePanelNav {
	pub(in crate::ui) component: ComponentId,
	pub(in crate::ui) navigation_len: usize,
}

pub(super) fn active_panel(app: &App) -> PanelVm {
	if crate::ui::app::note_editor(app).is_some() {
		return note_editor_panel(app);
	}
	match crate::ui::app::view(app) {
		View::Overview => overview_panel(app),
		View::Tree => outline_panel(app),
		View::Refs => refs_panel(app),
		View::Unresolved => unresolved_panel(app),
		View::Check => check_panel(app),
		View::Change => change_panel(app),
		View::Views => views_panel(app),
		View::Notes => notes_panel(app),
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
		View::Notes => ActivePanelNav {
			component: ComponentId::PanelNotes,
			navigation_len: crate::ui::app::notes(app).notes().len(),
		},
	}
}

fn note_editor_panel(app: &App) -> PanelVm {
	let Some(editor) = crate::ui::app::note_editor(app) else {
		return PanelVm::new("note", ComponentId::PanelNotes);
	};
	let mut vm = PanelVm::new("note editor", ComponentId::PanelNotes).unwrapped();
	panel_section(&mut vm, "note editor");
	panel_kv(
		&mut vm,
		"target",
		editor.target_label.clone(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"moniker",
		editor.target_moniker.clone(),
		FitMode::Middle,
	);
	panel_kv(&mut vm, "kind", editor.kind.as_str(), FitMode::Tail);
	panel_kv(&mut vm, "status", editor.status.as_str(), FitMode::Tail);
	panel_kv(
		&mut vm,
		"field",
		match editor.field {
			crate::ui::app::NoteEditorField::Kind => "kind",
			crate::ui::app::NoteEditorField::Title => "title",
			crate::ui::app::NoteEditorField::Body => "body",
		},
		FitMode::Tail,
	);
	if editor.dirty {
		panel_warning(&mut vm, "unsaved changes");
	}
	if editor.confirm_delete {
		panel_danger(&mut vm, "press Ctrl+d again to delete this note");
	}
	panel_blank(&mut vm);
	panel_selector(
		&mut vm,
		"kind",
		note_kind_options(),
		note_kind_index(editor.kind),
		editor.field == crate::ui::app::NoteEditorField::Kind,
	);
	panel_blank(&mut vm);
	panel_text_editor(
		&mut vm,
		"title",
		editor.title.clone(),
		3,
		editor.field == crate::ui::app::NoteEditorField::Title,
	);
	panel_blank(&mut vm);
	panel_text_editor(
		&mut vm,
		"body",
		editor.body.clone(),
		10,
		editor.field == crate::ui::app::NoteEditorField::Body,
	);
	panel_blank(&mut vm);
	panel_section(&mut vm, "controls");
	panel_muted(
		&mut vm,
		"Up/Down field  Left/Right kind  Tab/Shift+Tab field  Enter newline in body  Ctrl+s save  Ctrl+d delete",
	);
	vm
}

fn note_kind_options() -> Vec<String> {
	["note", "todo", "gotcha", "request"]
		.into_iter()
		.map(ToOwned::to_owned)
		.collect()
}

fn note_kind_index(kind: code_moniker_workspace::notes::NoteKind) -> usize {
	match kind {
		code_moniker_workspace::notes::NoteKind::Note => 0,
		code_moniker_workspace::notes::NoteKind::Todo => 1,
		code_moniker_workspace::notes::NoteKind::Gotcha => 2,
		code_moniker_workspace::notes::NoteKind::Request => 3,
	}
}

fn notes_panel(app: &App) -> PanelVm {
	let mut vm = PanelVm::new("notes", ComponentId::PanelNotes).unwrapped();
	let resolved = resolved_notes_for_panel(app);
	let counts = note_counts(&resolved);
	panel_section(&mut vm, "notes lens");
	panel_kv(
		&mut vm,
		"pending",
		counts.pending.to_string(),
		FitMode::Tail,
	);
	panel_kv(
		&mut vm,
		"ongoing",
		counts.ongoing.to_string(),
		FitMode::Tail,
	);
	panel_kv(&mut vm, "done", counts.done.to_string(), FitMode::Tail);
	panel_kv(&mut vm, "orphan", counts.orphan.to_string(), FitMode::Tail);
	if let Some(error) = notes_error(app) {
		panel_blank(&mut vm);
		panel_danger(&mut vm, "notes unavailable");
		panel_muted(&mut vm, error.to_string());
	}
	panel_blank(&mut vm);
	panel_table(
		&mut vm,
		vec![
			Column::left("res", 7),
			Column::left("status", 9),
			Column::left("kind", 9),
			Column::left("target", 28),
			Column::left("title", 44),
		],
		resolved
			.iter()
			.map(|item| {
				vec![
					note_resolution_flag(item),
					item.note.status.as_str().to_string(),
					item.note.kind.as_str().to_string(),
					note_target_label(item),
					item.note.title.clone(),
				]
			})
			.collect(),
	);
	if resolved.is_empty() {
		panel_muted(&mut vm, "no notes");
	}
	if let Some(selected) = app
		.app_store
		.shell()
		.panel_navigation
		.selected
		.or(Some(0))
		.and_then(|idx| resolved.get(idx))
	{
		panel_blank(&mut vm);
		push_note_detail(&mut vm, selected);
	}
	vm
}

fn resolved_notes_for_panel(app: &App) -> Vec<code_moniker_workspace::notes::ResolvedNote> {
	let mut notes = crate::ui::app::notes(app).notes().to_vec();
	sort_notes_for_lens(&mut notes);
	if let Some(snapshot) = crate::ui::app::store(app).queries().snapshot() {
		resolve_notes(&notes, snapshot)
	} else {
		notes
			.into_iter()
			.map(|note| code_moniker_workspace::notes::ResolvedNote {
				note,
				resolution: NoteResolution::Orphan,
			})
			.collect()
	}
}

fn note_resolution_flag(note: &code_moniker_workspace::notes::ResolvedNote) -> String {
	match &note.resolution {
		NoteResolution::Resolved { .. } => "ok".to_string(),
		NoteResolution::Orphan => "orphan".to_string(),
	}
}

fn note_target_label(note: &code_moniker_workspace::notes::ResolvedNote) -> String {
	match &note.resolution {
		NoteResolution::Resolved { target_label, .. } => target_label.clone(),
		NoteResolution::Orphan => "orphan".to_string(),
	}
}

fn push_note_detail(vm: &mut PanelVm, note: &code_moniker_workspace::notes::ResolvedNote) {
	panel_section(vm, "selected note");
	panel_kv(vm, "id", note.note.id.as_str(), FitMode::Tail);
	panel_kv(vm, "status", note.note.status.as_str(), FitMode::Tail);
	panel_kv(vm, "kind", note.note.kind.as_str(), FitMode::Tail);
	panel_kv(vm, "title", note.note.title.clone(), FitMode::Tail);
	match &note.resolution {
		NoteResolution::Resolved {
			target_label,
			target_file,
			..
		} => {
			panel_kv(vm, "target", target_label.clone(), FitMode::Tail);
			panel_kv(vm, "file", target_file.clone(), FitMode::Tail);
			panel_kv(vm, "orphan", "no", FitMode::Tail);
		}
		NoteResolution::Orphan => {
			panel_kv(vm, "target", "orphan", FitMode::Tail);
			panel_kv(vm, "orphan", "yes", FitMode::Tail);
		}
	}
	panel_kv(vm, "moniker", note.note.moniker.clone(), FitMode::Middle);
	if !note.note.body.is_empty() {
		panel_blank(vm);
		panel_section(vm, "body");
		for line in note.note.body.lines().take(12) {
			panel_muted(vm, line.to_string());
		}
	}
}

#[derive(Default)]
struct NoteCounts {
	pending: usize,
	ongoing: usize,
	done: usize,
	orphan: usize,
}

fn note_counts(notes: &[code_moniker_workspace::notes::ResolvedNote]) -> NoteCounts {
	let mut counts = NoteCounts::default();
	for note in notes {
		match note.note.status {
			code_moniker_workspace::notes::NoteStatus::Pending => counts.pending += 1,
			code_moniker_workspace::notes::NoteStatus::Ongoing => counts.ongoing += 1,
			code_moniker_workspace::notes::NoteStatus::Done => counts.done += 1,
		}
		if note.resolution.is_orphan() {
			counts.orphan += 1;
		}
	}
	counts
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
	let show_all = crate::ui::app::views_show_all(app);
	panel_kv(
		&mut vm,
		"render",
		if show_all { "all" } else { "summary" },
		FitMode::Tail,
	);
	panel_muted(&mut vm, "press a to toggle summary/all");
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
	let snapshot = crate::ui::app::store(app).queries().snapshot();
	push_view_boundaries(&mut vm, view, snapshot, show_all);
	push_view_gotchas(&mut vm, view, snapshot, show_all);
	vm
}

fn selected_view_id(app: &App) -> Option<&str> {
	crate::ui::app::selected_nav_row(app).and_then(|row| match &row.kind {
		NavNodeKind::View { id, .. } => Some(id.as_str()),
		_ => row.view_ids.first().map(String::as_str),
	})
}

fn push_view_boundaries(
	vm: &mut PanelVm,
	view: &crate::views::ViewDocument,
	snapshot: Option<&code_moniker_workspace::snapshot::WorkspaceSnapshot>,
	show_all: bool,
) {
	let boundaries = &view.spec.boundaries;
	if boundaries.is_empty() {
		return;
	}
	panel_blank(vm);
	panel_section(vm, "boundaries");
	for boundary in boundaries {
		panel_bullet(vm, format!("{} owns {}", boundary.id, boundary.owns.len()));
		for owns in boundary.owns.iter().take(3) {
			panel_info(vm, format!("  owns    {owns}"));
		}
		for forbids in &boundary.forbids {
			let status = if boundary.forbid_rules.is_empty() {
				"advisory"
			} else {
				"enforced"
			};
			panel_warning(vm, format!("  forbids {forbids} ({status})"));
		}
		if show_all {
			push_view_symbols(vm, view, snapshot, &boundary.symbols);
		}
	}
}

fn push_view_gotchas(
	vm: &mut PanelVm,
	view: &crate::views::ViewDocument,
	snapshot: Option<&code_moniker_workspace::snapshot::WorkspaceSnapshot>,
	show_all: bool,
) {
	let gotchas = &view.spec.gotchas;
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
		if show_all {
			push_view_symbols(vm, view, snapshot, &gotcha.symbols);
		}
	}
}

fn push_view_symbols(
	vm: &mut PanelVm,
	view: &crate::views::ViewDocument,
	snapshot: Option<&code_moniker_workspace::snapshot::WorkspaceSnapshot>,
	selectors: &[String],
) {
	if selectors.is_empty() {
		return;
	}
	let Some(snapshot) = snapshot else {
		panel_danger(vm, "  evidence unavailable: workspace index is not ready");
		return;
	};
	let resolution = crate::views::resolve_symbols(
		snapshot,
		&view.scope_path,
		selectors,
		crate::views::RenderOptions {
			moniker_display: crate::views::MonikerDisplay::None,
			context_lines: 2,
			include_code: true,
		},
	);
	push_symbol_resolution(vm, resolution);
}

fn push_symbol_resolution(vm: &mut PanelVm, resolution: crate::views::SymbolResolution) {
	for evidence in resolution.evidence {
		panel_evidence(
			vm,
			evidence.label.clone(),
			evidence.selector.clone(),
			evidence.file.clone(),
			evidence.slice,
		);
		if !evidence.code.is_empty() {
			panel_source_snippet(
				vm,
				source_lines_vm(evidence.code.iter().map(|(number, text)| {
					let active = evidence
						.active_slice
						.is_some_and(|(start, end)| start <= *number && *number <= end);
					(*number as u32, text.clone(), active)
				})),
			);
		}
	}
	for missing in resolution.missing {
		panel_danger(vm, format!("  missing selector {}", missing.selector));
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
	push_selected_notes(&mut vm, app, &symbol.identity);
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

fn push_selected_notes(vm: &mut PanelVm, app: &App, moniker: &str) {
	if let Some(error) = notes_error(app) {
		panel_section(vm, "notes");
		panel_danger(vm, "notes unavailable");
		panel_muted(vm, error.to_string());
		panel_blank(vm);
		return;
	}
	let notes = notes_for_moniker(app, moniker);
	if notes.is_empty() {
		return;
	}
	panel_section(vm, "notes");
	for note in notes {
		panel_bullet(
			vm,
			format!(
				"{} {}  {}",
				note.status.as_str(),
				note.kind.as_str(),
				note.title
			),
		);
		for line in note.body.lines().take(6) {
			panel_muted(vm, format!("  {line}"));
		}
	}
	panel_blank(vm);
}

fn notes_for_moniker(app: &App, moniker: &str) -> Vec<code_moniker_workspace::notes::Note> {
	let notes = crate::ui::app::notes(app);
	let mut notes = notes
		.notes()
		.iter()
		.filter(|note| note.moniker == moniker)
		.cloned()
		.collect::<Vec<_>>();
	notes.sort_by(|left, right| {
		left.status
			.cmp(&right.status)
			.then_with(|| left.updated_at.cmp(&right.updated_at).reverse())
			.then_with(|| left.id.cmp(&right.id))
	});
	notes
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
	if let Some(target) = crate::ui::nav_notes::nav_row_note_target(
		crate::ui::app::store(app),
		row,
		&app.config.scheme,
	) {
		push_selected_notes(&mut vm, app, &target.moniker);
	}
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
	source_lines_vm(
		snippet
			.into_iter()
			.map(|line| (line.number, line.text, line.active)),
	)
}

fn source_lines_vm(lines: impl IntoIterator<Item = (u32, String, bool)>) -> Vec<SourceLineVm> {
	let lines = lines.into_iter().collect::<Vec<_>>();
	let width = lines
		.iter()
		.map(|(number, _, _)| number.to_string().len())
		.max()
		.unwrap_or(4)
		.max(4);
	lines
		.into_iter()
		.map(|(number, text, active)| SourceLineVm {
			number,
			number_width: width,
			text,
			active,
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
				live_refresh: crate::args::LiveRefresh::Auto,
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
