use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::session::SessionOptions;
use crate::ui::app::{
	App, AppConfig, FocusRegion, focus_region, handle_key, new_app, toggle_selected_nav,
};
use crate::ui::render::view;
use crate::ui::workspace_read::{load_local_file_catalog, load_local_workspace};

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
	assert_visible(&screen, "navigator 27 files");
	assert_visible(&screen, "java");
	assert_visible(&screen, "billing-service");
	assert_visible(&screen, "common-lib");
	assert_visible(&screen, "inventory-service");
	assert_visible(&screen, "loyalty-platform");
	assert_visible(&screen, "order-service");
	assert_visible(&screen, "spring-edge");
	assert_visible(&screen, "files      27");
	assert_visible(&screen, "defs       0");
	assert_visible(&screen, "refs       0");
}

#[test]
fn multiproject_multiple_paths_behave_like_virtual_parent() {
	let mut harness = TuiAcceptance::load_multiproject_paths(multiproject_project_paths(), None);
	toggle_selected_nav(&mut harness.app);
	let screen = harness.render_text(160, 45);

	assert_visible(&screen, "multiprojet");
	assert_visible(&screen, "navigator 27 files");
	assert_visible(&screen, "java");
	assert_visible(&screen, "billing-service");
	assert_visible(&screen, "common-lib");
	assert_visible(&screen, "inventory-service");
	assert_visible(&screen, "loyalty-platform");
	assert_visible(&screen, "order-service");
	assert_visible(&screen, "spring-edge");
	assert_visible(&screen, "files      27");
	assert_visible(&screen, "defs       236");
	assert_visible(&screen, "refs       538");
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
	assert_visible(&screen, "entry owns 1");
	assert_hidden(&screen, "test-map");

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
	assert_visible(&screen, "27");
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
	assert_visible(&screen, "28");
	assert_visible(&screen, "contexts");
	assert_visible(&screen, "8");
	assert_visible(&screen, "references");
	assert_visible(&screen, "OrderApplication");
	assert_visible(&screen, "LoyaltyApplication");
	assert_visible(&screen, "SpringCustomerService");
	assert_visible(
		&screen,
		"usage lens for RiskPolicy: 28 reference(s), 8 navigable context(s)",
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

fn write_rust_module_reexport_fixture(fixture: &Path) {
	write_under(fixture, "src/lib.rs", "pub mod api;\n");
	write_under(fixture, "src/api.rs", "pub struct Api;\n");
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
