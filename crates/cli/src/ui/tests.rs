use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use code_moniker_core::lang::Lang;

use crate::DEFAULT_SCHEME;
use crate::inspect::SessionOptions;

use super::kinds::{KindGroup, definition_kind_group, definition_kind_order, reference_kind_group};
use super::source::source_snippet_lines;
use super::store::IndexStore;
use super::theme::THEME;
use super::*;

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

fn line_text(line: &Line<'_>) -> String {
	line.spans
		.iter()
		.map(|span| span.content.as_ref())
		.collect::<String>()
}

fn select_nav_label(app: &mut App, label: &str) {
	app.selection = app
		.nav_rows
		.iter()
		.position(|row| row.label == label)
		.unwrap_or_else(|| panic!("missing navigator row {label}: {:?}", app.nav_rows));
}

fn select_nav_label_ending_with(app: &mut App, suffix: &str) {
	app.selection = app
		.nav_rows
		.iter()
		.position(|row| row.label.ends_with(suffix))
		.unwrap_or_else(|| {
			panic!(
				"missing navigator row ending with {suffix}: {:?}",
				app.nav_rows
			)
		});
}

fn apply_text_filter(app: &mut App, raw: &str) {
	app.filter_draft = raw.to_string();
	app.apply_filter();
}

#[test]
fn component_titles_include_stable_collaboration_markers() {
	let title = block_title("navigator", ComponentId::Navigator);
	let rendered = line_text(&title);

	assert_eq!(rendered, "navigator [ui.navigator]");
	assert_eq!(ComponentId::PanelRefs.as_str(), "ui.panel.refs");
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
		vec!["Overview", "Outline", "Refs", "Check"]
	);
	assert!(
		commands
			.iter()
			.any(|command| command.label == "Edit filter"
				&& command.shortcut.as_deref() == Some("/")),
		"{commands:?}"
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
}

#[test]
fn header_exposes_visualization_regime_and_scope_only() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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

	assert_eq!(app.regime, VisualizationRegime::Explorer);
	assert_eq!(app.view, View::Overview);
	let initial = line_text(&header_line(&app, 120));
	assert_eq!(
		initial,
		"code-moniker [ui.header] regime explorer  scope all"
	);

	apply_text_filter(&mut app, "Alpha");

	assert_eq!(app.regime, VisualizationRegime::Search);
	assert_eq!(app.panel_policy, PanelPolicy::Contextual);
	assert_eq!(app.view, View::Tree);
	let filtered = line_text(&header_line(&app, 120));
	assert!(filtered.contains("regime search"), "{filtered}");
	assert!(filtered.contains("scope /Alpha"), "{filtered}");
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
	let store = MemoryIndexStore::load(&SessionOptions {
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

	assert_eq!(app.route, ExplorerFeature::route(ROUTE_OVERVIEW));
	app.handle_key(key(KeyCode::Char('3'))).unwrap();

	assert_eq!(app.view, View::Refs);
	assert_eq!(app.route, ExplorerFeature::route(ROUTE_REFS));
}

#[test]
fn contextual_panel_tracks_selected_declaration_in_explorer_regime() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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

	assert_eq!(app.regime, VisualizationRegime::Explorer);
	assert_eq!(app.panel_policy, PanelPolicy::Contextual);
	assert_eq!(app.view, View::Overview);

	app.handle_key(key(KeyCode::Enter)).unwrap();
	app.handle_key(key(KeyCode::Down)).unwrap();
	assert!(app.selected().is_none());
	assert_eq!(app.view, View::Overview);

	app.handle_key(key(KeyCode::Enter)).unwrap();
	app.handle_key(key(KeyCode::Down)).unwrap();

	assert_eq!(
		last_name(
			&app.store
				.def(&app.selected().expect("selected declaration"))
				.moniker
		),
		"Alpha"
	);
	assert_eq!(app.view, View::Tree);
	assert_eq!(app.route, ExplorerFeature::route(ROUTE_OUTLINE));
}

