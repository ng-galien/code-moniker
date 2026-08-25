use super::*;

fn read_view_detail(daemon: &mut WorkspaceDaemon, id: &str) -> Box<ViewDetailResult> {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::ViewRead(ViewReadQuery {
			uri: format!("workspace/views/{id}"),
			scheme: None,
			context_lines: 0,
			include_code: false,
		}),
	))));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected view detail response, got {response:?}");
	};
	let QueryResult::ViewRead(ViewReadResult::Detail(detail)) = response.result else {
		panic!("expected view detail, got {:?}", response.result);
	};
	detail
}

#[test]
fn view_detail_loads_rules_from_the_canonical_taxonomy_config() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::create_dir_all(temp.path().join("src")).expect("create source directory");
	fs::write(temp.path().join("src/lib.rs"), "pub fn entry() {}\n").expect("write source");
	fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
default_rules = false

[rules.taxonomy]
patterns = ["ownership"]
components = ["workspace"]

[[rust.fn.where]]
id = "workspace-ownership-keeps-entry"
expr = "name = 'entry'"
message = "Workspace entry must remain present."
rationale = "The canonical view must expose the project rule rationale."

[[views]]
id = "workspace-boundaries"
scope = "."

[[views.boundaries]]
id = "workspace"
owns = ["the workspace entry point"]
rules = ["workspace-ownership-keeps-entry"]
"#,
	)
	.expect("write canonical rules and view config");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));

	let detail = read_view_detail(&mut daemon, "workspace-boundaries");

	assert_eq!(detail.id, "workspace-boundaries");
	assert_eq!(detail.rules.len(), 1);
	assert_eq!(
		detail.rules[0].id,
		"rust.fn.workspace-ownership-keeps-entry"
	);
	assert_eq!(
		detail.rules[0].rationale.as_deref(),
		Some("The canonical view must expose the project rule rationale.")
	);
	assert!(detail.boundaries[0].rule_refs[0].present);
}

#[test]
fn view_detail_uses_the_daemon_config_root_for_multi_root_rules() {
	let temp = tempfile::tempdir().expect("tempdir");
	let first = temp.path().join("crates/first");
	let second = temp.path().join("crates/second");
	fs::create_dir_all(first.join("src")).expect("create first source directory");
	fs::create_dir_all(second.join("src")).expect("create second source directory");
	fs::write(first.join("src/lib.rs"), "pub fn first_entry() {}\n").expect("write first");
	fs::write(second.join("src/lib.rs"), "pub fn second_entry() {}\n").expect("write second");
	fs::write(
		temp.path().join(".code-moniker.toml"),
		r#"
default_rules = false

[rules.taxonomy]
patterns = ["ownership"]
components = ["workspace"]

[[rust.fn.where]]
id = "workspace-ownership-keeps-entries"
expr = "name =~ 'entry'"
message = "Workspace entries must remain present."
rationale = "The daemon config root owns rule vocabulary above all selected roots."
"#,
	)
	.expect("write multi-root canonical config");
	fs::write(
		first.join("code-moniker.fragment.toml"),
		r#"
fragment = "first"

[[views]]
id = "multi-root-boundaries"
scope = "."

[[views.boundaries]]
id = "workspace"
owns = ["the selected workspace roots"]
rules = ["workspace-ownership-keeps-entries"]
"#,
	)
	.expect("write fragment view");
	let mut daemon = WorkspaceDaemon::new(vec![first, second]).expect("daemon");
	let refresh = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refresh, ProtocolResponse::Command(_)));

	let detail = read_view_detail(&mut daemon, "multi-root-boundaries");

	assert_eq!(detail.rules.len(), 1);
	assert_eq!(
		detail.rules[0].rationale.as_deref(),
		Some("The daemon config root owns rule vocabulary above all selected roots.")
	);
	assert!(detail.boundaries[0].rule_refs[0].present);
}
