use code_moniker_core::lang::Lang;

use crate::ui::app::{
	FocusRegion, HeaderSearchState, PanelNavigationState, VisualizationMode, display_filter,
	is_filtered, kind_filter_summary, lang_filter_summary, scope_label,
};
use crate::ui::events::{HeaderSearchFocus, UiMode};
use crate::ui::panel::PanelVm;
use crate::ui::render::component::ComponentId;
use crate::ui::store::navigation::{
	NavigationPaneView, NavigationState, navigation_explorer_def_count, navigation_primary_view,
	navigation_usage_view, navigation_visible_defs,
};
use crate::ui::store::navigation_tree::{NavNodeKind, NavRow};
use crate::ui::workspace_read::{self, LocalWorkspaceFacade, UsageFocus};
use code_moniker_workspace::snapshot::{ChangeStatus, SymbolId};
type DefLocation = SymbolId;

use crate::ui::app::App;

#[derive(Clone, Debug)]
pub(in crate::ui) struct ExplorerVm {
	pub(in crate::ui) header: HeaderVm,
	pub(in crate::ui) search: SearchBarVm,
	pub(in crate::ui) primary_nav: NavPaneVm,
	pub(in crate::ui) usage_nav: Option<NavPaneVm>,
	pub(in crate::ui) panel: PanelVm,
	pub(in crate::ui) panel_navigation: PanelNavigationState,
	pub(in crate::ui) panel_focused: bool,
	pub(in crate::ui) footer: FooterVm,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct HeaderVm {
	pub(in crate::ui) mode: &'static str,
	pub(in crate::ui) scope: String,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct SearchBarVm {
	pub(in crate::ui) focused: bool,
	pub(in crate::ui) focus: Option<HeaderSearchFocus>,
	pub(in crate::ui) text: String,
	pub(in crate::ui) display_text: String,
	pub(in crate::ui) lang_summary: String,
	pub(in crate::ui) kind_summary: String,
	pub(in crate::ui) combo_open: bool,
	pub(in crate::ui) popup: Option<SearchPopupVm>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct SearchPopupVm {
	pub(in crate::ui) focus: HeaderSearchFocus,
	pub(in crate::ui) title: &'static str,
	pub(in crate::ui) items: Vec<String>,
	pub(in crate::ui) cursor: usize,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NavPaneVm {
	pub(in crate::ui) title: String,
	pub(in crate::ui) component: ComponentId,
	pub(in crate::ui) rows: Vec<NavRowVm>,
	pub(in crate::ui) selection: usize,
	pub(in crate::ui) focused: bool,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NavRowVm {
	pub(in crate::ui) label: String,
	pub(in crate::ui) depth: usize,
	pub(in crate::ui) has_children: bool,
	pub(in crate::ui) expanded: bool,
	pub(in crate::ui) file_count: usize,
	pub(in crate::ui) def_count: usize,
	pub(in crate::ui) kind: NavRowVmKind,
}

#[derive(Clone, Debug)]
pub(in crate::ui) enum NavRowVmKind {
	Root,
	Lang,
	Dir,
	File {
		change_count: Option<usize>,
	},
	ChangeFile,
	Def {
		lang: Lang,
		kind: String,
		change: Option<NavChangeVm>,
	},
	Change(Option<NavChangeVm>),
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NavChangeVm {
	pub(in crate::ui) lang: Lang,
	pub(in crate::ui) kind: String,
	pub(in crate::ui) status: ChangeStatus,
	pub(in crate::ui) usage_count: usize,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct FooterVm {
	pub(in crate::ui) prefix: &'static str,
	pub(in crate::ui) status: String,
}

pub(in crate::ui) struct ExplorerVmContext<'a> {
	mode: UiMode,
	view_mode: VisualizationMode,
	scope: String,
	header_search: &'a HeaderSearchState,
	navigation: &'a NavigationState,
	panel: PanelVm,
	panel_navigation: &'a PanelNavigationState,
	focus_region: FocusRegion,
	usage_lens: Option<&'a UsageFocus>,
	filtered: bool,
	workspace: &'a LocalWorkspaceFacade,
	status: &'a str,
}

impl ExplorerVm {
	pub(in crate::ui) fn from_app(app: &App) -> Self {
		let ctx = ExplorerVmContext {
			mode: crate::ui::app::mode(app),
			view_mode: crate::ui::app::view_mode(app),
			scope: scope_label(app),
			header_search: crate::ui::app::header_search(app),
			navigation: crate::ui::app::navigation(app),
			panel: super::active_panel(app),
			panel_navigation: &app.app_store.shell().panel_navigation,
			focus_region: crate::ui::app::focus_region(app),
			usage_lens: crate::ui::app::usage_lens(app),
			filtered: is_filtered(app),
			workspace: crate::ui::app::store(app),
			status: crate::ui::app::status(app),
		};
		Self::from_context(ctx)
	}

	pub(in crate::ui) fn from_context(ctx: ExplorerVmContext<'_>) -> Self {
		Self {
			header: HeaderVm {
				mode: ctx.view_mode.label(),
				scope: ctx.scope.clone(),
			},
			search: search_vm(&ctx),
			primary_nav: primary_nav_vm(&ctx),
			usage_nav: usage_nav_vm(&ctx),
			panel: ctx.panel,
			panel_navigation: ctx.panel_navigation.clone(),
			panel_focused: focus_region_visible(ctx.mode, ctx.focus_region, FocusRegion::Panel),
			footer: FooterVm {
				prefix: footer_prefix(ctx.mode),
				status: ctx.status.to_string(),
			},
		}
	}
}

pub(in crate::ui) fn focus_region_visible(
	mode: UiMode,
	current: FocusRegion,
	region: FocusRegion,
) -> bool {
	matches!(mode, UiMode::Normal) && current == region
}

fn search_vm(ctx: &ExplorerVmContext<'_>) -> SearchBarVm {
	let search = ctx.header_search;
	let focus = match ctx.mode {
		UiMode::HeaderSearch(focus) => Some(focus),
		UiMode::Normal => None,
	};
	SearchBarVm {
		focused: focus.is_some(),
		focus,
		text: search.text.clone(),
		display_text: display_filter(search.text.trim()).to_string(),
		lang_summary: lang_filter_summary(&search.langs),
		kind_summary: kind_filter_summary(&search.kind_filters),
		combo_open: search.combo_open,
		popup: search_popup_vm(ctx, focus),
	}
}

fn search_popup_vm(
	ctx: &ExplorerVmContext<'_>,
	focus: Option<HeaderSearchFocus>,
) -> Option<SearchPopupVm> {
	if !ctx.header_search.combo_open {
		return None;
	}
	let search = ctx.header_search;
	match focus {
		Some(HeaderSearchFocus::Lang) => {
			let options = search.available_langs.clone();
			let mut items = vec![if search.langs.is_empty() {
				"[x] all languages".to_string()
			} else {
				"clear language filter".to_string()
			}];
			for lang in &options {
				let mark = if search.langs.contains(lang) {
					"[x]"
				} else {
					"[ ]"
				};
				items.push(format!("{mark} {}", lang.tag()));
			}
			Some(SearchPopupVm {
				focus: HeaderSearchFocus::Lang,
				title: "lang selector",
				items,
				cursor: search.lang_cursor,
			})
		}
		Some(HeaderSearchFocus::Kind) => {
			let options = search.available_kind_filters.clone();
			let mut items = vec![if search.kind_filters.is_empty() {
				"[x] all kinds".to_string()
			} else {
				"clear kind filter".to_string()
			}];
			for option in &options {
				let mark = if search.kind_filters.contains(option) {
					"[x]"
				} else {
					"[ ]"
				};
				items.push(format!("{mark} {}", option.label()));
			}
			Some(SearchPopupVm {
				focus: HeaderSearchFocus::Kind,
				title: "kind selector",
				items,
				cursor: search.kind_cursor,
			})
		}
		_ => None,
	}
}

fn primary_nav_vm(ctx: &ExplorerVmContext<'_>) -> NavPaneVm {
	let navigation = ctx.navigation;
	let visible_defs = navigation_visible_defs(navigation);
	let pane = navigation_primary_view(navigation);
	let title = if ctx.filtered {
		if ctx.view_mode == VisualizationMode::Change {
			format!(
				" change {} files {} defs ",
				matched_file_count(visible_defs),
				visible_defs.len()
			)
		} else {
			format!(
				" filtered {} files {} defs ",
				matched_file_count(visible_defs),
				visible_defs.len()
			)
		}
	} else {
		format!(
			" navigator {} files {} defs ",
			workspace_read::stats(ctx.workspace).files,
			navigation_explorer_def_count(ctx.navigation)
		)
	};
	NavPaneVm {
		title,
		component: ComponentId::Navigator,
		rows: nav_rows_vm(ctx, pane),
		selection: pane.selection,
		focused: focus_region_visible(ctx.mode, ctx.focus_region, FocusRegion::Navigator),
	}
}

fn usage_nav_vm(ctx: &ExplorerVmContext<'_>) -> Option<NavPaneVm> {
	let focus = ctx.usage_lens?;
	let pane = navigation_usage_view(ctx.navigation)?;
	Some(NavPaneVm {
		title: format!(
			" usages {}  {} files {} defs ",
			focus.label,
			matched_file_count(&focus.contexts),
			focus.contexts.len()
		),
		component: ComponentId::NavigatorUsages,
		rows: nav_rows_vm(ctx, pane),
		selection: pane.selection,
		focused: focus_region_visible(ctx.mode, ctx.focus_region, FocusRegion::UsageLens),
	})
}

fn nav_rows_vm(ctx: &ExplorerVmContext<'_>, pane: NavigationPaneView<'_>) -> Vec<NavRowVm> {
	pane.rows
		.iter()
		.map(|row| nav_row_vm(ctx, row, pane))
		.collect()
}

fn nav_row_vm(ctx: &ExplorerVmContext<'_>, row: &NavRow, pane: NavigationPaneView<'_>) -> NavRowVm {
	NavRowVm {
		label: row.label.clone(),
		depth: row.depth,
		has_children: row.has_children,
		expanded: pane.expanded.contains(&row.key),
		file_count: row.file_count,
		def_count: row.def_count,
		kind: nav_row_kind_vm(ctx, row),
	}
}

fn nav_row_kind_vm(ctx: &ExplorerVmContext<'_>, row: &NavRow) -> NavRowVmKind {
	match &row.kind {
		NavNodeKind::Root => NavRowVmKind::Root,
		NavNodeKind::Lang => NavRowVmKind::Lang,
		NavNodeKind::Dir => NavRowVmKind::Dir,
		NavNodeKind::File(file_idx) => NavRowVmKind::File {
			change_count: file_change_count(ctx.workspace, *file_idx),
		},
		NavNodeKind::ChangeFile => NavRowVmKind::ChangeFile,
		NavNodeKind::Def(loc) => {
			let symbol = workspace_read::symbol_summary(ctx.workspace, loc);
			let kind = symbol.kind.clone();
			NavRowVmKind::Def {
				lang: symbol.lang,
				kind: kind.clone(),
				change: symbol.change.map(|change| NavChangeVm {
					lang: symbol.lang,
					kind,
					status: change.status,
					usage_count: change.usage_count,
				}),
			}
		}
		NavNodeKind::Change(id) => NavRowVmKind::Change(
			workspace_read::change_summary(ctx.workspace, id.clone()).map(|change| NavChangeVm {
				lang: change.lang,
				kind: change.kind,
				status: change.status,
				usage_count: change.usage_count,
			}),
		),
	}
}

fn file_change_count(workspace: &LocalWorkspaceFacade, file_idx: usize) -> Option<usize> {
	let count = workspace_read::change_count_for_file(workspace, file_idx);
	(count > 0).then_some(count)
}

fn footer_prefix(mode: UiMode) -> &'static str {
	match mode {
		UiMode::HeaderSearch(HeaderSearchFocus::Text) => "search",
		UiMode::HeaderSearch(HeaderSearchFocus::Lang) => "lang",
		UiMode::HeaderSearch(HeaderSearchFocus::Kind) => "kind",
		UiMode::Normal => "status",
	}
}

fn matched_file_count(defs: &[DefLocation]) -> usize {
	defs.len()
}
