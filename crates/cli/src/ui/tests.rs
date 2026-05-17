use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::process::Command;

use code_moniker_core::lang::Lang;

use crate::DEFAULT_SCHEME;
use crate::workspace::SessionOptions;

use super::events::FilterEdit;
use super::events::Msg;
use super::kinds::{KindGroup, definition_kind_group, definition_kind_order, reference_kind_group};
use super::panels::source_snippet_lines;
use super::source::source_snippet;
use super::theme::THEME;
use super::*;
use crate::workspace::IndexStore;

fn key(code: KeyCode) -> KeyEvent {
	KeyEvent::new(code, KeyModifiers::empty())
}

fn write(root: &std::path::Path, rel: &str, body: &str) {
	let p = root.join(rel);
	if let Some(parent) = p.parent() {
		std::fs::create_dir_all(parent).unwrap();
	}
	std::fs::write(p, body).unwrap();
}

fn git(root: &std::path::Path, args: &[&str]) {
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

fn line_text(line: &Line<'_>) -> String {
	line.spans
		.iter()
		.map(|span| span.content.as_ref())
		.collect::<String>()
}

fn symbol_name(app: &App, loc: &DefLocation) -> String {
	app.store().symbol_summary(loc).name
}

fn symbol_kind(app: &App, loc: &DefLocation) -> String {
	app.store().symbol_summary(loc).kind
}

fn find_symbol(app: &App, kind: &str, name: &str) -> DefLocation {
	app.store()
		.all_navigable_defs()
		.into_iter()
		.find(|loc| {
			let symbol = app.store().symbol_summary(loc);
			symbol.kind == kind && symbol.name == name
		})
		.unwrap_or_else(|| panic!("missing symbol {kind} {name}"))
}

fn select_nav_label(app: &mut App, label: &str) {
	let idx = app
		.nav_rows()
		.iter()
		.position(|row| row.label == label)
		.unwrap_or_else(|| panic!("missing navigator row {label}: {:?}", app.nav_rows()));
	select_nav_index(app, idx);
}

fn select_nav_label_ending_with(app: &mut App, suffix: &str) {
	let idx = app
		.nav_rows()
		.iter()
		.position(|row| row.label.ends_with(suffix))
		.unwrap_or_else(|| {
			panic!(
				"missing navigator row ending with {suffix}: {:?}",
				app.nav_rows()
			)
		});
	select_nav_index(app, idx);
}

fn select_nav_index(app: &mut App, idx: usize) {
	while app.selected_nav_index() < idx {
		app.dispatch_navigation(NavigationAction::MoveDown);
	}
	while app.selected_nav_index() > idx {
		app.dispatch_navigation(NavigationAction::MoveUp);
	}
}

fn apply_text_filter(app: &mut App, raw: &str) {
	app.update(AppAction::Ui(Msg::HeaderSearchReset));
	for ch in raw.chars() {
		app.update(AppAction::Ui(Msg::HeaderSearchInput(FilterEdit::Push(ch))));
	}
	app.apply_header_search(None, true);
}

fn apply_kind_filter(app: &mut App, text: &str, lang: Option<Lang>, kind: &str) {
	app.update(AppAction::Ui(Msg::HeaderSearchReset));
	for ch in text.chars() {
		app.update(AppAction::Ui(Msg::HeaderSearchInput(FilterEdit::Push(ch))));
	}
	app.dispatch_shell(ShellAction::SetHeaderSearchFilters {
		lang,
		kind: Some(kind.to_string()),
	});
	app.apply_header_search(None, true);
}

#[test]
fn component_titles_include_stable_collaboration_markers() {
	let title = block_title("navigator", ComponentId::Navigator);
	let rendered = line_text(&title);

	assert_eq!(rendered, "navigator [ui.navigator]");
	assert_eq!(ComponentId::SearchInput.as_str(), "ui.search.input");
	assert_eq!(ComponentId::PanelRefs.as_str(), "ui.panel.refs");
	assert_eq!(ComponentId::PanelChange.as_str(), "ui.panel.change");
	assert_eq!(ComponentId::SourceSnippet.as_str(), "ui.source.snippet");
}

#[test]
fn feature_registry_exposes_static_explorer_contracts() {
	let registry = FeatureRegistry::static_registry();
	let navigation = registry.navigation();
	let commands = registry.commands();

	assert_eq!(registry.initial_route(), ExplorerFeature::initial_route());
	assert!(registry.can_open(&ExplorerFeature::route(ROUTE_REFS)));
	assert!(!registry.can_open(&Route::new("missing", "index")));
	assert_eq!(
		navigation
			.iter()
			.map(|item| item.label.as_str())
			.collect::<Vec<_>>(),
		vec!["Overview", "Outline", "Refs", "Check", "Change"]
	);
	assert!(
		commands
			.iter()
			.any(|command| command.label == "Focus header search"
				&& command.shortcut.as_deref() == Some("s")),
		"{commands:?}"
	);
	assert!(
		commands
			.iter()
			.any(|command| command.label == "Show changes"
				&& command.shortcut.as_deref() == Some("d")),
		"{commands:?}"
	);
	assert!(
		commands
			.iter()
			.any(|command| command.label == "Copy panel snapshot"
				&& command.shortcut.as_deref() == Some("y")),
		"{commands:?}"
	);
}

#[test]
fn y_key_copies_panel_only_in_normal_mode() {
	assert!(matches!(
		key_to_msg(UiMode::Normal, key(KeyCode::Char('y'))),
		Msg::CopyPanelSnapshot
	));
	assert!(matches!(
		key_to_msg(
			UiMode::HeaderSearch(HeaderSearchFocus::Text),
			key(KeyCode::Char('y'))
		),
		Msg::HeaderSearchInput(FilterEdit::Push('y'))
	));
	assert!(matches!(
		key_to_msg(
			UiMode::HeaderSearch(HeaderSearchFocus::Lang),
			key(KeyCode::Char('y'))
		),
		Msg::Noop
	));
}

#[test]
fn panel_display_helpers_render_tables_and_fitted_details() {
	let columns = [
		text::Column::left("lang", 6),
		text::Column::right("defs", 5),
	];

	assert_eq!(
		line_text(&panel::table_header(&columns, 13)),
		"lang     defs"
	);
	assert_eq!(
		line_text(&panel::table_row(
			&columns,
			&["java".to_string(), "1234".to_string()],
			13
		)),
		"java     1234"
	);
	let narrow = line_text(&panel::table_row(
		&columns,
		&["typescript".to_string(), "123456".to_string()],
		8,
	));
	assert!(text::visible_len(&narrow) <= 8, "{narrow}");
	assert_eq!(
		line_text(&panel::danger_section("invalid filter")),
		"invalid filter"
	);
	assert_eq!(
		line_text(&panel::kv(
			"moniker",
			"common-lib/lang:java/package:acme",
			24,
			text::FitMode::Middle
		)),
		"moniker   commo...e:acme"
	);
}

#[test]
fn kind_palette_groups_known_kinds_and_keeps_fallback() {
	assert_eq!(
		THEME.kind.color_for_group(KindGroup::Callable),
		THEME.kind.callable,
		"callables should share a kind palette color"
	);
	assert_eq!(
		THEME.kind.color_for_group(KindGroup::Type),
		THEME.kind.type_like
	);
	assert_eq!(
		THEME.kind.color_for_group(KindGroup::Value),
		THEME.kind.value
	);
	assert_eq!(
		THEME.kind.color_for_group(KindGroup::Reference),
		THEME.kind.reference
	);
	assert_eq!(
		THEME.kind.color_for_group(KindGroup::Unknown),
		THEME.kind.fallback
	);
}

#[test]
fn ui_kind_groups_come_from_language_contracts() {
	assert_eq!(
		definition_kind_group(Lang::Java, "interface"),
		KindGroup::Type
	);
	assert_eq!(
		definition_kind_group(Lang::Cs, "property"),
		KindGroup::Value
	);
	assert_eq!(
		definition_kind_group(Lang::Sql, "schema"),
		KindGroup::Namespace
	);
	assert_eq!(reference_kind_group("uses_type"), KindGroup::Reference);
	assert!(
		definition_kind_order(Lang::Java, "class") < definition_kind_order(Lang::Java, "method")
	);
	assert!(
		definition_kind_order(Lang::Java, "field") < definition_kind_order(Lang::Java, "method")
	);
}

#[test]
fn header_exposes_visualization_mode_and_scope_only() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert_eq!(app.view_mode(), VisualizationMode::Explorer);
	assert_eq!(app.view(), View::Overview);
	let initial = line_text(&header_line(&app, 120));
	assert_eq!(
		initial,
		"code-moniker [ui.header] mode explorer  [ui.search.input] search [<all>] lang [all] kind [all] scope all"
	);

	apply_text_filter(&mut app, "Alpha");

	assert_eq!(app.view_mode(), VisualizationMode::Search);
	assert_eq!(app.panel_policy(), PanelPolicy::Contextual);
	assert_eq!(app.view(), View::Tree);
	let filtered = line_text(&header_line(&app, 120));
	assert!(filtered.contains("mode search"), "{filtered}");
	assert!(filtered.contains("search [Alpha]"), "{filtered}");
	assert!(filtered.contains("scope search:Alpha"), "{filtered}");
	assert!(!filtered.contains("panel"), "{filtered}");
	assert!(!filtered.contains("files"), "{filtered}");
	assert!(!filtered.contains("defs"), "{filtered}");
	assert!(!filtered.contains("refs"), "{filtered}");
	assert!(!filtered.contains("filter"), "{filtered}");
}

