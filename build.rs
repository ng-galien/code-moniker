//! Build vendored PL/pgSQL parser sources into our extension's .dylib so we
//! get a portable, deterministic plpgsql parser without depending on
//! plpgsql.so's internal symbols (which are hidden as local on macOS).
//!
//! Only compiles when a pgN feature is selected — pure-Rust builds skip
//! the C compilation entirely.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
	let pg_n = match select_pg_feature() {
		Some(v) => v,
		None => return, // no pgN feature → no plpgsql vendor build
	};

	println!("cargo:rerun-if-changed=vendor/plpgsql");
	println!("cargo:rerun-if-changed=build.rs");

	let pg_config = pg_config_for(&pg_n);
	let includedir = run_pg_config(&pg_config, "--includedir-server");

	let vendor = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor/plpgsql");

	let mut build = cc::Build::new();
	build
		.include(&vendor)
		.include(&includedir)
		.flag_if_supported("-Wno-implicit-fallthrough")
		.flag_if_supported("-Wno-unused-parameter")
		.flag_if_supported("-Wno-unused-but-set-variable")
		.flag_if_supported("-Wno-deprecated-non-prototype")
		.flag_if_supported("-Wno-sign-compare");

	for src in [
		"pl_gram.c",
		"pl_scanner.c",
		"pl_funcs.c",
		"pl_comp.c",
		"pcm_plpgsql_driver.c",
	] {
		build.file(vendor.join(src));
	}

	build.compile("pcm_plpgsql");
}

fn select_pg_feature() -> Option<String> {
	for v in ["14", "15", "16", "17"] {
		if env::var(format!("CARGO_FEATURE_PG{}", v)).is_ok() {
			return Some(v.to_string());
		}
	}
	None
}

fn pg_config_for(pg_n: &str) -> String {
	if let Ok(p) = env::var(format!("PG_CONFIG_PG{}", pg_n)) {
		return p;
	}
	if let Ok(home) = env::var("HOME") {
		// pgrx default install layout
		let candidate = format!("{}/.pgrx/{}.{}/pgrx-install/bin/pg_config", home, pg_n, minor_for(pg_n));
		if std::path::Path::new(&candidate).exists() {
			return candidate;
		}
	}
	"pg_config".to_string()
}

/// pgrx encodes the minor in the directory name (e.g. 17.9). Default to the
/// latest known minor; user can override via PG_CONFIG_PG{N}.
fn minor_for(pg_n: &str) -> &'static str {
	match pg_n {
		"17" => "9",
		"16" => "10",
		"15" => "14",
		"14" => "19",
		_ => "0",
	}
}

fn run_pg_config(pg_config: &str, arg: &str) -> String {
	let out = Command::new(pg_config)
		.arg(arg)
		.output()
		.unwrap_or_else(|e| panic!("failed to invoke {pg_config} {arg}: {e}"));
	if !out.status.success() {
		panic!("{pg_config} {arg} failed: {}", String::from_utf8_lossy(&out.stderr));
	}
	String::from_utf8(out.stdout)
		.expect("non-utf8 from pg_config")
		.trim()
		.to_string()
}
