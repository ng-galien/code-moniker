use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::session::SessionOptions;
use crate::ui::app::main_split_percent;
use crate::ui::app::{
	App, AppConfig, FocusRegion, focus_region, handle_key, handle_store_event_sync, new_app,
	toggle_selected_nav,
};
use crate::ui::render::view;
use crate::ui::workspace_read::{load_local_file_catalog, load_local_workspace};
use code_moniker_workspace::live::WorkspaceLiveEvent;

const MULTIPROJECT_FIXTURE: &str = "../workspace/tests/fixtures/projects/java/multiprojet";
const RUST_MULTIPROJECT_FIXTURE: &str = "../workspace/tests/fixtures/projects/rust/multiproject";

struct TuiAcceptance {
	app: App,
	_cache_dir: tempfile::TempDir,
}

impl TuiAcceptance {
	fn load_multiproject() -> Self {
		Self::load_multiproject_with_profile(None)
	}

	fn load_multiproject_with_profile(profile: Option<&str>) -> Self {
		Self::load_multiproject_paths(vec![multiproject_fixture()], profile)
	}

	fn load_multiproject_paths(paths: Vec<PathBuf>, profile: Option<&str>) -> Self {
		let cache_dir = tempfile::tempdir().expect("cache dir");
		let fixture = multiproject_fixture();
		let opts = SessionOptions {
			paths,
			project: Some("multiprojet".to_string()),
			cache_dir: Some(cache_dir.path().to_path_buf()),
		};
		let (store, cache) = load_local_workspace(&opts).expect("load Java multiproject fixture");
		let app = new_app(
			store,
			cache,
			opts,
			app_config(
				fixture.join(".code-moniker.toml"),
				profile.map(ToOwned::to_owned),
				false,
			),
		);
		Self {
			app,
			_cache_dir: cache_dir,
		}
	}

	fn load_multiproject_catalog_only() -> Self {
		let cache_dir = tempfile::tempdir().expect("cache dir");
		let fixture = multiproject_fixture();
		let opts = SessionOptions {
			paths: vec![fixture.clone()],
			project: Some("multiprojet".to_string()),
			cache_dir: Some(cache_dir.path().to_path_buf()),
		};
		let (store, cache) =
			load_local_file_catalog(&opts).expect("load Java multiproject file catalog");
		let app = new_app(
			store,
			cache,
			opts,
			app_config(fixture.join(".code-moniker.toml"), None, false),
		);
		Self {
			app,
			_cache_dir: cache_dir,
		}
	}

	fn load_rust_multiproject() -> Self {
		let cache_dir = tempfile::tempdir().expect("cache dir");
		let fixture = rust_multiproject_fixture();
		let opts = SessionOptions {
			paths: vec![fixture.clone()],
			project: Some("rust-multiproject".to_string()),
			cache_dir: Some(cache_dir.path().to_path_buf()),
		};
		let (store, cache) = load_local_workspace(&opts).expect("load Rust multiproject fixture");
		let app = new_app(
			store,
			cache,
			opts,
			app_config(fixture.join(".code-moniker.toml"), None, false),
		);
		Self {
			app,
			_cache_dir: cache_dir,
		}
	}

	fn render_text(&self, width: u16, height: u16) -> String {
		let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
		terminal
			.draw(|frame| {
				let vm = crate::ui::explorer::view_model(&self.app);
				view::render_shell(frame, frame.area(), &vm);
			})
			.expect("draw TUI");
		format!("{}", terminal.backend())
	}

	fn search(&mut self, text: &str) {
		self.press(KeyCode::Char('s'));
		self.type_text(text);
		self.press(KeyCode::Enter);
	}

	fn type_text(&mut self, text: &str) {
		for ch in text.chars() {
			self.press(KeyCode::Char(ch));
		}
	}

	fn navigate_to_risk_policy_class(&mut self) {
		self.press(KeyCode::Enter);
		self.press_n(KeyCode::Down, 2);
		self.press(KeyCode::Enter);
		self.press(KeyCode::Down);
		self.press(KeyCode::Enter);
		self.press_n(KeyCode::Down, 4);
		self.press(KeyCode::Enter);
		self.press(KeyCode::Down);
	}

	fn press_n(&mut self, code: KeyCode, count: usize) {
		for _ in 0..count {
			self.press(code);
		}
	}