#[test]
fn view_switches_update_shell_route_through_effects() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert_eq!(app.route().clone(), ExplorerFeature::route(ROUTE_OVERVIEW));
	app.handle_key(key(KeyCode::Char('3'))).unwrap();

	assert_eq!(app.view(), View::Refs);
	assert_eq!(app.route().clone(), ExplorerFeature::route(ROUTE_REFS));
}

#[test]
fn nested_shell_dispatches_apply_emitted_effects() {
	let tmp = tempfile::tempdir().unwrap();
	let mut app = App::new(
		WorkspaceStore::empty(SessionOptions {
			paths: vec![tmp.path().into()],
			project: Some("app".into()),
			cache_dir: None,
		}),
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.dispatch_shell(ShellAction::EmitNotify("nested effect applied".to_string()));

	assert_eq!(app.status(), "nested effect applied");
}

#[test]
fn contextual_panel_tracks_selected_declaration_in_explorer_mode() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert_eq!(app.view_mode(), VisualizationMode::Explorer);
	assert_eq!(app.panel_policy(), PanelPolicy::Contextual);
	assert_eq!(app.view(), View::Overview);

	app.handle_key(key(KeyCode::Enter)).unwrap();
	app.handle_key(key(KeyCode::Down)).unwrap();
	assert!(app.selected().is_none());
	assert_eq!(app.view(), View::Overview);

	app.handle_key(key(KeyCode::Enter)).unwrap();
	app.handle_key(key(KeyCode::Down)).unwrap();

	assert_eq!(
		symbol_name(&app, &app.selected().expect("selected declaration")),
		"Alpha"
	);
	assert_eq!(app.view(), View::Tree);
	assert_eq!(app.route().clone(), ExplorerFeature::route(ROUTE_OUTLINE));
}

#[test]
fn app_filter_limits_visible_declarations_and_keeps_tree_navigation() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	apply_text_filter(&mut app, "Alpha");
	assert!(
		app.visible_defs()
			.iter()
			.all(|loc| symbol_name(&app, loc).contains("Alpha")),
		"{:?}",
		app.visible_defs()
	);
	assert!(!app.visible_defs().is_empty());
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Alpha"),
		"{:?}",
		app.nav_rows()
	);
	assert!(!app.nav_rows().iter().any(|row| row.label.contains("Beta")));
	select_nav_label_ending_with(&mut app, "Alpha");
	assert_eq!(
		symbol_name(&app, &app.selected().expect("selected Alpha")),
		"Alpha"
	);
}