#[test]
fn app_filter_limits_visible_declarations_and_keeps_tree_navigation() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let store = MemoryIndexStore::load(&SessionOptions {
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
		app.visible_defs
			.iter()
			.all(|loc| last_name(&app.store.def(loc).moniker).contains("Alpha")),
		"{:?}",
		app.visible_defs
	);
	assert!(!app.visible_defs.is_empty());
	assert!(
		app.nav_rows
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Alpha"),
		"{:?}",
		app.nav_rows
	);
	assert!(!app.nav_rows.iter().any(|row| row.label.contains("Beta")));
	select_nav_label_ending_with(&mut app, "Alpha");
	assert_eq!(
		last_name(
			&app.store
				.def(&app.selected().expect("selected Alpha"))
				.moniker
		),
		"Alpha"
	);
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
	let store = MemoryIndexStore::load(&SessionOptions {
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

	assert_eq!(app.nav_rows.len(), 1);
	assert_eq!(app.nav_rows[0].label, "ts/src");
	assert_eq!(app.nav_rows[0].file_count, 2);
	assert!(app.selected().is_none());

	app.toggle_selected_nav();
	select_nav_label(&mut app, "a.ts");
	app.toggle_selected_nav();

	select_nav_label(&mut app, "Foo");
	let selected = app.selected().expect("selected symbol");
	assert_eq!(last_name(&app.store.def(&selected).moniker), "Foo");
	assert!(
		app.nav_rows
			.iter()
			.any(|row| row.label.starts_with("helper"))
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
	let store = MemoryIndexStore::load(&SessionOptions {
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
		.nav_rows
		.iter()
		.filter_map(|row| matches!(row.kind, NavNodeKind::Def(_)).then_some(row.label.as_str()))
		.collect();
	assert_eq!(labels, vec!["Zeta", "YResolver", "Ahelper()", "Bvalue"]);
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
	let store = MemoryIndexStore::load(&SessionOptions {
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
		.nav_rows
		.iter()
		.filter(|row| {
			row.label.starts_with("billing-service/") || row.label.starts_with("order-service/")
		})
		.collect();
	assert_eq!(service_rows.len(), 2, "{:?}", app.nav_rows);
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
		app.nav_rows
			.iter()
			.any(|row| row.label == "common-lib/src/main/java/com/acme/common"),
		"{:?}",
		app.nav_rows
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
	let store = MemoryIndexStore::load(&SessionOptions {
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
	let money_formatter = (0..app.store.file_count())
		.flat_map(|file_idx| {
			app.store
				.file(file_idx)
				.graph
				.defs()
				.enumerate()
				.map(move |(def_idx, _)| DefLocation {
					file: file_idx,
					def: def_idx,
				})
		})
		.find(|loc| {
			let def = app.store.def(loc);
			def_kind(def) == "class" && last_name(&def.moniker) == "MoneyFormatter"
		})
		.expect("MoneyFormatter class");

	app.focus_usages(money_formatter);

	assert_eq!(app.view, View::Refs);
	assert!(
		app.status.contains("usages of MoneyFormatter"),
		"{}",
		app.status
	);
	assert_eq!(app.regime, VisualizationRegime::Usages);
	assert_eq!(app.panel_policy, PanelPolicy::Contextual);
	let header = line_text(&header_line(&app, 120));
	assert!(header.contains("regime usages"), "{header}");
	assert!(header.contains("scope MoneyFormatter"), "{header}");
	assert!(!header.contains("panel"), "{header}");
	assert!(
		app.visible_defs
			.iter()
			.any(|loc| last_name(&app.store.def(loc).moniker) == "BillingApplication"),
		"{:?}",
		app.visible_defs
	);
	assert!(
		!app.visible_defs
			.iter()
			.any(|loc| last_name(&app.store.def(loc).moniker) == "OrderApplication"),
		"{:?}",
		app.visible_defs
	);
	assert!(
		app.nav_rows.iter().any(|row| {
			row.label.contains("billing-service") && row.label.contains("BillingApplication")
		}),
		"{:?}",
		app.nav_rows
	);
	assert!(
		!app.nav_rows
			.iter()
			.any(|row| row.label.contains("order-service")),
		"{:?}",
		app.nav_rows
	);
}

#[test]
fn refs_panel_prioritizes_incoming_impact_with_location_context() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class MoneyFormatter {}\nclass BillingApplication { formatter: MoneyFormatter = new MoneyFormatter(); }\n",
	);
	let store = MemoryIndexStore::load(&SessionOptions {
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
	let money_formatter = (0..app.store.file_count())
		.flat_map(|file_idx| {
			app.store
				.file(file_idx)
				.graph
				.defs()
				.enumerate()
				.map(move |(def_idx, _)| DefLocation {
					file: file_idx,
					def: def_idx,
				})
		})
		.find(|loc| {
			let def = app.store.def(loc);
			def_kind(def) == "class" && last_name(&def.moniker) == "MoneyFormatter"
		})
		.expect("MoneyFormatter class");
	let panel_width = 64;
	let lines: Vec<_> = refs_panel_lines(
		&app,
		money_formatter,
		app.store.def(&money_formatter),
		panel_width,
	)
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
			.any(|line| line.contains("source app/") && line.contains("field:formatter")),
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
fn kind_filter_limits_navigator_to_matching_declaration_kinds() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\ninterface Resolver {}\nfunction helper() {}\n",
	);
	let store = MemoryIndexStore::load(&SessionOptions {
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

	apply_text_filter(&mut app, "kind:interface Resolver");

	assert_eq!(app.visible_defs.len(), 1, "{:?}", app.nav_rows);
	assert!(
		app.nav_rows
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Resolver"),
		"{:?}",
		app.nav_rows
	);
	assert!(!app.nav_rows.iter().any(|row| row.label.contains("Alpha")));
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
	let store = MemoryIndexStore::load(&SessionOptions {
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

	apply_text_filter(&mut app, "kind:fn build");

	assert_eq!(app.visible_defs.len(), 1, "{:?}", app.nav_rows);
	assert!(
		app.nav_rows.iter().any(|row| row.label.contains("build")),
		"{:?}",
		app.nav_rows
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
	let store = MemoryIndexStore::load(&SessionOptions {
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

	apply_text_filter(&mut app, "kind:local");

	assert!(app.visible_defs.is_empty(), "{:?}", app.visible_defs);
	assert!(app.nav_rows.is_empty(), "{:?}", app.nav_rows);
}

#[test]
fn invalid_filter_regex_clears_rows_with_actionable_status() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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

	assert!(app.active_filter.error().is_some());
	assert!(app.nav_rows.is_empty());
	assert!(
		app.status.contains("invalid filter regex"),
		"{}",
		app.status
	);
}

#[test]
fn source_snippet_preserves_indent_and_dims_context_lines() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"const before = 1;\nfunction target() {\n    nested();\n}\nconst after = 2;\n",
	);
	let store = MemoryIndexStore::load(&SessionOptions {
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
		.visible_defs
		.iter()
		.copied()
		.find(|loc| last_name(&app.store.def(loc).moniker).starts_with("target"))
		.expect("target function");

	let lines = source_snippet_lines(&app, &target, 1);

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
fn editing_filter_keystrokes_update_draft_until_enter_applies_filter() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let store = MemoryIndexStore::load(&SessionOptions {
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
	let total = app.visible_defs.len();

	app.handle_key(key(KeyCode::Char('/'))).unwrap();
	for c in "Alpha".chars() {
		app.handle_key(key(KeyCode::Char(c))).unwrap();
	}

	assert_eq!(app.mode, UiMode::EditingFilter);
	assert_eq!(app.filter_draft, "Alpha");
	assert_eq!(app.visible_defs.len(), total);

	app.handle_key(key(KeyCode::Enter)).unwrap();

	assert_eq!(app.mode, UiMode::Normal);
	assert!(app.visible_defs.len() < total);
	assert!(
		app.visible_defs
			.iter()
			.all(|loc| last_name(&app.store.def(loc).moniker).contains("Alpha")),
		"{:?}",
		app.visible_defs
	);
	assert!(app.status.contains("Alpha"), "{}", app.status);
	assert!(
		app.nav_rows
			.iter()
			.any(|row| row.label == "ts/src/a.ts/Alpha")
	);
	assert!(!app.nav_rows.iter().any(|row| row.label.contains("Beta")));
}

#[test]
fn editing_filter_accepts_printable_chars_with_terminal_modifiers() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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

	app.handle_key(key(KeyCode::Char('/'))).unwrap();
	app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::ALT))
		.unwrap();

	assert_eq!(app.filter_draft, "A");
	assert!(app.status.contains("A"), "{}", app.status);
}

#[test]
fn normal_mode_x_clears_filter_but_editing_mode_x_updates_draft() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\nclass Beta {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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

	app.handle_key(key(KeyCode::Char('/'))).unwrap();
	app.handle_key(key(KeyCode::Char('x'))).unwrap();

	assert_eq!(app.mode, UiMode::EditingFilter);
	assert_eq!(app.filter_draft, "Alphax");
	assert!(app.is_filtered());

	app.handle_key(key(KeyCode::Esc)).unwrap();
	assert_eq!(app.mode, UiMode::Normal);
	assert!(app.is_filtered());

	app.handle_key(key(KeyCode::Char('x'))).unwrap();
	assert!(!app.is_filtered());
	assert_eq!(app.regime, VisualizationRegime::Explorer);
	assert_eq!(app.filter_label(), "<all>");
}

#[test]
fn escape_closes_navigation_and_explicit_quit_keys_exit() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let store = MemoryIndexStore::load(&SessionOptions {
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
	assert_eq!(app.view, View::Overview);
	assert!(!app.active_expanded().contains(&selected_key));
	assert!(app.status.contains("closed"), "{}", app.status);
	assert_eq!(app.view, View::Overview);
	assert!(matches!(app.check, CheckState::Pending));

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
	let store = MemoryIndexStore::load(&SessionOptions {
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
	let visible = app.visible_defs.clone();
	let view = app.view;
	let status = app.status.clone();

	app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
		.unwrap();
	app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
		.unwrap();

	assert_eq!(app.view, view);
	assert_eq!(app.visible_defs, visible);
	assert_eq!(app.status, status);
	assert!(app.is_filtered());
}
