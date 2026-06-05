use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TRACE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

pub(crate) fn record(event: &str, duration: Duration, detail: impl AsRef<str>) {
	let Some(file) = trace_file() else {
		return;
	};
	let timestamp_ms = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_millis())
		.unwrap_or_default();
	let duration_ms = duration.as_millis();
	let detail = sanitize(detail.as_ref());
	if let Ok(mut file) = file.lock() {
		let _ = writeln!(file, "{timestamp_ms}\t{event}\t{duration_ms}\t{detail}");
	}
}

fn trace_file() -> Option<&'static Mutex<File>> {
	TRACE
		.get_or_init(|| {
			let target = std::env::var_os("CODE_MONIKER_UI_LOG")?;
			let path = trace_path(target);
			let mut file = OpenOptions::new()
				.create(true)
				.append(true)
				.open(path)
				.ok()?;
			let _ = writeln!(file, "timestamp_ms\tevent\tduration_ms\tdetail");
			Some(Mutex::new(file))
		})
		.as_ref()
}

fn trace_path(target: std::ffi::OsString) -> PathBuf {
	if matches!(target.to_str(), Some("1" | "true" | "TRUE" | "yes" | "YES")) {
		std::env::temp_dir().join("code-moniker-ui.log")
	} else {
		PathBuf::from(target)
	}
}

fn sanitize(value: &str) -> String {
	value.replace(['\t', '\n', '\r'], " ")
}