#[test]
fn search_mode_ranks_symbol_hits_and_feeds_contextual_navigator() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/z/core.ts",
		"class CustomerProfile {}\nclass OrderFlow {}\n",
	);
	write(
		tmp.path(),
		"src/a/customer/billing.ts",
		"class BillingService {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let total = app.visible_defs().len();
	let hits = app
		.store()
		.search_symbols_filtered("customer", 10, None, None);
	let hit_names: Vec<_> = hits.iter().map(|hit| symbol_name(&app, &hit.loc)).collect();

	assert_eq!(
		hit_names.first().map(String::as_str),
		Some("CustomerProfile")
	);
	assert!(
		hit_names.iter().any(|name| name == "BillingService"),
		"{hit_names:?}"
	);

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	for c in "customer".chars() {
		app.handle_key(key(KeyCode::Char(c))).unwrap();
	}

	assert_eq!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	assert_eq!(app.header_search().text, "customer");
	assert_eq!(app.visible_defs().len(), total);

	app.handle_key(key(KeyCode::Enter)).unwrap();

	assert_eq!(app.mode(), UiMode::Normal);
	assert_eq!(app.view_mode(), VisualizationMode::Search);
	assert!(app.is_filtered());
	assert!(matches!(app.active_filter(), ActiveFilter::HeaderSearch(_)));
	assert_eq!(symbol_name(&app, &app.visible_defs()[0]), "CustomerProfile");
	assert_eq!(
		symbol_name(
			&app,
			&app.selected().expect("top ranked search hit is selected")
		),
		"CustomerProfile"
	);
	assert!(
		!app.visible_defs()
			.iter()
			.any(|loc| symbol_name(&app, loc) == "OrderFlow"),
		"{:?}",
		app.visible_defs()
	);
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label.contains("CustomerProfile")),
		"{:?}",
		app.nav_rows()
	);
	let header = line_text(&header_line(&app, 120));
	assert!(header.contains("mode search"), "{header}");
	assert!(header.contains("search [customer]"), "{header}");
	assert!(header.contains("scope search:customer"), "{header}");
	assert!(app.status().contains("search:customer"), "{}", app.status());
}

#[test]
fn header_search_is_always_visible_and_keeps_navigator_space() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class CustomerProfile {}\nclass OrderFlow {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert!(!search_input_visible(&app));
	let initial_header = line_text(&header_line(&app, 120));
	assert!(
		initial_header.contains("[ui.search.input]"),
		"{initial_header}"
	);
	assert!(
		initial_header.contains("search [<all>]"),
		"{initial_header}"
	);

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	for c in "customer".chars() {
		app.handle_key(key(KeyCode::Char(c))).unwrap();
	}

	assert_eq!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	assert!(search_input_visible(&app));
	assert_eq!(search_input_title(&app), "search text focused");
	assert_eq!(search_input_value(&app), "customer");
	let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
	terminal
		.draw(|frame| render_shell(frame, frame.area(), &mut app))
		.unwrap();
	let screen = format!("{}", terminal.backend());

	assert!(screen.contains("[ui.search.input]"), "{screen}");
	assert!(screen.contains("search ["), "{screen}");
	assert!(
		screen.find("[ui.search.input]")
			< screen.find("navigator").or_else(|| screen.find("filtered")),
		"header search should render before the navigator: {screen}"
	);

	app.handle_key(key(KeyCode::Enter)).unwrap();

	assert_eq!(app.mode(), UiMode::Normal);
	assert!(search_input_visible(&app));
	assert_eq!(search_input_title(&app), "search");
	assert_eq!(search_input_value(&app), "customer");

	app.handle_key(key(KeyCode::Char('x'))).unwrap();
	app.apply_header_search(None, true);

	assert!(!search_input_visible(&app));
}

#[test]
fn header_values_are_fitted_before_scope_is_rendered() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.update(AppAction::Ui(Msg::ToggleHeaderSearch));
	for ch in "very-long-symbol-query-that-would-overflow-the-header".chars() {
		app.update(AppAction::Ui(Msg::HeaderSearchInput(FilterEdit::Push(ch))));
	}
	app.dispatch_shell(ShellAction::SetHeaderSearchFilters {
		lang: Some(Lang::Ts),
		kind: Some("very_long_kind_name".to_string()),
	});
	app.apply_header_search(None, true);

	let header = line_text(&header_line(&app, 100));
	assert!(text::visible_len(&header) <= 100, "{header}");
	assert!(header.contains("scope "), "{header}");
}

#[test]
fn header_search_applies_structured_filters_before_result_limit() {
	let tmp = tempfile::tempdir().unwrap();
	let java = (0..510)
		.map(|idx| format!("class A{idx:03}Resolver {{}}\n"))
		.collect::<String>();
	write(tmp.path(), "src/main/java/com/acme/Resolvers.java", &java);
	write(tmp.path(), "src/ts/target.ts", "class ZResolver {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_kind_filter(&mut app, "Resolver", Some(Lang::Ts), "class");

	let names = app
		.visible_defs()
		.iter()
		.map(|loc| symbol_name(&app, loc))
		.collect::<Vec<_>>();
	assert_eq!(names, vec!["ZResolver"]);
}

#[test]
fn panel_snapshot_text_names_active_component_and_body() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	let overview = active_panel_snapshot(&app).to_text(app.view_mode().label(), &app.scope_label());
	assert!(
		overview.contains("component ui.panel.overview"),
		"{overview}"
	);
	assert!(overview.contains("mode      explorer"), "{overview}");
	assert!(overview.contains("summary"), "{overview}");
	assert!(overview.contains("files     1"), "{overview}");
	assert!(
		overview.contains("lang          files      defs      refs"),
		"{overview}"
	);
	assert!(overview.contains("shape            count"), "{overview}");

	apply_text_filter(&mut app, "Alpha");
	select_nav_label_ending_with(&mut app, "Alpha");
	app.sync_contextual_view();

	let outline = active_panel_snapshot(&app).to_text(app.view_mode().label(), &app.scope_label());
	assert!(outline.contains("component ui.panel.outline"), "{outline}");
	assert!(outline.contains("kind      class"), "{outline}");
	assert!(outline.contains("name      Alpha"), "{outline}");
}

#[test]
fn clipboard_result_updates_user_visible_status() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_clipboard_result(clipboard::ClipboardResult {
		component: "ui.panel.refs".to_string(),
		result: Ok(()),
	});
	assert_eq!(app.status(), "copied ui.panel.refs snapshot to clipboard");

	app.handle_clipboard_result(clipboard::ClipboardResult {
		component: "ui.panel.refs".to_string(),
		result: Err("missing clipboard command".to_string()),
	});
	assert_eq!(
		app.status(),
		"clipboard copy failed for ui.panel.refs: missing clipboard command"
	);
}