	fn press(&mut self, code: KeyCode) {
		handle_key(&mut self.app, KeyEvent::new(code, KeyModifiers::NONE)).expect("handle key");
	}
}

#[test]
fn multiproject_catalog_phase_renders_file_tree_before_symbols() {
	let mut harness = TuiAcceptance::load_multiproject_catalog_only();
	toggle_selected_nav(&mut harness.app);
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "multiprojet");
	assert_visible(&screen, "navigator 29 files");
	assert_visible(&screen, "java");
	assert_visible(&screen, "billing-service");
	assert_visible(&screen, "common-lib");
	assert_visible(&screen, "inventory-service");
	assert_visible(&screen, "loyalty-platform");
	assert_visible(&screen, "order-service");
	assert_visible(&screen, "spring-edge");
	assert_visible(&screen, "files      29");
	assert_visible(&screen, "defs       0");
	assert_visible(&screen, "refs       0");
}

#[test]
fn on_demand_store_event_marks_navigator_stale_until_refresh() {
	let mut harness = TuiAcceptance::load_multiproject();
	harness.app.config.live_refresh = crate::args::LiveRefresh::OnDemand;

	handle_store_event_sync(
		&mut harness.app,
		WorkspaceLiveEvent::SourcesChanged(vec![multiproject_fixture().join("App.java")]),
	);

	let screen = harness.render_text(160, 45);
	assert_visible(&screen, "stale: 1 stale path(s)");
	assert_visible(&screen, "R refreshes");
	assert!(
		crate::ui::app::pending_live_plan_summary(&harness.app).is_some(),
		"pending live plan should be recorded"
	);

	harness.press(KeyCode::Char('R'));
	assert!(
		crate::ui::app::status(&harness.app).contains("task runtime unavailable"),
		"without a task runtime the refresh cannot run: {}",
		crate::ui::app::status(&harness.app)
	);
	assert!(
		crate::ui::app::pending_live_plan_summary(&harness.app).is_some(),
		"pending live plan must survive a failed refresh queue"
	);
}

#[test]
fn auto_store_event_keeps_navigator_fresh() {
	let mut harness = TuiAcceptance::load_multiproject();

	handle_store_event_sync(
		&mut harness.app,
		WorkspaceLiveEvent::SourcesChanged(vec![multiproject_fixture().join("App.java")]),
	);

	assert!(
		crate::ui::app::pending_live_plan_summary(&harness.app).is_none(),
		"auto mode must not record staleness"
	);
	let screen = harness.render_text(160, 45);
	assert!(
		!screen.contains("stale:"),
		"auto mode must not render a stale badge"
	);
}

#[test]
fn multiproject_multiple_paths_behave_like_virtual_parent() {
	let mut harness = TuiAcceptance::load_multiproject_paths(multiproject_project_paths(), None);
	toggle_selected_nav(&mut harness.app);
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "multiprojet");
	assert_visible(&screen, "navigator 29 files");
	assert_visible(&screen, "java");
	assert_visible(&screen, "billing-service");
	assert_visible(&screen, "common-lib");
	assert_visible(&screen, "inventory-service");
	assert_visible(&screen, "loyalty-platform");
	assert_visible(&screen, "order-service");
	assert_visible(&screen, "spring-edge");
	assert_visible(&screen, "files      29");
	assert_visible(&screen, "defs       243");
	assert_visible(&screen, "refs       577");
}

#[test]
fn multiproject_initial_screen_exposes_navigation_contract() {
	let mut harness = TuiAcceptance::load_multiproject();
	toggle_selected_nav(&mut harness.app);
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "code-moniker");
	assert_visible(&screen, "multiprojet");
	assert_visible(&screen, "search");
	assert_visible(&screen, "java");
	assert_visible(&screen, "common-lib");
	assert_visible(&screen, "order-service");
}

