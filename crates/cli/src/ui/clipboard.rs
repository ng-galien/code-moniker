use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::thread;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::ui) struct ClipboardResult {
	pub(in crate::ui) component: String,
	pub(in crate::ui) result: Result<(), String>,
}

pub(super) fn copy_text_async<F>(component: String, text: String, notify: F) -> anyhow::Result<()>
where
	F: FnOnce(ClipboardResult) + Send + 'static,
{
	if text.is_empty() {
		return Err(anyhow::anyhow!("clipboard snapshot is empty"));
	}
	thread::Builder::new()
		.name("code-moniker-clipboard".to_string())
		.spawn(move || {
			let result = copy_with_system_clipboard(&text).map_err(|error| format!("{error:#}"));
			notify(ClipboardResult { component, result });
		})?;
	Ok(())
}

#[cfg(target_os = "macos")]
fn copy_with_system_clipboard(text: &str) -> anyhow::Result<()> {
	copy_with_command("pbcopy", &[], text)
}

#[cfg(target_os = "windows")]
fn copy_with_system_clipboard(text: &str) -> anyhow::Result<()> {
	copy_with_command("clip", &[], text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn copy_with_system_clipboard(text: &str) -> anyhow::Result<()> {
	let candidates: &[(&str, &[&str])] = &[
		("wl-copy", &[]),
		("xclip", &["-selection", "clipboard"]),
		("xsel", &["--clipboard", "--input"]),
	];
	let mut last_error = None;
	for (program, args) in candidates {
		match copy_with_command(program, args, text) {
			Ok(()) => return Ok(()),
			Err(error) => last_error = Some(error),
		}
	}
	Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no clipboard command available")))
}

fn copy_with_command(program: &str, args: &[&str], text: &str) -> anyhow::Result<()> {
	let mut child = match Command::new(program)
		.args(args)
		.stdin(Stdio::piped())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.spawn()
	{
		Ok(child) => child,
		Err(error) if error.kind() == ErrorKind::NotFound => {
			return Err(anyhow::anyhow!("{program} not found"));
		}
		Err(error) => return Err(error.into()),
	};
	let Some(mut stdin) = child.stdin.take() else {
		return Err(anyhow::anyhow!("cannot open {program} stdin"));
	};
	if let Err(error) = stdin.write_all(text.as_bytes()) {
		let _ = child.kill();
		let _ = child.wait();
		return Err(error.into());
	}
	drop(stdin);
	let status = child.wait()?;
	if !status.success() {
		return Err(anyhow::anyhow!("{program} exited with {status}"));
	}
	Ok(())
}
