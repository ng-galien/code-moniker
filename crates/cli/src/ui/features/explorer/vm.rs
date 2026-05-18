use code_moniker_core::lang::Lang;

use crate::ui::app::{FocusRegion, PanelNavigationState, VisualizationMode};
use crate::ui::component::ComponentId;
use crate::ui::events::{HeaderSearchFocus, UiMode};
use crate::ui::navigator::{NavNodeKind, NavRow};
use crate::ui::panels::PanelVm;
use crate::ui::store::navigation::NavigationPaneView;
use crate::workspace::{ChangeStatus, DefLocation, IndexStore};

use super::ExplorerFeature;
use crate::ui::{App, display_filter, kind_filter_summary, lang_filter_summary};

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

impl ExplorerVm {
	pub(in crate::ui) fn from_app(app: &App) -> Self {
		let mode = app.mode();
		let view_mode = app.view_mode();
		let scope = app.scope_label();
		Self {
			header: HeaderVm {
				mode: view_mode.label(),
				scope: scope.clone(),
			},
			search: search_vm(app),
			primary_nav: primary_nav_vm(app),
			usage_nav: usage_nav_vm(app),
			panel: ExplorerFeature::active_panel(app),
			panel_navigation: app.panel_navigation().clone(),
			panel_focused: focus_region_visible(mode, app.focus_region(), FocusRegion::Panel),
			footer: FooterVm {
				prefix: footer_prefix(mode),
				status: app.status().to_string(),
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

fn search_vm(app: &App) -> SearchBarVm {
	let search = app.header_search();
	let focus = match app.mode() {
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
		popup: search_popup_vm(app, focus),
	}
}

fn search_popup_vm(app: &App, focus: Option<HeaderSearchFocus>) -> Option<SearchPopupVm> {
	if !app.header_search().combo_open {
		return None;
	}
	let search = app.header_search();
	match focus {
		Some(HeaderSearchFocus::Lang) => {
			let options = app.available_header_langs();
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
			let options = app.available_header_kind_filters();
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

fn primary_nav_vm(app: &App) -> NavPaneVm {
	let navigation = app.app_store.navigation();
	let visible_defs = navigation.visible_defs();
	let pane = navigation.primary_view();
	let title = if app.is_filtered() {
		if app.view_mode() == VisualizationMode::Change {
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
			app.store().stats().files,
			app.app_store.navigation().explorer_def_count()
		)
	};
	NavPaneVm {
		title,
		component: ComponentId::Navigator,
		rows: nav_rows_vm(app, pane),
		selection: pane.selection,
		focused: focus_region_visible(app.mode(), app.focus_region(), FocusRegion::Navigator),
	}
}

fn usage_nav_vm(app: &App) -> Option<NavPaneVm> {
	let focus = app.usage_lens()?;
	let pane = app.app_store.navigation().usage_view()?;
	Some(NavPaneVm {
		title: format!(
			" usages {}  {} files {} defs ",
			focus.label,
			matched_file_count(&focus.contexts),
			focus.contexts.len()
		),
		component: ComponentId::NavigatorUsages,
		rows: nav_rows_vm(app, pane),
		selection: pane.selection,
		focused: focus_region_visible(app.mode(), app.focus_region(), FocusRegion::UsageLens),
	})
}

fn nav_rows_vm(app: &App, pane: NavigationPaneView<'_>) -> Vec<NavRowVm> {
	pane.rows
		.iter()
		.map(|row| nav_row_vm(app, row, pane))
		.collect()
}

fn nav_row_vm(app: &App, row: &NavRow, pane: NavigationPaneView<'_>) -> NavRowVm {
	NavRowVm {
		label: row.label.clone(),
		depth: row.depth,
		has_children: row.has_children,
		expanded: pane.expanded.contains(&row.key),
		file_count: row.file_count,
		def_count: row.def_count,
		kind: nav_row_kind_vm(app, row),
	}
}

fn nav_row_kind_vm(app: &App, row: &NavRow) -> NavRowVmKind {
	match row.kind {
		NavNodeKind::Root => NavRowVmKind::Root,
		NavNodeKind::Lang => NavRowVmKind::Lang,
		NavNodeKind::Dir => NavRowVmKind::Dir,
		NavNodeKind::File(file_idx) => NavRowVmKind::File {
			change_count: file_change_count(app, file_idx),
		},
		NavNodeKind::ChangeFile => NavRowVmKind::ChangeFile,
		NavNodeKind::Def(loc) => {
			let symbol = app.store().symbol_summary(&loc);
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
		NavNodeKind::Change(id) => {
			NavRowVmKind::Change(app.store().change_summary(id).map(|change| NavChangeVm {
				lang: change.lang,
				kind: change.kind,
				status: change.status,
				usage_count: change.usage_count,
			}))
		}
	}
}

fn file_change_count(app: &App, file_idx: usize) -> Option<usize> {
	let count = app.store().change_count_for_file(file_idx);
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
	defs.iter()
		.map(|loc| loc.file)
		.collect::<std::collections::BTreeSet<_>>()
		.len()
}