#[test]
fn main_horizontal_split_resizes_with_control_arrows_and_resets() {
	let mut harness = TuiAcceptance::load_multiproject();
	let initial = harness.render_text(160, 45);
	let initial_width = navigator_border_width(&initial);
	assert_eq!(main_split_percent(&harness.app), 42);

	press_with_modifiers(&mut harness.app, KeyCode::Right, KeyModifiers::CONTROL);
	let widened = harness.render_text(160, 45);
	let widened_width = navigator_border_width(&widened);
	assert_eq!(main_split_percent(&harness.app), 46);
	assert!(widened_width > initial_width);
	assert_visible(&widened, "layout split: 46% navigator / 54% panel");

	harness.press(KeyCode::Char('<'));
	assert_eq!(main_split_percent(&harness.app), 42);

	harness.press(KeyCode::Char('>'));
	harness.press(KeyCode::Char('>'));
	assert_eq!(main_split_percent(&harness.app), 50);

	harness.press(KeyCode::Char('='));
	let reset = harness.render_text(160, 45);
	assert_eq!(main_split_percent(&harness.app), 42);
	assert_eq!(navigator_border_width(&reset), initial_width);
	assert_visible(&reset, "layout split: 42% navigator / 58% panel");
}

#[test]
fn debug_flag_controls_component_markers() {
	let mut harness = TuiAcceptance::load_multiproject();
	toggle_selected_nav(&mut harness.app);
	let screen = harness.render_text(120, 32);

	assert_hidden(&screen, "[ui.header]");
	assert_hidden(&screen, "[ui.search.input]");
	assert_hidden(&screen, "[ui.navigator]");
	assert_hidden(&screen, "[ui.panel.overview]");

	harness.app.config.debug = true;
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "[ui.header]");
	assert_visible(&screen, "[ui.search.input]");
	assert_visible(&screen, "[ui.navigator]");
	assert_visible(&screen, "[ui.panel.overview]");
}

#[test]
fn rust_module_only_file_shows_reexports_in_navigator() {
	let mut harness = load_rust_module_reexport_fixture();

	harness.press(KeyCode::Enter);
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "lib.rs  1 files  0 defs  1 reexports");
}

#[test]
fn views_lens_marks_tree_nodes_and_tracks_selection() {
	let mut harness = load_views_fixture();

	harness.press(KeyCode::Char('v'));
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "navigator");
	assert_visible(&screen, "[v2]");
	assert_visible(&screen, "fixture-map");
	assert_visible(&screen, "workspace/views/fixture-map");
	assert_visible(&screen, "view lens");
	assert_visible(&screen, "render     summary");
	assert_visible(&screen, "entry owns 1");
	assert_hidden(&screen, "selector class:App");
	assert_hidden(&screen, "class App {}");
	assert_hidden(&screen, "test-map");

	harness.press(KeyCode::Char('a'));
	let screen = harness.render_text(120, 32);
	assert_visible(&screen, "render     all");
	assert_visible(&screen, "selector class:App");
	assert_visible(&screen, "class App {}");

	harness.press(KeyCode::Char('a'));
	harness.press(KeyCode::Enter);
	harness.press(KeyCode::Down);
	harness.press(KeyCode::Down);
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "[v1]");
	assert_visible(&screen, "test-map");
	assert_visible(&screen, "workspace/views/test-map");
	assert_visible(&screen, "test-entry owns 1");

	harness.press(KeyCode::Char('v'));
	let screen = harness.render_text(120, 32);
	assert_visible(&screen, "overview");
}

#[test]
fn notes_on_selected_symbol_are_visible_in_navigator_and_outline() {
	let (mut harness, fixture) = load_notes_fixture();
	harness.search("App");
	let moniker = selected_symbol_identity(&harness.app);
	write_note_fixture(&fixture, &moniker);
	handle_store_event_sync(&mut harness.app, WorkspaceLiveEvent::Notes);

	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "[!1]");
	assert_visible(&screen, "notes");
	assert_visible(&screen, "pending todo  Check App");
	assert_visible(&screen, "Agent should inspect App before editing.");
	assert_visible(&screen, "selected");
	assert_visible(&screen, "moniker");
}

