use std::io::Write;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;

use crate::Exit;
use crate::args::GitRuntimeArgs;
use code_moniker_workspace::git_runtime::{GitRuntimeError, run_git_executable_bounded};

const PROTOCOL_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitRuntimeEnvelope {
	protocol_version: u32,
	executable: String,
	outcome: &'static str,
	stdout_base64: Option<String>,
	category: Option<String>,
	message: Option<String>,
}

pub(crate) fn run<W1: Write, W2: Write>(
	args: &GitRuntimeArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	let cwd = match std::env::current_dir() {
		Ok(cwd) => cwd,
		Err(error) => {
			return write_error(
				args,
				GitRuntimeError {
					category: "resolution_failed".to_string(),
					message: format!("cannot resolve supervisor working directory: {error}"),
				},
				stdout,
				stderr,
			);
		}
	};
	let command_args = args
		.arguments
		.iter()
		.map(String::as_str)
		.collect::<Vec<_>>();
	match run_git_executable_bounded(
		&args.executable,
		&cwd,
		&command_args,
		Duration::from_millis(args.timeout_ms),
		args.output_limit,
	) {
		Ok(output) => write_envelope(
			GitRuntimeEnvelope {
				protocol_version: PROTOCOL_VERSION,
				executable: args.executable.display().to_string(),
				outcome: "ok",
				stdout_base64: Some(
					base64::engine::general_purpose::STANDARD.encode(output.stdout),
				),
				category: None,
				message: None,
			},
			stdout,
			stderr,
		),
		Err(error) => write_error(args, error, stdout, stderr),
	}
}

fn write_error<W1: Write, W2: Write>(
	args: &GitRuntimeArgs,
	error: GitRuntimeError,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	write_envelope(
		GitRuntimeEnvelope {
			protocol_version: PROTOCOL_VERSION,
			executable: args.executable.display().to_string(),
			outcome: "error",
			stdout_base64: None,
			category: Some(error.category),
			message: Some(error.message),
		},
		stdout,
		stderr,
	)
}

fn write_envelope<W1: Write, W2: Write>(
	envelope: GitRuntimeEnvelope,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	match serde_json::to_writer(&mut *stdout, &envelope)
		.and_then(|()| stdout.write_all(b"\n").map_err(serde_json::Error::io))
	{
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(
				stderr,
				"cannot write Git runtime supervisor response: {error}"
			);
			Exit::UsageError
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn invalid_executable_is_a_versioned_protocol_error() {
		let args = GitRuntimeArgs {
			executable: "git".into(),
			timeout_ms: 1,
			output_limit: 1,
			arguments: vec!["--version".to_string()],
		};
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&args, &mut stdout, &mut stderr), Exit::Match);
		let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
		assert_eq!(value["protocolVersion"], 1);
		assert_eq!(value["outcome"], "error");
		assert_eq!(value["category"], "invalid_configuration");
		assert!(stderr.is_empty());
	}
}