#[test]
fn spawn_effect_runs_task_through_shell_event_channel() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let (tx, rx) = std::sync::mpsc::channel();
	app.set_event_sender(tx);

	app.apply_effect(Effect::Spawn(runtime::TaskSpec::noop("lazy smoke")));

	let ShellEvent::TaskCompleted(result) = rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("task result")
	else {
		panic!("expected task completion");
	};
	app.update(AppAction::TaskCompleted(result));

	assert_eq!(app.status(), "lazy smoke completed: task completed");
}

#[test]
fn boot_opens_with_empty_store_then_loads_index_async() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let mut app = App::boot(
		SessionOptions {
			paths: vec![tmp.path().into()],
			project: Some("app".into()),
			cache_dir: None,
		},
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let (tx, rx) = std::sync::mpsc::channel();
	app.set_event_sender(tx);

	assert_eq!(app.store().stats().files, 0);
	app.queue_startup_load();

	assert_eq!(app.status(), "loading file tree in background");
	let ShellEvent::TaskCompleted(result) = rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("file catalog result")
	else {
		panic!("expected file catalog completion");
	};
	app.update(AppAction::TaskCompleted(result));

	assert_eq!(app.store().stats().files, 1);
	assert_eq!(app.store().stats().defs, 0);
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "ts/src" && row.file_count == 1),
		"{:?}",
		app.nav_rows()
	);
	assert_eq!(
		app.status(),
		"file tree ready; loading symbols in background"
	);

	let ShellEvent::TaskCompleted(result) = rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("startup index result")
	else {
		panic!("expected startup index completion");
	};
	app.update(AppAction::TaskCompleted(result));

	assert!(app.store().stats().defs > 0);
	assert!(app.take_watch_roots_update().is_some());
	assert_eq!(app.status(), "reload index completed");
}

#[test]
fn full_store_event_queues_async_reload_when_event_loop_is_available() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let (tx, rx) = std::sync::mpsc::channel();
	app.set_event_sender(tx);

	write(tmp.path(), "src/b.ts", "class Beta {}\n");
	app.handle_store_event(StoreEvent::FullIndex);

	assert!(
		app.status().contains("task queued: reload index"),
		"{}",
		app.status()
	);
	let ShellEvent::TaskCompleted(result) = rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("store reload result")
	else {
		panic!("expected store reload completion");
	};
	app.update(AppAction::TaskCompleted(result));

	assert!(
		app.store()
			.all_navigable_defs()
			.iter()
			.any(|loc| symbol_name(&app, loc) == "Beta"),
		"store should be replaced by async reload result"
	);
	assert_eq!(app.status(), "reload index completed");
}

#[test]
fn change_store_event_queues_async_git_overlay_refresh() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return Long.toString(cents);\n  }\n}\n",
	);
	git(tmp.path(), &["init"]);
	git(tmp.path(), &["config", "user.email", "agent@example.com"]);
	git(tmp.path(), &["config", "user.name", "Agent"]);
	git(tmp.path(), &["add", "."]);
	git(tmp.path(), &["commit", "-m", "baseline"]);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let (tx, rx) = std::sync::mpsc::channel();
	app.set_event_sender(tx);

	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return \"$\" + cents;\n  }\n}\n",
	);
	app.handle_store_event(StoreEvent::GitOverlay);

	assert!(
		app.status().contains("task queued: refresh git overlay"),
		"{}",
		app.status()
	);
	let ShellEvent::TaskCompleted(result) = rx
		.recv_timeout(std::time::Duration::from_secs(2))
		.expect("git overlay result")
	else {
		panic!("expected git overlay completion");
	};
	app.update(AppAction::TaskCompleted(result));

	assert!(
		app.store()
			.change_rows()
			.iter()
			.any(|change| change.name == "format(cents:long)"),
		"{:?}",
		app.store().change_rows()
	);
	assert_eq!(app.status(), "refresh git overlay completed");
}

#[test]
fn change_mode_reports_sources_without_git() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert!(
		app.store()
			.change_overview()
			.resources
			.iter()
			.any(|resource| !resource.available)
	);

	app.handle_key(key(KeyCode::Char('d'))).unwrap();

	assert_eq!(app.view_mode(), VisualizationMode::Change);
	assert_eq!(app.view(), View::Change);
	assert_eq!(
		line_text(&header_line(&app, 120)),
		"code-moniker [ui.header] mode change  [ui.search.input] search [<all>] lang [all] kind [all] scope HEAD..worktree"
	);
	assert!(app.nav_rows().is_empty());
	let lines = change_panel_lines(&app, 80);
	let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
	assert!(
		rendered.contains("not inside a Git repository"),
		"{rendered}"
	);
}

#[test]
fn change_mode_reports_each_non_git_source_in_multi_source_sessions() {
	let tmp = tempfile::tempdir().unwrap();
	let common = tmp.path().join("common-lib");
	let service = tmp.path().join("billing-service");
	write(&common, "src/Common.java", "class Common {}\n");
	write(&service, "src/Billing.java", "class Billing {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![common.clone(), service.clone()],
		project: None,
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('d'))).unwrap();

	let lines = change_panel_lines(&app, 100);
	let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
	assert!(rendered.contains("common-lib"), "{rendered}");
	assert!(rendered.contains("billing-service"), "{rendered}");
	assert_eq!(
		app.store()
			.change_overview()
			.resources
			.iter()
			.filter(|resource| !resource.available)
			.count(),
		2
	);
}

