use std::io::Write;

use code_moniker_daemon_client::{DaemonClient, config_roots, daemon_workspace_config};
use tracing::info;

use crate::args::{McpArgs, McpTransport};
use crate::mcp::{DaemonRuntime, McpContext};
use crate::session::SessionOptions;
use crate::{DEFAULT_SCHEME, Exit, mcp};

pub(crate) fn run<W1: Write, W2: Write>(args: &McpArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	match run_inner(args, stdout, stderr) {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W1: Write, W2: Write>(
	args: &McpArgs,
	_stdout: &mut W1,
	_stderr: &mut W2,
) -> anyhow::Result<()> {
	init_logging();
	let scheme = args.scheme.as_deref().unwrap_or(DEFAULT_SCHEME).to_string();
	let daemon_config = daemon_workspace_config(
		&args.paths,
		args.project.to_owned(),
		args.cache.to_owned(),
		Some(live_refresh_label(args).to_string()),
	)?;
	let opts = SessionOptions {
		paths: config_roots(&daemon_config),
		project: args.project.to_owned(),
		cache_dir: args.cache.to_owned(),
	};
	let daemon_keepalive = match args.transport {
		McpTransport::Http => {
			let client = DaemonClient::connect_or_start_config(daemon_config.clone())?;
			DaemonRuntime::client(client, daemon_config)
		}
		McpTransport::Stdio => in_process_runtime(daemon_config)?,
	};
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("code-moniker-mcp")
		.build()?;
	let result = runtime.block_on(run_server(
		opts,
		scheme,
		args.transport,
		args.port,
		daemon_keepalive.clone(),
	));
	drop(daemon_keepalive);
	runtime.shutdown_timeout(std::time::Duration::from_millis(100));
	result
}

fn in_process_runtime(
	config: code_moniker_query::DaemonWorkspaceConfig,
) -> anyhow::Result<DaemonRuntime> {
	DaemonRuntime::in_process_preload(config)
}

fn live_refresh_label(args: &McpArgs) -> &'static str {
	match args.live_refresh {
		crate::args::LiveRefresh::OnDemand => "on-demand",
		crate::args::LiveRefresh::Auto => "auto",
	}
}

async fn run_server(
	opts: SessionOptions,
	scheme: String,
	transport: McpTransport,
	port: u16,
	daemon: DaemonRuntime,
) -> anyhow::Result<()> {
	let paths_label = path_list(&opts);
	let context = McpContext::new(opts, scheme, daemon);
	match transport {
		McpTransport::Http => run_http_server(context, port, &paths_label).await,
		McpTransport::Stdio => {
			info!(
				event = "stdio_transport_ready",
				paths = %paths_label,
				runtime = "in_process",
				"mcp stdio transport ready"
			);
			mcp::serve_stdio(context).await
		}
	}
}

async fn run_http_server(context: McpContext, port: u16, paths_label: &str) -> anyhow::Result<()> {
	let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
	let addr = listener.local_addr()?;
	let router = mcp::router(context);
	info!(
		event = "http_transport_ready",
		endpoint = %format!("http://{addr}/mcp"),
		paths = %paths_label,
		runtime = "daemon",
		"mcp http transport ready"
	);
	axum::serve(listener, router).await?;
	Ok(())
}

fn init_logging() {
	let _ = tracing_subscriber::fmt()
		.with_writer(std::io::stderr)
		.with_target(false)
		.with_level(true)
		.compact()
		.try_init();
}

fn path_list(opts: &SessionOptions) -> String {
	opts.paths
		.iter()
		.map(|path| path.display().to_string())
		.collect::<Vec<_>>()
		.join(",")
}