#[test]
fn note_editor_creates_updates_and_deletes_notes_from_tui() {
	let (mut harness, fixture) = load_notes_fixture();
	harness.search("App");

	harness.press(KeyCode::Char('n'));
	harness.press(KeyCode::Right);
	harness.press(KeyCode::Down);
	harness.type_text("Review App");
	harness.press(KeyCode::Tab);
	harness.type_text("Check constructor behavior.");
	harness.press(KeyCode::Enter);
	harness.type_text("Keep body multiline.");
	press_with_modifiers(&mut harness.app, KeyCode::Char('o'), KeyModifiers::CONTROL);
	press_with_modifiers(&mut harness.app, KeyCode::Char('s'), KeyModifiers::CONTROL);

	let notes_path = fixture.join(".code-moniker/notes.toml");
	let notes = std::fs::read_to_string(&notes_path).expect("notes saved");
	assert!(notes.contains("Review App"), "{notes}");
	assert!(notes.contains("kind = \"gotcha\""), "{notes}");
	assert!(notes.contains("Check constructor behavior."), "{notes}");
	assert!(notes.contains("Keep body multiline."), "{notes}");
	assert!(notes.contains("status = \"ongoing\""), "{notes}");

	let screen = harness.render_text(120, 32);
	assert_visible(&screen, "[!1]");
	assert_visible(&screen, "Review App");

	harness.press(KeyCode::Char('8'));
	let screen = harness.render_text(120, 32);
	assert_visible(&screen, "notes lens");
	assert_visible(&screen, "ongoing");
	assert_visible(&screen, "Review App");

	harness.press(KeyCode::Char('n'));
	harness.press(KeyCode::Down);
	press_with_modifiers(&mut harness.app, KeyCode::Char('u'), KeyModifiers::CONTROL);
	harness.type_text("Edited App note");
	press_with_modifiers(&mut harness.app, KeyCode::Char('s'), KeyModifiers::CONTROL);

	let notes = std::fs::read_to_string(&notes_path).expect("notes updated");
	assert!(notes.contains("Edited App note"), "{notes}");
	assert!(!notes.contains("Review App"), "{notes}");

	harness.press(KeyCode::Char('8'));
	harness.press(KeyCode::Char('n'));
	press_with_modifiers(&mut harness.app, KeyCode::Char('d'), KeyModifiers::CONTROL);
	press_with_modifiers(&mut harness.app, KeyCode::Char('d'), KeyModifiers::CONTROL);

	let notes = std::fs::read_to_string(&notes_path).expect("notes deleted");
	assert!(!notes.contains("Edited App note"), "{notes}");
	let screen = harness.render_text(120, 32);
	assert_visible(&screen, "no notes");
}

#[test]
fn note_editor_abandons_empty_draft_without_creating_note() {
	let (mut harness, fixture) = load_notes_fixture();
	harness.search("App");

	harness.press(KeyCode::Char('N'));
	harness.press(KeyCode::Esc);

	assert!(
		!fixture.join(".code-moniker/notes.toml").exists(),
		"empty draft should not create notes file"
	);
	let screen = harness.render_text(120, 32);
	assert_hidden(&screen, "[!1]");
}

#[test]
fn note_editor_can_target_any_navigator_node() {
	let (mut harness, fixture) = load_notes_fixture();

	harness.press(KeyCode::Char('n'));
	let editor = harness.render_text(140, 36);
	assert_visible(&editor, "note editor");
	assert_visible(&editor, "[x] todo");
	assert_visible(&editor, "directory java/src/main/java");
	harness.press(KeyCode::Down);
	harness.type_text("Project note");
	press_with_modifiers(&mut harness.app, KeyCode::Char('s'), KeyModifiers::CONTROL);

	let notes = std::fs::read_to_string(fixture.join(".code-moniker/notes.toml"))
		.expect("navigation note saved");
	assert!(notes.contains("Project note"), "{notes}");
	assert!(notes.contains("workspace/navigation/"), "{notes}");

	harness.press(KeyCode::Char('2'));
	let outline = harness.render_text(160, 40);
	assert_visible(&outline, "[!1]");
	assert_visible(&outline, "notes");
	assert_visible(&outline, "Project note");

	harness.press(KeyCode::Char('8'));
	let lens = harness.render_text(160, 40);
	assert_visible(&lens, "notes lens");
	assert_visible(&lens, "Project note");
	assert_visible(&lens, "navigation explorer:dir:java:src/main/java");
}

#[test]
fn malformed_notes_file_is_visible_in_notes_surfaces() {
	let (mut harness, fixture) = load_notes_fixture();
	harness.search("App");
	write_under(&fixture, ".code-moniker/notes.toml", "[[notes]\n");
	handle_store_event_sync(&mut harness.app, WorkspaceLiveEvent::Notes);

	let outline = harness.render_text(140, 36);
	assert_visible(&outline, "notes unavailable");

	harness.press(KeyCode::Char('8'));
	let lens = harness.render_text(140, 36);
	assert_visible(&lens, "notes lens");
	assert_visible(&lens, "notes unavailable");
}

