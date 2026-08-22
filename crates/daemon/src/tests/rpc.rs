use super::*;

#[tokio::test]
async fn rpc_syntax_parse_does_not_wait_for_the_workspace_lock() {
	let temp = tempfile::tempdir().expect("tempdir");
	let (events, _) = tokio::sync::broadcast::channel(16);
	let service = test_rpc_service(
		WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
		vec![temp.path().to_path_buf()],
		events,
	);
	let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));

	let response = service
		.dispatch(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::SyntaxParse(code_moniker_query::SyntaxParseQuery {
				language: "rs".to_string(),
				source: "fn answer() -> u32 { 42 }".to_string(),
				uri: None,
				max_depth: 6,
				max_nodes: 100,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			}),
		))))
		.await
		.expect("RPC dispatch");
	release_lock.send(()).expect("release workspace lock");
	lock_holder.join().expect("workspace lock holder");

	let ProtocolResponse::Query(response) = response else {
		panic!("expected stateless syntax response, got {response:?}");
	};
	assert!(matches!(response.result, QueryResult::SyntaxTree(_)));
}

#[tokio::test]
async fn rpc_exclusive_requests_queue_instead_of_reporting_workspace_loading() {
	let temp = tempfile::tempdir().expect("tempdir");
	let (events, _) = tokio::sync::broadcast::channel(16);
	let service = Arc::new(test_rpc_service(
		WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon"),
		vec![temp.path().to_path_buf()],
		events,
	));
	let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));
	let mut pending = tokio::spawn({
		let service = Arc::clone(&service);
		async move {
			service
				.dispatch(ProtocolRequest::Command(CommandRequest {
					command: Command::WorkspaceRefresh,
				}))
				.await
		}
	});

	assert!(
		tokio::time::timeout(std::time::Duration::from_millis(50), &mut pending)
			.await
			.is_err(),
		"the request should wait for the active workspace mutation"
	);
	release_lock.send(()).expect("release workspace lock");
	lock_holder.join().expect("workspace lock holder");

	let response = pending.await.expect("dispatch task").expect("RPC dispatch");
	assert!(matches!(response, ProtocolResponse::Command(_)));
}

#[tokio::test]
async fn rpc_stale_reads_use_the_published_snapshot_during_workspace_mutation() {
	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Customer;\n").expect("seed fixture");
	let mut daemon = WorkspaceDaemon::new(vec![temp.path().to_path_buf()]).expect("daemon");
	let refreshed = daemon.handle_protocol(ProtocolRequest::Command(CommandRequest {
		command: Command::WorkspaceRefresh,
	}));
	assert!(matches!(refreshed, ProtocolResponse::Command(_)));
	let (events, _) = tokio::sync::broadcast::channel(16);
	let service = test_rpc_service(daemon, vec![temp.path().to_path_buf()], events);
	{
		let daemon = service.daemon.lock().expect("workspace lock");
		publish_current_snapshot(&daemon, &service.published);
	}
	let (release_lock, lock_holder) = hold_workspace_lock(Arc::clone(&service.daemon));

	let response = tokio::time::timeout(
		std::time::Duration::from_millis(100),
		service.dispatch(ProtocolRequest::Query(Box::new(QueryRequest {
			query: Query::SymbolSearch(code_moniker_query::SymbolSearchQuery {
				name: Some("Customer".to_string()),
				..Default::default()
			}),
			consistency: Consistency::StaleOk,
			page: Page::default(),
		}))),
	)
	.await
	.expect("stale read must not wait for the workspace mutation")
	.expect("RPC dispatch");
	let ProtocolResponse::Query(response) = response else {
		panic!("expected symbol response, got {response:?}");
	};
	let QueryResult::SymbolList(symbols) = response.result else {
		panic!("expected symbol list, got {:?}", response.result);
	};
	assert_eq!(symbols.total, 1);

	let response = service
		.dispatch(ProtocolRequest::Query(Box::new(QueryRequest::new(
			Query::WorkspaceStatus,
		))))
		.await
		.expect("workspace status");
	let ProtocolResponse::Query(response) = response else {
		panic!("expected workspace status, got {response:?}");
	};
	let QueryResult::WorkspaceStatus(status) = response.result else {
		panic!("expected workspace status, got {:?}", response.result);
	};
	assert_eq!(status.phase, WorkspacePhase::Refreshing);
	assert!(status.stale);
	assert!(status.stale_summary.contains("refresh in progress"));
	release_lock.send(()).expect("release workspace lock");
	lock_holder.join().expect("workspace lock holder");
}