#[test]
fn change_mode_filters_changed_symbols_and_toggles_blast_radius() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return Long.toString(cents);\n  }\n}\n",
	);
	write(
		tmp.path(),
		"src/main/java/com/acme/BillingApplication.java",
		"package com.acme;\npublic class BillingApplication {\n  public String run(MoneyFormatter formatter) {\n    return formatter.format(10);\n  }\n}\n",
	);
	git(tmp.path(), &["init"]);
	git(tmp.path(), &["config", "user.email", "agent@example.com"]);
	git(tmp.path(), &["config", "user.name", "Agent"]);
	git(tmp.path(), &["add", "."]);
	git(tmp.path(), &["commit", "-m", "baseline"]);
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return \"$\" + cents;\n  }\n}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('d'))).unwrap();

	assert_eq!(app.view_mode(), VisualizationMode::Change);
	assert_eq!(app.view(), View::Change);
	assert_eq!(app.visible_defs().len(), 1, "{:?}", app.visible_defs());
	let changed = app.visible_defs()[0];
	assert_eq!(symbol_name(&app, &changed), "format(cents:long)");
	assert!(app.store().change_detail_for_symbol(&changed).is_some());
	assert_eq!(app.store().change_count_for_file(changed.file), 1);
	let change = app.store().change_detail_for_symbol(&changed).unwrap();
	assert_eq!(change.blast_radius.summary.refs, 1);
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "format(cents:long)"),
		"{:?}",
		app.nav_rows()
	);
	let diff_lines = change_panel_lines(&app, 100);
	let rendered_diff = diff_lines
		.iter()
		.map(line_text)
		.collect::<Vec<_>>()
		.join("\n");
	assert!(
		rendered_diff.contains("status    modified"),
		"{rendered_diff}"
	);
	assert!(rendered_diff.contains("blast radius"), "{rendered_diff}");
	assert!(rendered_diff.contains("1 direct usage"), "{rendered_diff}");

	app.handle_key(key(KeyCode::Char('u'))).unwrap();

	assert_eq!(app.view_mode(), VisualizationMode::Change);
	assert_eq!(app.view(), View::Change);
	assert_eq!(app.change_panel(), ChangePanelMode::Usages);
	let usage_lines = change_panel_lines(&app, 100);
	let rendered_usages = usage_lines
		.iter()
		.map(line_text)
		.collect::<Vec<_>>()
		.join("\n");
	assert!(
		rendered_usages.contains("blast radius"),
		"{rendered_usages}"
	);
	assert!(
		rendered_usages.contains("BillingApplication"),
		"{rendered_usages}"
	);

	app.handle_key(key(KeyCode::Char('u'))).unwrap();

	assert_eq!(app.change_panel(), ChangePanelMode::Diff);
}

#[test]
fn change_mode_shows_removed_symbol_and_its_blast_radius() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return Long.toString(cents);\n  }\n}\n",
	);
	write(
		tmp.path(),
		"src/main/java/com/acme/BillingApplication.java",
		"package com.acme;\npublic class BillingApplication {\n  public String run(MoneyFormatter formatter) {\n    return formatter.format(10);\n  }\n}\n",
	);
	git(tmp.path(), &["init"]);
	git(tmp.path(), &["config", "user.email", "agent@example.com"]);
	git(tmp.path(), &["config", "user.name", "Agent"]);
	git(tmp.path(), &["add", "."]);
	git(tmp.path(), &["commit", "-m", "baseline"]);
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('d'))).unwrap();

	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "format(cents:long)"
				&& matches!(row.kind, NavNodeKind::Change(_))),
		"{:?}",
		app.nav_rows()
	);
	select_nav_label(&mut app, "format(cents:long)");
	let lines = change_panel_lines(&app, 100);
	let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
	assert!(rendered.contains("status    removed"), "{rendered}");
	assert!(rendered.contains("format(cents:long)"), "{rendered}");
	assert!(rendered.contains("1 direct usage"), "{rendered}");
}

#[test]
fn full_store_event_reloads_index_and_refreshes_active_search() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_text_filter(&mut app, "Beta");

	assert!(app.visible_defs().is_empty(), "{:?}", app.visible_defs());

	write(tmp.path(), "src/b.ts", "class Beta {}\n");
	app.handle_store_event(StoreEvent::FullIndex);

	assert!(
		app.visible_defs()
			.iter()
			.any(|loc| symbol_name(&app, loc) == "Beta"),
		"{:?}",
		app.visible_defs()
	);
	assert!(
		app.nav_rows().iter().any(|row| row.label.contains("Beta")),
		"{:?}",
		app.nav_rows()
	);
	assert!(app.status().contains("store reloaded"), "{}", app.status());
}

#[test]
fn full_store_event_preserves_expanded_tree_and_selected_symbol() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.ts");
	app.toggle_selected_nav();
	select_nav_label(&mut app, "Alpha");
	let selected_key = app.selected_nav_row().expect("selected row").key.clone();

	write(tmp.path(), "src/0.ts", "class Before {}\n");
	app.handle_store_event(StoreEvent::FullIndex);

	assert_eq!(
		app.selected_nav_row().map(|row| row.key.clone()),
		Some(selected_key),
		"{:?}",
		app.nav_rows()
	);
	assert!(
		app.nav_rows().iter().any(|row| row.label == "Beta"),
		"file expansion should survive reload: {:?}",
		app.nav_rows()
	);
	assert!(
		app.nav_rows().iter().any(|row| row.label == "0.ts"),
		"reloaded tree should include new files without collapsing opened branches: {:?}",
		app.nav_rows()
	);
}

#[test]
fn full_store_event_refreshes_change_navigator_while_change_mode_is_active() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return Long.toString(cents);\n  }\n}\n",
	);
	git(tmp.path(), &["init"]);
	git(tmp.path(), &["config", "user.email", "agent@example.com"]);
	git(tmp.path(), &["config", "user.name", "Agent"]);
	git(tmp.path(), &["add", "."]);
	git(tmp.path(), &["commit", "-m", "baseline"]);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('d'))).unwrap();
	assert!(app.nav_rows().is_empty(), "{:?}", app.nav_rows());

	write(
		tmp.path(),
		"src/main/java/com/acme/MoneyFormatter.java",
		"package com.acme;\npublic class MoneyFormatter {\n  public String format(long cents) {\n    return \"$\" + cents;\n  }\n}\n",
	);
	app.handle_store_event(StoreEvent::FullIndex);

	assert_eq!(app.view_mode(), VisualizationMode::Change);
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "format(cents:long)"
				&& matches!(row.kind, NavNodeKind::Change(_))),
		"{:?}",
		app.nav_rows()
	);
	assert!(app.status().contains("store reloaded"), "{}", app.status());
}

#[test]
fn navigator_compacts_linear_branches_and_expands_at_branch_points() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Foo { bar() { return 1; } }\nfunction helper() { return 2; }\n",
	);
	write(tmp.path(), "src/nested/b.ts", "class Other {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	assert_eq!(app.nav_rows().len(), 1);
	assert_eq!(app.nav_rows()[0].label, "ts/src");
	assert_eq!(app.nav_rows()[0].file_count, 2);
	assert!(app.selected().is_none());

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.ts");
	app.toggle_selected_nav();

	select_nav_label(&mut app, "Foo");
	let selected = app.selected().expect("selected symbol");
	assert_eq!(symbol_name(&app, &selected), "Foo");
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label.starts_with("helper"))
	);
}

