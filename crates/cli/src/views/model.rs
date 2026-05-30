use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug)]
pub(crate) struct ViewDocument {
	pub(crate) fragment: String,
	pub(crate) anchor: PathBuf,
	pub(crate) scope_path: String,
	pub(crate) spec: ViewSpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewSpec {
	pub(crate) id: String,
	#[serde(default)]
	pub(crate) title: Option<String>,
	#[serde(default = "default_scope")]
	pub(crate) scope: String,
	#[serde(default)]
	pub(crate) intent: Option<String>,
	#[serde(default)]
	pub(crate) summary: Option<String>,
	#[serde(default)]
	pub(crate) boundaries: Vec<BoundarySpec>,
	#[serde(default)]
	pub(crate) gotchas: Vec<GotchaSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BoundarySpec {
	pub(crate) id: String,
	#[serde(default)]
	pub(crate) owns: Vec<String>,
	#[serde(default)]
	pub(crate) forbids: Vec<String>,
	#[serde(default)]
	pub(crate) rationale: Option<String>,
	#[serde(default)]
	pub(crate) symbols: Vec<String>,
	#[serde(default)]
	pub(crate) rules: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GotchaSpec {
	pub(crate) id: String,
	pub(crate) rationale: String,
	#[serde(default)]
	pub(crate) symbols: Vec<String>,
	#[serde(default)]
	pub(crate) rules: Vec<String>,
	#[serde(default)]
	pub(crate) check: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MonikerDisplay {
	None,
	Compact,
	Uri,
}

impl MonikerDisplay {
	pub(crate) fn parse(value: Option<&str>) -> anyhow::Result<Self> {
		match value.unwrap_or("none") {
			"none" => Ok(Self::None),
			"compact" => Ok(Self::Compact),
			"uri" => Ok(Self::Uri),
			value => {
				anyhow::bail!("unknown moniker_format `{value}`; expected none, compact, or uri")
			}
		}
	}

	pub(crate) fn render(self, uri: &str) -> Option<String> {
		match self {
			Self::None => None,
			Self::Uri => Some(uri.to_string()),
			Self::Compact => Some(
				uri.strip_prefix("code+moniker://./")
					.or_else(|| uri.strip_prefix("code+moniker://"))
					.unwrap_or(uri)
					.to_string(),
			),
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderOptions {
	pub(crate) moniker_display: MonikerDisplay,
	pub(crate) context_lines: usize,
}

fn default_scope() -> String {
	".".to_string()
}
