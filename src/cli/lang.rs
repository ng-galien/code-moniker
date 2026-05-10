use std::path::Path;

use thiserror::Error;

use crate::lang::Lang;

#[derive(Debug, Error)]
pub enum LangError {
	#[error("unsupported file extension `.{0}` (known: ts/tsx/js/jsx, rs, java, py, go, cs, sql)")]
	UnknownExtension(String),
	#[error("file has no extension; cannot infer language")]
	NoExtension,
}

pub fn path_to_lang(path: &Path) -> Result<Lang, LangError> {
	let ext = path
		.extension()
		.and_then(|s| s.to_str())
		.map(|s| s.to_ascii_lowercase());
	let ext = match ext.as_deref() {
		Some("") | None => return Err(LangError::NoExtension),
		Some(e) => e,
	};
	match ext {
		"ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Ok(Lang::Ts),
		"rs" => Ok(Lang::Rs),
		"java" => Ok(Lang::Java),
		"py" | "pyi" => Ok(Lang::Python),
		"go" => Ok(Lang::Go),
		"cs" => Ok(Lang::Cs),
		#[cfg(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17"))]
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
	fn ts_family_resolves_to_ts() {
		for p in &[
			"x.ts",
			"x.tsx",
			"x.js",
			"x.jsx",
			"x.mjs",
			"x.cjs",
			"a/b/c/x.TS",
		] {
			assert_eq!(dispatch(p).unwrap(), Lang::Ts, "{p}");
		}
	}

	#[test]
	fn each_supported_extension_resolves() {
		assert_eq!(dispatch("a.rs").unwrap(), Lang::Rs);
		assert_eq!(dispatch("a.java").unwrap(), Lang::Java);
		assert_eq!(dispatch("a.py").unwrap(), Lang::Python);
		assert_eq!(dispatch("a.pyi").unwrap(), Lang::Python);
		assert_eq!(dispatch("a.go").unwrap(), Lang::Go);
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
	}

	#[cfg(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17"))]
	#[test]
	fn sql_resolves_when_pg_feature_on() {
		assert_eq!(dispatch("a.sql").unwrap(), Lang::Sql);
	}

	#[cfg(not(any(feature = "pg14", feature = "pg15", feature = "pg16", feature = "pg17")))]
	#[test]
	fn sql_unknown_without_pg_feature() {
		match dispatch("a.sql") {
			Err(LangError::UnknownExtension(s)) => assert_eq!(s, "sql"),
			other => panic!("unexpected: {other:?}"),
		}
	}
}