#[test]
fn malformed_notes_file_is_visible_after_workspace_replacement() {
	let (mut harness, fixture) = load_notes_fixture();
	write_under(&fixture, ".code-moniker/notes.toml", "[[notes]\n");
	let options = crate::ui::app::store_options(&harness.app);
	let (store, cache) = load_local_workspace(&options).expect("reload workspace");
	crate::ui::app::replace_store(&mut harness.app, store, cache, options);

	harness.press(KeyCode::Char('8'));
	let screen = harness.render_text(140, 36);
	assert_visible(&screen, "notes lens");
	assert_visible(&screen, "notes unavailable");
}

#[test]
fn notes_lens_flags_orphan_notes() {
	let (mut harness, fixture) = load_notes_fixture();
	write_under(
		&fixture,
		".code-moniker/notes.toml",
		r#"
		[[notes]]
		id = "note_orphan"
		moniker = "java:missing.type:Missing"
		kind = "gotcha"
		status = "pending"
		title = "Missing target"
		body = "The referenced moniker is gone."
		created_by = "user"
		created_at = "2026-06-02T00:00:00Z"
		updated_at = "2026-06-02T00:00:00Z"
		"#,
	);
	handle_store_event_sync(&mut harness.app, WorkspaceLiveEvent::Notes);

	harness.press(KeyCode::Char('8'));
	let screen = harness.render_text(160, 40);
	assert_visible(&screen, "notes lens");
	assert_visible(&screen, "orphan");
	assert_visible(&screen, "Missing target");
}

#[test]
fn multiproject_header_search_filters_visible_symbols() {
	let mut harness = TuiAcceptance::load_multiproject();

	harness.search("RiskPolicy");
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "search");
	assert_visible(&screen, "RiskPolicy");
	assert_visible(&screen, "filtered");
	assert_visible(&screen, "common-lib");
}

#[test]
fn multiproject_check_panel_reports_clean_rule_run() {
	let mut harness = TuiAcceptance::load_multiproject_with_profile(Some("code"));

	harness.press(KeyCode::Char('c'));
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "check");
	assert_visible(&screen, "check summary");
	assert_visible(&screen, "files");
	assert_visible(&screen, "29");
	assert_visible(&screen, "flagged");
	assert_visible(&screen, "violations");
	assert_visible(&screen, "0");
	assert_visible(&screen, "check complete: 0 violation(s) across 0 file(s)");
}

#[test]
fn multiproject_usage_lens_requires_selected_declaration() {
	let mut harness = TuiAcceptance::load_multiproject();

	harness.press(KeyCode::Char('u'));
	let screen = harness.render_text(120, 32);

	assert_visible(&screen, "overview");
	assert_visible(&screen, "select a declaration before focusing usages");
}

#[test]
fn multiproject_usage_lens_shows_cross_module_references() {
	let mut harness = TuiAcceptance::load_multiproject();

	harness.navigate_to_risk_policy_class();
	harness.press(KeyCode::Char('u'));
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "usages");
	assert_visible(&screen, "usage focus");
	assert_visible(&screen, "RiskPolicy");
	assert_visible(&screen, "refs");
	assert_visible(&screen, "34");
	assert_visible(&screen, "contexts");
	assert_visible(&screen, "14");
	assert_visible(&screen, "references");
	assert_visible(&screen, "OrderApplication");
	assert_visible(&screen, "LoyaltyApplication");
	assert_visible(&screen, "CustomerConfiguration");
	assert_visible(
		&screen,
		"usage lens for RiskPolicy: 34 reference(s), 14 navigable context(s)",
	);
}

