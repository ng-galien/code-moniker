use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, UriError, from_uri};

mod build;
mod parse;
mod serialize;

pub use build::build_graph;
pub use parse::parse_spec;
pub use serialize::{SerializeError, graph_to_spec};

pub(crate) fn parse_moniker_uri(uri: &str) -> Result<Moniker, UriError> {
	let scheme_end = uri.find("://").ok_or(UriError::MissingProject)?;
	from_uri(
		uri,
		&UriConfig {
			scheme: &uri[..scheme_end + 3],
		},
	)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclareSpec {
	pub root: Moniker,
	pub lang: Lang,
	pub symbols: Vec<DeclSymbol>,
	pub edges: Vec<DeclEdge>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Lang {
	Ts,
	Rs,
	Java,
	Python,
	Go,
	Cs,
	Sql,
}

impl Lang {
	pub fn from_tag(s: &str) -> Option<Self> {
		match s {
			"ts" => Some(Self::Ts),
			"rs" => Some(Self::Rs),
			"java" => Some(Self::Java),
			"python" => Some(Self::Python),
			"go" => Some(Self::Go),
			"cs" => Some(Self::Cs),
			"sql" => Some(Self::Sql),
			_ => None,
		}
	}

	pub fn tag(self) -> &'static str {
		match self {
			Self::Ts => "ts",
			Self::Rs => "rs",
			Self::Java => "java",
			Self::Python => "python",
			Self::Go => "go",
			Self::Cs => "cs",
			Self::Sql => "sql",
		}
	}

	pub fn allowed_kinds(self) -> &'static [&'static str] {
		match self {
			Self::Ts => &[
				"class",
				"interface",
				"type",
				"function",
				"method",
				"const",
				"namespace",
				"module",
				"enum",
			],
			Self::Rs => &[
				"struct", "enum", "trait", "impl", "fn", "method", "const", "static", "mod", "type",
			],
			Self::Java => &[
				"class",
				"interface",
				"enum",
				"record",
				"annotation_type",
				"method",
				"constructor",
				"field",
			],
			Self::Python => &["class", "function", "method", "async_function"],
			Self::Go => &[
				"type",
				"struct",
				"interface",
				"func",
				"method",
				"var",
				"const",
			],
			Self::Cs => &[
				"class",
				"interface",
				"struct",
				"record",
				"enum",
				"delegate",
				"method",
				"constructor",
				"field",
				"property",
				"event",
			],
			Self::Sql => &["function", "procedure", "view", "table", "schema"],
		}
	}

	pub fn allowed_visibilities(self) -> &'static [&'static str] {
		match self {
			Self::Ts => &["public", "private", "module"],
			Self::Rs => &["public", "private", "module"],
			Self::Java => &["public", "protected", "package", "private"],
			Self::Python => &["public", "private", "module"],
			Self::Go => &["public", "module"],
			Self::Cs => &["public", "protected", "internal", "private"],
			Self::Sql => &[],
		}
	}

	pub fn ignores_visibility(self) -> bool {
		matches!(self, Self::Sql)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclSymbol {
	pub moniker: Moniker,
	pub kind: String,
	pub parent: Moniker,
	pub visibility: Option<String>,
	pub signature: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclEdge {
	pub from: Moniker,
	pub kind: EdgeKind,
	pub to: Moniker,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum EdgeKind {
	DependsOn,
	Calls,
	InjectsProvide,
	InjectsRequire,
}

impl EdgeKind {
	pub fn from_tag(s: &str) -> Option<Self> {
		match s {
			"depends_on" => Some(Self::DependsOn),
			"calls" => Some(Self::Calls),
			"injects:provide" => Some(Self::InjectsProvide),
			"injects:require" => Some(Self::InjectsRequire),
			_ => None,
		}
	}

	pub fn tag(self) -> &'static str {
		match self {
			Self::DependsOn => "depends_on",
			Self::Calls => "calls",
			Self::InjectsProvide => "injects:provide",
			Self::InjectsRequire => "injects:require",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclareError {
	JsonParse(String),
	NotAnObject(&'static str),
	MissingField {
		path: String,
		field: &'static str,
	},
	InvalidType {
		path: String,
		expected: &'static str,
	},
	UnknownLang(String),
	UnknownEdgeKind(String),
	InvalidMoniker {
		path: String,
		value: String,
		reason: String,
	},
	KindNotInProfile {
		lang: Lang,
		kind: String,
	},
	VisibilityNotInProfile {
		lang: Lang,
		visibility: String,
	},
	KindMismatchMoniker {
		path: String,
		declared_kind: String,
		moniker_last_kind: String,
	},
	DuplicateMoniker {
		moniker: String,
	},
	UnknownParent {
		path: String,
		parent: String,
	},
	UnknownEdgeSource {
		path: String,
		from: String,
	},
	GraphError(String),
}

impl std::fmt::Display for DeclareError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		use DeclareError::*;
		match self {
			JsonParse(msg) => write!(f, "spec is not valid JSON: {msg}"),
			NotAnObject(what) => write!(f, "{what} must be a JSON object"),
			MissingField { path, field } => {
				write!(f, "{path}: required field `{field}` is missing")
			}
			InvalidType { path, expected } => write!(f, "{path}: expected {expected}"),
			UnknownLang(s) => write!(
				f,
				"unknown lang `{s}` (expected ts | rs | java | python | go | cs | sql)"
			),
			UnknownEdgeKind(s) => write!(
				f,
				"unknown edge kind `{s}` (expected depends_on | calls | injects:provide | injects:require)"
			),
			InvalidMoniker {
				path,
				value,
				reason,
			} => write!(f, "{path}: invalid moniker URI `{value}`: {reason}"),
			KindNotInProfile { lang, kind } => write!(
				f,
				"kind `{kind}` is not allowed for lang={} (see profile)",
				lang.tag()
			),
			VisibilityNotInProfile { lang, visibility } => write!(
				f,
				"visibility `{visibility}` is not allowed for lang={} (see profile)",
				lang.tag()
			),
			KindMismatchMoniker {
				path,
				declared_kind,
				moniker_last_kind,
			} => write!(
				f,
				"{path}: declared kind `{declared_kind}` does not match the moniker's last segment kind `{moniker_last_kind}`"
			),
			DuplicateMoniker { moniker } => {
				write!(f, "duplicate moniker in symbols: {moniker}")
			}
			UnknownParent { path, parent } => write!(
				f,
				"{path}: parent `{parent}` is neither the root nor a previously declared symbol"
			),
			UnknownEdgeSource { path, from } => {
				write!(f, "{path}: edge `from` `{from}` is not a declared symbol")
			}
			GraphError(msg) => write!(f, "graph build error: {msg}"),
		}
	}
}

impl std::error::Error for DeclareError {}

pub fn declare_from_json_str(json: &str) -> Result<CodeGraph, DeclareError> {
	let value: serde_json::Value =
		serde_json::from_str(json).map_err(|e| DeclareError::JsonParse(e.to_string()))?;
	declare_from_json_value(&value)
}

pub fn declare_from_json_value(json: &serde_json::Value) -> Result<CodeGraph, DeclareError> {
	let spec = parse_spec(json)?;
	build_graph(&spec)
}
