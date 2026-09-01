use std::path::Path;

use thiserror::Error;

use code_moniker_core::lang::Lang;

#[derive(Debug, Error)]
pub enum LangError {
	#[error(
		"unsupported file extension `.{0}` (known: ts/tsx/mts/cts/js/jsx/mjs/cjs, rs, java, py/pyi, go, c/h, cs, sql/sql.in/plpgsql)"
	)]
	UnknownExtension(String),
	#[error("file has no extension; cannot infer language")]
	NoExtension,
}

pub fn path_to_lang(path: &Path) -> Result<Lang, LangError> {
	if path
		.file_name()
		.and_then(|name| name.to_str())
		.is_some_and(|name| name.to_ascii_lowercase().ends_with(".sql.in"))
	{
		return Ok(Lang::Sql);
	}
	let ext = path
		.extension()
		.and_then(|s| s.to_str())
		.map(|s| s.to_ascii_lowercase());
	let ext = match ext.as_deref() {
		Some("") | None => return Err(LangError::NoExtension),
		Some(e) => e,
	};
	match ext {
		"tsx" => Ok(Lang::Tsx),
		"jsx" => Ok(Lang::Jsx),
		"js" | "mjs" | "cjs" => Ok(Lang::Js),
		"ts" | "mts" | "cts" => Ok(Lang::Ts),
		"rs" => Ok(Lang::Rs),
		"java" => Ok(Lang::Java),
		"py" | "pyi" => Ok(Lang::Python),
		"go" => Ok(Lang::Go),
		"c" | "h" => Ok(Lang::C),
		"cs" => Ok(Lang::Cs),
		"sql" | "plpgsql" => Ok(Lang::Sql),
		other => Err(LangError::UnknownExtension(other.to_string())),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;

	fn dispatch(s: &str) -> Result<Lang, LangError> {
		path_to_lang(&PathBuf::from(s))
	}

	#[test]
	fn typescript_resolves_to_its_own_language() {
		for p in &["x.ts", "x.mts", "x.cts", "a/b/c/x.TS"] {
			assert_eq!(dispatch(p).unwrap(), Lang::Ts, "{p}");
		}
	}

	#[test]
	fn tsx_resolves_to_its_own_language() {
		assert_eq!(dispatch("x.tsx").unwrap(), Lang::Tsx);
		assert_eq!(dispatch("x.TSX").unwrap(), Lang::Tsx);
	}

	#[test]
	fn javascript_module_formats_share_the_javascript_language() {
		for p in &["x.js", "x.mjs", "x.cjs", "a/b/c/x.JS"] {
			assert_eq!(dispatch(p).unwrap(), Lang::Js, "{p}");
		}
	}

	#[test]
	fn jsx_resolves_to_its_own_language() {
		assert_eq!(dispatch("x.jsx").unwrap(), Lang::Jsx);
		assert_eq!(dispatch("x.JSX").unwrap(), Lang::Jsx);
	}

	#[test]
	fn each_supported_extension_resolves() {
		assert_eq!(dispatch("a.rs").unwrap(), Lang::Rs);
		assert_eq!(dispatch("a.java").unwrap(), Lang::Java);
		assert_eq!(dispatch("a.py").unwrap(), Lang::Python);
		assert_eq!(dispatch("a.pyi").unwrap(), Lang::Python);
		assert_eq!(dispatch("a.go").unwrap(), Lang::Go);
		assert_eq!(dispatch("a.c").unwrap(), Lang::C);
		assert_eq!(dispatch("a.h").unwrap(), Lang::C);
		assert_eq!(dispatch("a.cs").unwrap(), Lang::Cs);
	}

	#[test]
	fn unknown_extension_errors() {
		match dispatch("a.txt") {
			Err(LangError::UnknownExtension(s)) => assert_eq!(s, "txt"),
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn missing_extension_errors() {
		match dispatch("Makefile") {
			Err(LangError::NoExtension) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn case_is_insensitive() {
		assert_eq!(dispatch("X.JAVA").unwrap(), Lang::Java);
		assert_eq!(dispatch("X.RS").unwrap(), Lang::Rs);
		assert_eq!(dispatch("X.H").unwrap(), Lang::C);
	}

	#[test]
	fn sql_extension_resolves() {
		assert_eq!(dispatch("a.sql").unwrap(), Lang::Sql);
		assert_eq!(dispatch("a.plpgsql").unwrap(), Lang::Sql);
		assert_eq!(dispatch("extension.SQL.IN").unwrap(), Lang::Sql);
	}
}
