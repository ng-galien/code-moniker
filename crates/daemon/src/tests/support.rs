use super::*;

pub(super) fn test_rpc_service(
	daemon: WorkspaceDaemon,
	roots: Vec<PathBuf>,
	events: tokio::sync::broadcast::Sender<WorkspaceEventDto>,
) -> DaemonRpcService {
	DaemonRpcService {
		daemon: Arc::new(Mutex::new(daemon)),
		published: Arc::new(RwLock::new(None)),
		lifecycle: Arc::new(RwLock::new(WorkspaceLifecycle::ready())),
		roots: Arc::from(roots),
		events,
		shutdown: Arc::new(tokio::sync::Notify::new()),
		handshake: HandshakeResponse {
			protocol_version: code_moniker_query::PROTOCOL_VERSION,
			daemon_version: "test".to_string(),
			build: producer_identity(),
			workspace_root: "test".to_string(),
			workspace_roots: Vec::new(),
			capabilities: CapabilitySet::default(),
		},
	}
}

pub(super) fn hold_workspace_lock(
	daemon: Arc<Mutex<WorkspaceDaemon>>,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
	let (locked_tx, locked_rx) = std::sync::mpsc::channel();
	let (release_tx, release_rx) = std::sync::mpsc::channel();
	let holder = std::thread::spawn(move || {
		let _workspace_lock = daemon.lock().expect("workspace lock");
		locked_tx.send(()).expect("announce workspace lock");
		let _ = release_rx.recv();
	});
	locked_rx.recv().expect("wait for workspace lock");
	(release_tx, holder)
}

pub(super) fn search_symbols(daemon: &mut WorkspaceDaemon, text: &str) -> QueryResult {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
			workspace: None,
			text: Some(text.to_string()),
			path: Vec::new(),
			lang: Vec::new(),
			kind: Vec::new(),
			shape: Vec::new(),
			name: None,
			include_non_navigable: false,
			include_code: false,
			context_lines: 0,
			projection: Vec::new(),
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	match response {
		ProtocolResponse::Query(query) => query.result,
		other => panic!("expected query response, got {other:?}"),
	}
}

pub(super) fn search_symbols_named(daemon: &mut WorkspaceDaemon, name: &str) -> QueryResult {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
			name: Some(format!("^{}$", regex::escape(name))),
			include_code: true,
			context_lines: 0,
			..Default::default()
		}),
		consistency: code_moniker_query::Consistency::Current,
		page: Page::default(),
	})));
	match response {
		ProtocolResponse::Query(query) => query.result,
		other => panic!("expected query response, got {other:?}"),
	}
}

pub(super) fn replace_source_set(
	daemon: &mut WorkspaceDaemon,
	source_set: WorkspaceSourceSetDto,
) -> CommandResponse {
	let response = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceSourceSetReplace { source_set },
	}));
	let ProtocolResponse::Command(response) = response else {
		panic!("expected source-set replacement, got {response:?}");
	};
	response
}

pub(super) fn database_source_set(
	revision: &str,
	documents: &[(&str, &str)],
) -> WorkspaceSourceSetDto {
	WorkspaceSourceSetDto {
		srcset: "database".to_string(),
		revision: Some(revision.to_string()),
		documents: documents
			.iter()
			.map(|(uri, content)| WorkspaceSourceDocumentDto {
				uri: (*uri).to_string(),
				language: "sql".to_string(),
				content: (*content).to_string(),
			})
			.collect(),
	}
}

pub(super) fn incoming_usage_files(daemon: &mut WorkspaceDaemon, uri: &str) -> BTreeSet<String> {
	let response = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest {
		query: Query::SymbolUsages(SymbolUsagesQuery {
			workspace: None,
			uri: uri.to_string(),
			direction: UsageDirection::Incoming,
			path: Vec::new(),
			lang: Vec::new(),
			include_descendants: false,
			projection: Vec::new(),
		}),
		consistency: Consistency::Current,
		page: Page::default(),
	})));
	let ProtocolResponse::Query(response) = response else {
		panic!("expected usage response");
	};
	let QueryResult::SymbolUsages(result) = response.result else {
		panic!("expected symbol usages result");
	};
	result.rows.into_iter().map(|usage| usage.file).collect()
}

pub(super) fn remove_source_set(daemon: &mut WorkspaceDaemon, srcset: &str) -> CommandResponse {
	let response = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceSourceSetRemove {
			srcset: srcset.to_string(),
		},
	}));
	let ProtocolResponse::Command(response) = response else {
		panic!("expected source-set removal, got {response:?}");
	};
	response
}

pub(super) fn assert_symbol_total(daemon: &mut WorkspaceDaemon, text: &str, expected: usize) {
	let QueryResult::SymbolList(symbols) = search_symbols_named(daemon, text) else {
		panic!("expected symbol list");
	};
	assert_eq!(symbols.total, expected, "{symbols:?}");
}

pub(super) fn assert_memory_root_absent_from_rules(daemon: &mut WorkspaceDaemon, rules: &Path) {
	let listed = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::RulesList(code_moniker_query::RulesListQuery {
			rules: Some(rules.display().to_string()),
			..Default::default()
		}),
	))));
	let ProtocolResponse::Query(listed) = listed else {
		panic!("expected rules list, got {listed:?}");
	};
	let QueryResult::RulesList(listed) = listed.result else {
		panic!("expected rules list result, got {:?}", listed.result);
	};
	assert!(
		listed.roots.iter().all(|root| root != MEMORY_SOURCE_ROOT),
		"removed memory roots must disappear from rules.list: {listed:?}"
	);

	let checked = daemon.handle_protocol(ProtocolRequest::Query(Box::new(QueryRequest::new(
		Query::RulesCheck(RulesCheckQuery {
			workspace: None,
			profile: None,
			rules: Some(rules.display().to_string()),
			file: Vec::new(),
			report: true,
		}),
	))));
	let ProtocolResponse::Query(checked) = checked else {
		panic!("expected rules check, got {checked:?}");
	};
	let QueryResult::RulesCheck(checked) = checked.result else {
		panic!("expected rules check result, got {:?}", checked.result);
	};
	assert!(
		checked
			.roots
			.iter()
			.all(|root| root.root != MEMORY_SOURCE_ROOT),
		"removed memory roots must disappear from rules.check: {checked:?}"
	);
}
