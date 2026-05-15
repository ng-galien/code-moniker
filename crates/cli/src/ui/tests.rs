use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::DEFAULT_SCHEME;
use crate::inspect::{SessionIndex, SessionOptions};

use super::source::source_snippet_lines;
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

#[test]
fn app_filter_limits_visible_declarations_and_keeps_tree_navigation() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	app.filter = "Alpha".into();
	app.refresh_filter(true);
	assert!(
		app.visible_defs
			.iter()
			.all(|loc| last_name(&app.index.def(loc).moniker).contains("Alpha")),
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
			&app.index
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
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
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
	assert_eq!(last_name(&app.index.def(&selected).moniker), "Foo");
	assert!(
		app.nav_rows
			.iter()
			.any(|row| row.label.starts_with("helper"))
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
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.filter = "kind:interface Resolver".into();
	app.refresh_filter(true);

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
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.filter = "kind:fn build".into();
	app.refresh_filter(true);

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
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.filter = "kind:local".into();
	app.refresh_filter(true);

	assert!(app.visible_defs.is_empty(), "{:?}", app.visible_defs);
	assert!(app.nav_rows.is_empty(), "{:?}", app.nav_rows);
}

#[test]
fn invalid_filter_regex_clears_rows_with_actionable_status() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.filter = "*Provider".into();
	app.refresh_filter(true);

	assert!(app.filter_error.is_some());
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
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let target = app
		.visible_defs
		.iter()
		.copied()
		.find(|loc| last_name(&app.index.def(loc).moniker).starts_with("target"))
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
fn search_mode_keystrokes_update_filter_and_visible_declarations() {
	let tmp = tempfile::tempdir().unwrap();
	write(
		tmp.path(),
		"src/a.ts",
		"class Alpha {}\nclass Beta {}\nfunction gamma() {}\n",
	);
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);
	let total = app.visible_defs.len();

	app.handle_key(key(KeyCode::Char('/'))).unwrap();
	for c in "Alpha".chars() {
		app.handle_key(key(KeyCode::Char(c))).unwrap();
	}

	assert!(app.search_mode);
	assert_eq!(app.filter, "Alpha");
	assert!(app.visible_defs.len() < total);
	assert!(
		app.visible_defs
			.iter()
			.all(|loc| last_name(&app.index.def(loc).moniker).contains("Alpha")),
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
fn search_mode_accepts_printable_chars_with_terminal_modifiers() {
	let tmp = tempfile::tempdir().unwrap();
	write(tmp.path(), "src/a.ts", "class Alpha {}\n");
	let index = SessionIndex::load(&SessionOptions {
		paths: vec![tmp.path().into()],
		project: Some("app".into()),
		cache_dir: None,
	})
	.unwrap();
	let mut app = App::new(
		index,
		DEFAULT_SCHEME.to_string(),
		tmp.path().join(".code-moniker.toml"),
		None,
	);

	app.handle_key(key(KeyCode::Char('/'))).unwrap();
	app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::ALT))
		.unwrap();

	assert_eq!(app.filter, "A");
	assert!(app.status.contains("A"), "{}", app.status);
}
