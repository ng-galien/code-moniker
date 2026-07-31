use std::io::Write;
use std::path::PathBuf;

use code_moniker_daemon_client as daemon_client;
use code_moniker_query::{
	Consistency, format_query_response_projected, parse_query, query_projection,
};

use crate::Exit;
use crate::args::{QueryArgs, QueryConsistency};

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
	_stderr: &mut W2,
) -> anyhow::Result<()> {
	let mut request = parse_query(&args.query)?;
	if !args.query.contains("consistency") {
		request.consistency = flag_consistency(args.consistency);
	}
	let projection = query_projection(&request.query).to_vec();
	let client = query_daemon_client(args, request.query.capability())?;
	let response = client.query(request)?;
	if args.json {
		serde_json::to_writer_pretty(&mut *stdout, &response)?;
		writeln!(stdout)?;
	} else {
		write!(
			stdout,
			"{}",
			format_query_response_projected(&response, &projection)
		)?;
	}
	Ok(())
}

fn query_daemon_client(
	args: &QueryArgs,
	capability: &str,
) -> anyhow::Result<daemon_client::DaemonClient> {
	let Some(endpoint) = args.daemon.as_deref() else {
		return daemon_client::DaemonClient::connect_or_start_supporting(
			query_daemon_config(args)?,
			capability,
		);
	};
	let client = daemon_client::DaemonClient::connect_endpoint(endpoint)?;
	if client.supports_query(capability)? {
		return Ok(client);
	}
	anyhow::bail!("daemon {endpoint} does not support query capability {capability}")
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
		crate::args::cache_dir_with_env(&args.cache),
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