#[tokio::test]
async fn rpc_server_answers_query_and_streams_events() {
	use code_moniker_query::DaemonRpcClient;
	use code_moniker_query::{WorkspaceEventDto, WorkspaceEventKind};
	use jsonrpsee::ws_client::WsClientBuilder;

	let temp = tempfile::tempdir().expect("tempdir");
	fs::write(temp.path().join("lib.rs"), "pub struct Customer;\n").expect("seed fixture");
	let (events, _) = tokio::sync::broadcast::channel(16);
	let daemon = WorkspaceDaemon::with_events(
		DaemonWorkspaceConfig {
			roots: vec![temp.path().display().to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
		},
		events.clone(),
	)
	.expect("daemon");
	let build = producer_identity();
	let mut service = test_rpc_service(daemon, vec![temp.path().to_path_buf()], events.clone());
	service.handshake.build = build.clone();
	let daemon_handle = Arc::clone(&service.daemon);
	let server = Server::builder()
		.build("127.0.0.1:0")
		.await
		.expect("server binds");
	let addr = server.local_addr().expect("addr");
	let handle = server.start(service.into_rpc());

	let client = WsClientBuilder::default()
		.build(format!("ws://{addr}"))
		.await
		.expect("client connects");

	let (release_lock, lock_holder) = hold_workspace_lock(daemon_handle);
	let response = tokio::time::timeout(
		std::time::Duration::from_secs(1),
		client.query(QueryRequest::new(Query::SyntaxParse(
			code_moniker_query::SyntaxParseQuery {
				language: "rs".to_string(),
				source: "fn rpc_answer() -> u32 { 42 }".to_string(),
				uri: None,
				max_depth: 6,
				max_nodes: 100,
				named_only: true,
				include_text: false,
				max_text_chars: 80,
			},
		))),
	)
	.await
	.expect("syntax.parse must bypass the held workspace lock")
	.expect("syntax.parse RPC");
	release_lock.send(()).expect("release workspace lock");
	lock_holder.join().expect("workspace lock holder");
	assert!(matches!(response.result, QueryResult::SyntaxTree(_)));

	let response = client
		.query(QueryRequest::new(Query::WorkspaceStatus))
		.await
		.expect("query");
	let QueryResult::WorkspaceStatus(status) = response.result else {
		panic!("expected workspace status")
	};
	assert_eq!(status.producer, build);

	let mut subscription = client.subscribe_events().await.expect("subscribe");
	let replaced = client
		.command(CommandRequest {
			command: Command::WorkspaceSourceSetReplace {
				source_set: WorkspaceSourceSetDto {
					srcset: "generated".to_string(),
					revision: Some("rpc-1".to_string()),
					documents: vec![WorkspaceSourceDocumentDto {
						uri: "generated.rs".to_string(),
						language: "rs".to_string(),
						content: "pub struct RpcGenerated;\n".to_string(),
					}],
				},
			},
		})
		.await
		.expect("replace source set over RPC");
	let refreshed = subscription
		.next()
		.await
		.expect("refreshed event present")
		.expect("refreshed event decoded");
	assert_eq!(refreshed.kind, WorkspaceEventKind::Refreshed);
	assert_eq!(refreshed.generation, replaced.generation);

	events
		.send(WorkspaceEventDto {
			kind: WorkspaceEventKind::Notes,
			generation: None,
			stale_summary: None,
		})
		.expect("publish event");
	let event = subscription
		.next()
		.await
		.expect("event present")
		.expect("event decoded");
	assert_eq!(event.kind, WorkspaceEventKind::Notes);

	handle.stop().ok();
}