#[test]
fn navigator_renders_uncompacted_language_rows_as_containers() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "pgtap/sql/00_smoke.sql", "select 1;\n");
	write(
		tmp.path(),
		"crates/core/tests/fixtures/sql/users.sql",
		"select 2;\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	let row = app
		.nav_rows()
		.iter()
		.find(|row| row.label == "sql")
		.unwrap_or_else(|| panic!("missing uncompacted SQL language row: {:?}", app.nav_rows()));
	assert!(matches!(row.kind, NavNodeKind::Lang));
	assert!(row.has_children);

	let rendered = line_text(&nav_row_line(&app, row, false));
	assert!(
		rendered.contains("sql/"),
		"uncompacted language rows should render as containers: {rendered:?}"
	);
}

#[test]
fn explorer_orders_symbols_by_language_kind_contract() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"function Ahelper() {}\nconst Bvalue = 1;\nclass Zeta {}\ninterface YResolver {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.ts");
	app.toggle_selected_nav();

	let labels: Vec<_> = app
		.nav_rows()
		.iter()
		.filter_map(|row| matches!(row.kind, NavNodeKind::Def(_)).then_some(row.label.as_str()))
		.collect();
	assert_eq!(labels, vec!["Zeta", "YResolver", "Ahelper()", "Bvalue"]);
}

#[test]
fn explorer_orders_rust_symbols_by_language_kind_contract() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.rs",
		"fn helper() {}\n#[test]\nfn parses() {}\nstruct Service;\nconst LIMIT: u8 = 1;\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.rs");
	app.toggle_selected_nav();

	let labels: Vec<_> = app
		.nav_rows()
		.iter()
		.filter_map(|row| matches!(row.kind, NavNodeKind::Def(_)).then_some(row.label.as_str()))
		.collect();
	assert_eq!(labels, vec!["Service", "helper()", "parses()", "LIMIT"]);
}

#[test]
fn explorer_orders_symbols_after_flattening_non_navigable_modules() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.rs",
		"#[cfg(test)]\nmod tests {\n    fn helper() {}\n    #[test]\n    fn parses() {}\n}\nstruct Service;\nfn run() {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.rs");
	app.toggle_selected_nav();

	let labels: Vec<_> = app
		.nav_rows()
		.iter()
		.filter_map(|row| matches!(row.kind, NavNodeKind::Def(_)).then_some(row.label.as_str()))
		.collect();
	assert_eq!(labels, vec!["Service", "helper()", "run()", "parses()"]);
}

#[test]
fn explorer_shows_java_record_fields_before_accessors() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/User.java",
		"public record User(String id, int age) { public String label() { return id; } }\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "User.java");
	app.toggle_selected_nav();
	select_nav_label(&mut app, "User");
	app.toggle_selected_nav();

	let rows: Vec<_> = app
		.nav_rows()
		.iter()
		.filter_map(|row| {
			let NavNodeKind::Def(loc) = row.kind else {
				return None;
			};
			Some((symbol_kind(&app, &loc), row.label.clone()))
		})
		.collect();
	let id_field = rows
		.iter()
		.position(|(kind, label)| kind == "field" && label == "id")
		.unwrap_or_else(|| panic!("missing record field id: {rows:?}"));
	let age_field = rows
		.iter()
		.position(|(kind, label)| kind == "field" && label == "age")
		.unwrap_or_else(|| panic!("missing record field age: {rows:?}"));
	let first_method = rows
		.iter()
		.position(|(kind, _)| kind == "method")
		.unwrap_or_else(|| panic!("missing record accessors/methods: {rows:?}"));

	assert!(
		id_field < first_method && age_field < first_method,
		"record fields should be visible before accessors/methods: {rows:?}"
	);
}

#[test]
fn multi_source_navigator_keeps_source_roots_as_directory_rows() {
	let tmp = tempfile::tempdir().unwrap();
	let common = tmp.path().join("common-lib");
	let billing = tmp.path().join("billing-service");
	let order = tmp.path().join("order-service");
	write(
		&common,
		"src/main/java/com/acme/common/A.java",
		"class A {}\n",
	);
	write(
		&common,
		"src/main/java/com/acme/common/B.java",
		"class B {}\n",
	);
	write(
		&billing,
		"src/main/java/com/acme/billing/BillingApplication.java",
		"class BillingApplication {}\n",
	);
	write(
		&order,
		"src/main/java/com/acme/order/OrderApplication.java",
		"class OrderApplication {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![common, billing, order],
		project: None,
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	select_nav_label(&mut app, "java");
	app.toggle_selected_nav();

	let service_rows: Vec<_> = app
		.nav_rows()
		.iter()
		.filter(|row| {
			row.label.starts_with("billing-service/") || row.label.starts_with("order-service/")
		})
		.collect();
	assert_eq!(service_rows.len(), 2, "{:?}", app.nav_rows());
	for row in service_rows {
		assert!(
			matches!(row.kind, NavNodeKind::Dir),
			"single-file services should remain directory rows: {row:?}"
		);
		assert!(
			!row.label.contains("Application.java") && !row.label.contains("Application"),
			"service root row should not be compacted into a file or class: {row:?}"
		);
	}
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "common-lib/src/main/java/com/acme/common"),
		"{:?}",
		app.nav_rows()
	);
}

