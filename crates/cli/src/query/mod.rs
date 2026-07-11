use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{Consistency, QueryRequest, format_query_response, parse_query};

use crate::Exit;
use crate::args::{QueryArgs, QueryConsistency};

const LOADING_RETRY_TIMEOUT: Duration = Duration::from_secs(30);
const LOADING_RETRY_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) fn run<W1: Write, W2: Write>(
	args: &QueryArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match run_inner(args, stdout, stderr) {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W1: Write, W2: Write>(
	args: &QueryArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> anyhow::Result<()> {
	let mut request = parse_query(&args.query)?;
	if !args.query.contains("consistency") {
		request.consistency = flag_consistency(args.consistency);
	}
	let client = daemon_client::DaemonClient::connect_or_start_supporting(
		query_daemon_config(args)?,
		request.query.capability(),
	)?;
	let response = query_waiting_for_load(&client, request, stderr)?;
	if args.json {
		serde_json::to_writer_pretty(&mut *stdout, &response)?;
		writeln!(stdout)?;
	} else {
		write!(stdout, "{}", format_query_response(&response))?;
	}
	Ok(())
}

// A daemon that just started answers `workspace_loading` until its first
// index lands. Waiting here, bounded, is the difference between an agent's
// first call succeeding and an agent learning to distrust the tool.
fn query_waiting_for_load<W: Write>(
	client: &daemon_client::DaemonClient,
	request: QueryRequest,
	stderr: &mut W,
) -> anyhow::Result<code_moniker_query::QueryResponse> {
	let deadline = Instant::now() + LOADING_RETRY_TIMEOUT;
	let mut announced = false;
	loop {
		match client.query(request.clone()) {
			Err(error)
				if format!("{error:#}").contains("workspace_loading")
					&& Instant::now() < deadline =>
			{
				if !announced {
					let _ = writeln!(
						stderr,
						"code-moniker: waiting for the daemon to finish indexing…"
					);
					announced = true;
				}
				std::thread::sleep(LOADING_RETRY_INTERVAL);
			}
			result => return result,
		}
	}
}

fn flag_consistency(flag: QueryConsistency) -> Consistency {
	match flag {
		QueryConsistency::StaleOk => Consistency::StaleOk,
		QueryConsistency::RefreshIfStale => Consistency::RefreshIfStale,
		QueryConsistency::Current => Consistency::Current,
	}
}

fn daemon_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
	if roots.is_empty() {
		vec![PathBuf::from(".")]
	} else {
		roots.to_vec()
	}
}

fn query_daemon_config(
	args: &QueryArgs,
) -> anyhow::Result<code_moniker_query::DaemonWorkspaceConfig> {
	daemon_client::daemon_workspace_config(
		daemon_roots(&args.workspace_roots),
		args.project.clone(),
		args.cache.clone(),
		Some(live_refresh_label(args)),
	)
}

fn live_refresh_label(args: &QueryArgs) -> String {
	match args.live_refresh {
		crate::args::LiveRefresh::OnDemand => "on-demand",
		crate::args::LiveRefresh::Auto => "auto",
	}
	.to_string()
}
