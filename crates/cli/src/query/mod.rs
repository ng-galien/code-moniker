use std::io::Write;
use std::path::PathBuf;

use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{format_query_response, parse_query};

use crate::Exit;
use crate::args::QueryArgs;

pub(crate) fn run<W1: Write, W2: Write>(
	args: &QueryArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match run_inner(args, stdout) {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn run_inner<W: Write>(args: &QueryArgs, stdout: &mut W) -> anyhow::Result<()> {
	let request = parse_query(&args.query)?;
	let client = daemon_client::DaemonClient::connect_or_start_config(query_daemon_config(args)?)?;
	let response = client.query(request)?;
	if args.json {
		serde_json::to_writer_pretty(&mut *stdout, &response)?;
		writeln!(stdout)?;
	} else {
		write!(stdout, "{}", format_query_response(&response))?;
	}
	Ok(())
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