#[test]
fn usage_focus_filters_consumers_of_selected_common_java_symbol() {
	let tmp = tempfile::tempdir().unwrap();
	let common = tmp.path().join("common-lib");
	let billing = tmp.path().join("billing-service");
	let order = tmp.path().join("order-service");
	write(
		&common,
		"src/main/java/com/acme/common/MoneyFormatter.java",
		"package com.acme.common;\npublic class MoneyFormatter { public String format(long cents) { return Long.toString(cents); } }\n",
	);
	write(
		&billing,
		"src/main/java/com/acme/billing/BillingApplication.java",
		"package com.acme.billing;\nimport com.acme.common.MoneyFormatter;\npublic class BillingApplication { private final MoneyFormatter formatter = new MoneyFormatter(); public String run() { return formatter.format(10); } }\n",
	);
	write(
		&order,
		"src/main/java/com/acme/order/OrderApplication.java",
		"package com.acme.order;\npublic class OrderApplication { public String run() { return \"ok\"; } }\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![common, billing, order],
		project: None,
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let money_formatter = find_symbol(&app, "class", "MoneyFormatter");

	app.focus_usages(money_formatter);

	assert_eq!(app.view(), View::Refs);
	assert!(
		app.status().contains("usages of MoneyFormatter"),
		"{}",
		app.status()
	);
	assert_eq!(app.view_mode(), VisualizationMode::Usages);
	assert_eq!(app.panel_policy(), PanelPolicy::Contextual);
	let header = line_text(&header_line(&app, 120));
	assert!(header.contains("mode usages"), "{header}");
	assert!(header.contains("scope MoneyFormatter"), "{header}");
	assert!(!header.contains("panel"), "{header}");
	assert!(
		app.visible_defs()
			.iter()
			.any(|loc| symbol_name(&app, loc) == "BillingApplication"),
		"{:?}",
		app.visible_defs()
	);
	assert!(
		!app.visible_defs()
			.iter()
			.any(|loc| symbol_name(&app, loc) == "OrderApplication"),
		"{:?}",
		app.visible_defs()
	);
	assert!(
		app.nav_rows().iter().any(|row| {
			row.label.contains("billing-service") && row.label.contains("BillingApplication")
		}),
		"{:?}",
		app.nav_rows()
	);
	assert!(
		!app.nav_rows()
			.iter()
			.any(|row| row.label.contains("order-service")),
		"{:?}",
		app.nav_rows()
	);
}

#[test]
fn escape_leaves_empty_usage_focus_back_to_explorer() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/CustomerProfile.java",
		"public record CustomerProfile(boolean premium) {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let premium_accessor = find_symbol(&app, "method", "premium()");

	app.focus_usages(premium_accessor);

	assert_eq!(app.view_mode(), VisualizationMode::Usages);
	assert!(app.is_filtered());
	assert!(app.nav_rows().is_empty());
	assert!(app.status().contains("0 reference(s)"), "{}", app.status());

	assert!(!app.handle_key(key(KeyCode::Esc)).unwrap());

	assert_eq!(app.view_mode(), VisualizationMode::Explorer);
	assert!(!app.is_filtered());
	assert_eq!(app.filter_label(), "<all>");
	assert!(!app.nav_rows().is_empty());
	assert_eq!(app.view(), View::Overview);
	assert!(app.status().contains("filter cleared"), "{}", app.status());
}

#[test]
fn refs_panel_prioritizes_incoming_impact_with_location_context() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class MoneyFormatter {}\nclass BillingApplication { formatter: MoneyFormatter = new MoneyFormatter(); }\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let money_formatter = find_symbol(&app, "class", "MoneyFormatter");
	let panel_width = 64;
	let lines: Vec<_> = refs_panel_lines(&app, money_formatter, panel_width)
		.iter()
		.map(line_text)
		.collect();
	let incoming = lines
		.iter()
		.position(|line| line == "incoming impact")
		.expect("incoming section");
	let outgoing = lines
		.iter()
		.position(|line| line == "outgoing dependencies")
		.expect("outgoing section");

	assert!(incoming < outgoing, "{lines:?}");
	assert!(
		lines.iter().any(|line| line.contains("formatter")),
		"{lines:?}"
	);
	assert!(
		lines
			.iter()
			.any(|line| line.contains("src/a.ts") && line.contains(":L")),
		"{lines:?}"
	);
	assert!(
		lines.iter().any(|line| line.contains("source ")),
		"{lines:?}"
	);
	assert!(
		lines
			.iter()
			.any(|line| line.contains("source ts:") && line.contains("field:formatter")),
		"refs panel should render compact source monikers: {lines:?}"
	);
	let kind_lines: Vec<_> = lines
		.iter()
		.filter(|line| line.trim_start().starts_with("kinds"))
		.collect();
	assert_eq!(kind_lines.len(), 1, "{lines:?}");
	assert!(
		kind_lines[0].contains("instantiates") && kind_lines[0].contains("uses_type"),
		"refs with the same component context should be grouped: {lines:?}"
	);
	assert!(
		!lines.iter().any(|line| line.contains("code+moniker://")),
		"refs panel should not render full moniker URIs: {lines:?}"
	);
	assert!(
		lines.iter().all(|line| line.chars().count() <= panel_width),
		"refs panel lines should fit their component width: {lines:?}"
	);
}

#[test]
fn outline_panel_renders_compact_moniker_format() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/args.rs",
		"#[cfg(test)]\nmod tests {\n    #[test]\n    fn no_args_requires_subcommand() {}\n}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.toggle_selected_nav();
	select_nav_label(&mut app, "args.rs");
	app.toggle_selected_nav();
	select_nav_label(&mut app, "no_args_requires_subcommand()");
	app.sync_contextual_view();

	let snapshot = active_panel_snapshot(&app).to_text(app.view_mode().label(), &app.scope_label());
	assert!(
		snapshot.contains("moniker   rs:src/args.tests.test:no_args_requires_subcommand()"),
		"{snapshot}"
	);
	assert!(!snapshot.contains("/lang:"), "{snapshot}");
	assert!(!snapshot.contains("/dir:"), "{snapshot}");
}

#[test]
fn kind_filter_limits_navigator_to_matching_declaration_kinds() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\ninterface Resolver {}\nfunction helper() {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_kind_filter(&mut app, "Resolver", Some(Lang::Ts), "interface");

	assert_eq!(app.visible_defs().len(), 1, "{:?}", app.nav_rows());
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Resolver"),
		"{:?}",
		app.nav_rows()
	);
	assert!(!app.nav_rows().iter().any(|row| row.label.contains("Alpha")));
	assert!(
		app.filter_label().contains("kind:interface"),
		"{}",
		app.filter_label()
	);
}

#[test]
fn rust_fn_kind_is_navigable_and_filterable() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/lib.rs", "pub fn build() {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_kind_filter(&mut app, "build", Some(Lang::Rs), "fn");

	assert_eq!(app.visible_defs().len(), 1, "{:?}", app.nav_rows());
	assert!(
		app.nav_rows().iter().any(|row| row.label.contains("build")),
		"{:?}",
		app.nav_rows()
	);
}