#[test]
fn usage_lens_tab_cycle_visits_panel_before_usage_lens_and_backtab_reverses() {
	let mut harness = TuiAcceptance::load_multiproject();

	harness.navigate_to_risk_policy_class();
	harness.press(KeyCode::Char('u'));
	assert_eq!(focus_region(&harness.app), FocusRegion::UsageLens);

	harness.press(KeyCode::Tab);
	assert_eq!(focus_region(&harness.app), FocusRegion::Navigator);

	harness.press(KeyCode::Tab);
	assert_eq!(focus_region(&harness.app), FocusRegion::Panel);

	harness.press(KeyCode::Tab);
	assert_eq!(focus_region(&harness.app), FocusRegion::UsageLens);

	harness.press(KeyCode::BackTab);
	assert_eq!(focus_region(&harness.app), FocusRegion::Panel);

	harness.press(KeyCode::BackTab);
	assert_eq!(focus_region(&harness.app), FocusRegion::Navigator);

	harness.press(KeyCode::BackTab);
	assert_eq!(focus_region(&harness.app), FocusRegion::UsageLens);
}

#[test]
fn multiproject_usage_lens_shows_cross_project_java_interface_implementations() {
	let mut harness = TuiAcceptance::load_multiproject();

	harness.search("CustomerResolver");
	harness.press(KeyCode::Down);
	harness.press(KeyCode::Char('u'));
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "usages");
	assert_visible(&screen, "usage focus");
	assert_visible(&screen, "CustomerResolver");
	assert_visible(&screen, "references");
	assert_visible(&screen, "CustomerDirectory");
	assert_visible(&screen, "SpringCustomerRepository");
	assert_visible(&screen, "implements");
}

#[test]
fn rust_usage_lens_shows_imported_const_contexts_in_usage_navigator() {
	let mut harness = TuiAcceptance::load_rust_multiproject();

	harness.search("DEFAULT_REGION");
	let filtered = harness.render_text(160, 45);
	assert_visible(
		&filtered,
		"const public common-model/src/lib.rs/DEFAULT_REGION",
	);
	assert_visible(
		&filtered,
		"path public order-service/src/lib.rs/DEFAULT_REGION",
	);

	harness.press(KeyCode::Down);
	harness.press(KeyCode::Char('u'));
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "usages DEFAULT_REGION");
	assert_visible(&screen, "DEFAULT_REGION");
	assert_visible(&screen, "reexported_region_code");
	assert_visible(
		&screen,
		"fn public rs/order-service/src/lib.rs/reexported_region_code(",
	);
	assert_visible(
		&screen,
		"usage lens for DEFAULT_REGION: 2 reference(s), 1 navigable context(s)",
	);
}

fn multiproject_fixture() -> PathBuf {
	let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MULTIPROJECT_FIXTURE);
	path.canonicalize().unwrap_or_else(|error| {
		panic!(
			"missing multiproject fixture at {}: {error}",
			path.display()
		)
	})
}

fn rust_multiproject_fixture() -> PathBuf {
	let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(RUST_MULTIPROJECT_FIXTURE);
	path.canonicalize().unwrap_or_else(|error| {
		panic!(
			"missing Rust multiproject fixture at {}: {error}",
			path.display()
		)
	})
}

fn app_config(rules: PathBuf, profile: Option<String>, debug: bool) -> AppConfig {
	AppConfig {
		scheme: "default".to_string(),
		rules,
		profile,
		debug,
		live_refresh: crate::args::LiveRefresh::Auto,
	}
}

fn multiproject_project_paths() -> Vec<PathBuf> {
	let fixture = multiproject_fixture();
	[
		"billing-service",
		"common-lib",
		"inventory-service",
		"loyalty-platform",
		"order-service",
		"spring-edge",
	]
	.into_iter()
	.map(|name| fixture.join(name))
	.collect()
}

fn load_views_fixture() -> TuiAcceptance {
	let cache_dir = tempfile::tempdir().expect("cache dir");
	let fixture = cache_dir.path().join("fixture");
	write_views_fixture(&fixture);
	let opts = SessionOptions {
		paths: vec![fixture.clone()],
		project: Some("views-fixture".to_string()),
		cache_dir: Some(cache_dir.path().to_path_buf()),
	};
	let (store, cache) = load_local_workspace(&opts).expect("load views fixture");
	let app = new_app(
		store,
		cache,
		opts,
		app_config(fixture.join(".code-moniker.toml"), None, false),
	);
	TuiAcceptance {
		app,
		_cache_dir: cache_dir,
	}
}

