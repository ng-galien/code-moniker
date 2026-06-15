use std::io::Write;

use code_moniker_daemon_client::{DaemonClient, daemon_workspace_config};
use tracing::info;

use crate::args::McpArgs;
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
	let paths = args.paths.clone();
	let daemon_config = daemon_workspace_config(
		paths.clone(),
		args.project.to_owned(),
		args.cache.to_owned(),
		Some(live_refresh_label(args).to_string()),
	)?;
	let client = DaemonClient::connect_or_start_config(daemon_config)?;
	let opts = SessionOptions {
		paths,
		project: args.project.to_owned(),
		cache_dir: args.cache.to_owned(),
	};
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("code-moniker-mcp")
		.build()?;
	runtime.block_on(run_server(
		opts,
		scheme,
		args.port,
		DaemonRuntime::client(client),
	))
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
	port: u16,
	daemon: DaemonRuntime,
) -> anyhow::Result<()> {
	let paths_label = path_list(&opts);
	let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
	let addr = listener.local_addr()?;
	let context = McpContext::new(opts, scheme, daemon);
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