#[test]
fn filter_counts_only_navigable_declarations() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/lib.rs",
		"pub fn build(value: u32) { let local = value; }\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_kind_filter(&mut app, "", Some(Lang::Rs), "local");

	assert!(app.visible_defs().is_empty(), "{:?}", app.visible_defs());
	assert!(app.nav_rows().is_empty(), "{:?}", app.nav_rows());
}

#[test]
fn free_search_treats_glob_like_text_as_plain_input() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	apply_text_filter(&mut app, "*Provider");

	assert!(matches!(app.active_filter(), ActiveFilter::HeaderSearch(_)));
	assert!(app.nav_rows().is_empty());
	assert!(app.status().contains("*Provider"), "{}", app.status());

	assert!(!app.handle_key(key(KeyCode::Esc)).unwrap());

	assert_eq!(app.view_mode(), VisualizationMode::Explorer);
	assert!(!app.is_filtered());
	assert!(!app.nav_rows().is_empty());
	assert_eq!(app.filter_label(), "<all>");
}

#[test]
fn source_snippet_preserves_indent_and_dims_context_lines() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"const before = 1;\nfunction target() {\n    nested();\n}\nconst after = 2;\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let target = app
		.visible_defs()
		.iter()
		.copied()
		.find(|loc| symbol_name(&app, loc).starts_with("target"))
		.expect("target function");

	let snippet = source_snippet(&app, &target, 1);
	let lines = source_snippet_lines(&snippet);

	let nested_line = lines
		.iter()
		.find(|line| {
			line.spans
				.iter()
				.any(|span| span.content.as_ref() == "nested();")
		})
		.expect("nested line");
	assert!(
		nested_line.spans.iter().any(|span| {
			span.content.as_ref() == "    " && span.style.bg == Some(THEME.source.active_indent_bg)
		}),
		"{nested_line:?}"
	);
	assert_eq!(nested_line.style.bg, Some(THEME.source.active_bg));
	let nested_body = nested_line
		.spans
		.iter()
		.find(|span| span.content.as_ref() == "nested();")
		.expect("nested body span");
	assert_eq!(nested_body.style.fg, Some(THEME.source.active_fg));
	assert_eq!(nested_body.style.bg, Some(THEME.source.active_bg));
	let before_line = lines
		.iter()
		.find(|line| {
			line.spans
				.iter()
				.any(|span| span.content.as_ref() == "const before = 1;")
		})
		.expect("before context line");
	assert_eq!(before_line.style.bg, Some(THEME.source.context_bg));
	assert_eq!(before_line.spans[2].style.fg, Some(THEME.source.context_fg));
	assert_eq!(
		before_line.spans[0].style.fg,
		Some(THEME.source.context_number_fg)
	);
}

#[test]
fn header_search_keystrokes_update_text_until_enter_applies_filter() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let total = app.visible_defs().len();

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	for c in "Alpha".chars() {
		app.handle_key(key(KeyCode::Char(c))).unwrap();
	}

	assert_eq!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	assert_eq!(app.header_search().text, "Alpha");
	assert_eq!(app.visible_defs().len(), total);

	app.handle_key(key(KeyCode::Enter)).unwrap();

	assert_eq!(app.mode(), UiMode::Normal);
	assert!(app.visible_defs().len() < total);
	assert!(
		app.visible_defs()
			.iter()
			.all(|loc| symbol_name(&app, loc).contains("Alpha")),
		"{:?}",
		app.visible_defs()
	);
	assert!(app.status().contains("Alpha"), "{}", app.status());
	assert!(
		app.nav_rows()
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Alpha")
	);
	assert!(!app.nav_rows().iter().any(|row| row.label.contains("Beta")));
}

#[test]
fn header_search_ignores_alt_modified_printable_chars() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::ALT))
		.unwrap();

	assert_eq!(app.header_search().text, "");
}

#[test]
fn x_resets_filter_from_navigation_and_search_header() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	apply_text_filter(&mut app, "Alpha");
	assert!(app.is_filtered());

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	app.handle_key(key(KeyCode::Char('x'))).unwrap();

	assert_eq!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	assert_eq!(app.header_search().text, "");
	assert!(!app.is_filtered());

	apply_text_filter(&mut app, "Alpha");
	app.handle_key(key(KeyCode::Char('x'))).unwrap();
	assert!(!app.is_filtered());
	assert_eq!(app.view_mode(), VisualizationMode::Explorer);
	assert_eq!(app.filter_label(), "<all>");
}

#[test]
fn live_search_clear_preserves_header_text_focus() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('s'))).unwrap();
	app.handle_key(key(KeyCode::Char('A'))).unwrap();
	app.apply_header_search(None, false);
	assert!(app.is_filtered());

	app.handle_key(key(KeyCode::Backspace)).unwrap();
	app.apply_header_search(None, false);

	assert_eq!(app.mode(), UiMode::HeaderSearch(HeaderSearchFocus::Text));
	assert!(!app.is_filtered());
	assert_eq!(app.header_search().text, "");
}

#[test]
fn escape_closes_navigation_and_explicit_quit_keys_exit() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	let selected_key = app.selected_nav_row().expect("selected row").key.clone();
	app.handle_key(key(KeyCode::Enter)).unwrap();
	assert!(app.active_expanded().contains(&selected_key));

	assert!(!app.handle_key(key(KeyCode::Esc)).unwrap());
	assert_eq!(app.view(), View::Overview);
	assert!(!app.active_expanded().contains(&selected_key));
	assert!(app.status().contains("closed"), "{}", app.status());
	assert_eq!(app.view(), View::Overview);
	assert!(matches!(app.check_state(), CheckState::Pending));

	assert!(app.handle_key(key(KeyCode::Char('q'))).unwrap());
	assert!(
		app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
			.unwrap()
	);
}

#[test]
fn normal_mode_ignores_control_modified_command_keys() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = WorkspaceStore::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		store,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	apply_text_filter(&mut app, "Alpha");
	let visible = app.visible_defs().to_vec();
	let view = app.view();
	let status = app.status().to_string();

	app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
		.unwrap();
	app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
		.unwrap();

	assert_eq!(app.view(), view);
	assert_eq!(app.visible_defs(), visible.as_slice());
	assert_eq!(app.status(), status);
	assert!(app.is_filtered());
}