fn load_rust_module_reexport_fixture() -> TuiAcceptance {
	let cache_dir = tempfile::tempdir().expect("cache dir");
	let fixture = cache_dir.path().join("fixture");
	write_rust_module_reexport_fixture(&fixture);
	let opts = SessionOptions {
		paths: vec![fixture.clone()],
		project: Some("rust-reexport-fixture".to_string()),
		cache_dir: Some(cache_dir.path().to_path_buf()),
	};
	let (store, cache) = load_local_workspace(&opts).expect("load Rust reexport fixture");
	let app = new_app(
		store,
		cache,
		opts,
		app_config(fixture.join(".code-moniker.toml"), None, false),
	);
	TuiAcceptance {
		app,
		_cache_dir: cache_dir,
	}
}

fn load_notes_fixture() -> (TuiAcceptance, PathBuf) {
	let cache_dir = tempfile::tempdir().expect("cache dir");
	let fixture = cache_dir.path().join("fixture");
	write_under(&fixture, "src/main/java/App.java", "class App {}\n");
	let opts = SessionOptions {
		paths: vec![fixture.clone()],
		project: Some("notes-fixture".to_string()),
		cache_dir: Some(cache_dir.path().to_path_buf()),
	};
	let (store, cache) = load_local_workspace(&opts).expect("load notes fixture");
	let app = new_app(
		store,
		cache,
		opts,
		app_config(fixture.join(".code-moniker.toml"), None, false),
	);
	(
		TuiAcceptance {
			app,
			_cache_dir: cache_dir,
		},
		fixture,
	)
}

fn write_rust_module_reexport_fixture(fixture: &Path) {
	write_under(fixture, "src/lib.rs", "pub mod api;\n");
	write_under(fixture, "src/api.rs", "pub struct Api;\n");
}

fn write_note_fixture(fixture: &Path, moniker: &str) {
	write_under(
		fixture,
		".code-moniker/notes.toml",
		&format!(
			r#"
			[[notes]]
			id = "note_app"
			moniker = "{moniker}"
			kind = "todo"
			status = "pending"
			title = "Check App"
			body = "Agent should inspect App before editing."
			created_by = "user"
			created_at = "2026-06-02T00:00:00Z"
			updated_at = "2026-06-02T00:00:00Z"
			"#
		),
	);
}

fn selected_symbol_identity(app: &App) -> String {
	let loc = crate::ui::app::selected(app).expect("selected symbol");
	crate::ui::workspace_read::symbol_summary(crate::ui::app::store(app), &loc).identity
}

fn write_views_fixture(fixture: &Path) {
	write_under(fixture, "src/main/java/App.java", "class App {}\n");
	write_under(fixture, "src/test/java/AppTest.java", "class AppTest {}\n");
	write_under(
		fixture,
		"src/main/java/code-moniker.fragment.toml",
		r#"
		fragment = "fixture-java"

		[[views]]
		id = "fixture-map"
		title = "Fixture map"
		scope = "."
		intent = "Keep the fixture boundary visible."

		[[views.boundaries]]
		id = "entry"
		owns = ["application entry"]
		forbids = ["runtime ownership"]
		symbols = ["class:App"]
		"#,
	);
	write_under(
		fixture,
		"src/test/java/code-moniker.fragment.toml",
		r#"
		fragment = "fixture-test-java"

		[[views]]
		id = "test-map"
		title = "Test map"
		scope = "."
		intent = "Keep the test boundary visible."

		[[views.boundaries]]
		id = "test-entry"
		owns = ["test entrypoint"]
		forbids = ["production ownership"]
		symbols = ["class:AppTest"]
		"#,
	);
}

fn write_under(root: &Path, rel: &str, contents: &str) {
	let path = root.join(rel);
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).expect("mkdir");
	}
	std::fs::write(path, contents).expect("write fixture");
}

fn press_with_modifiers(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
	handle_key(app, KeyEvent::new(code, modifiers)).expect("handle key");
}

fn navigator_border_width(screen: &str) -> usize {
	screen
		.lines()
		.find_map(|line| {
			line.find("┌navigator")
				.and_then(|start| line[start..].find('┐').map(|end| end + 1))
		})
		.expect("navigator border width")
}

fn assert_visible(screen: &str, expected: &str) {
	if !screen.contains(expected) {
		panic!("expected visible text `{expected}` in TUI screen:\n{screen}");
	}
}

fn assert_hidden(screen: &str, unexpected: &str) {
	if screen.contains(unexpected) {
		panic!("expected hidden text `{unexpected}` in TUI screen:\n{screen}");
	}
}
