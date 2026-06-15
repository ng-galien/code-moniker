use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct ViewDocument {
	pub fragment: String,
	pub anchor: PathBuf,
	pub scope_path: String,
	pub spec: ViewSpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
	pub id: String,
	#[serde(default)]
	pub title: Option<String>,
	#[serde(default = "default_scope")]
	pub scope: String,
	#[serde(default)]
	pub intent: Option<String>,
	#[serde(default)]
	pub summary: Option<String>,
	#[serde(default)]
	pub boundaries: Vec<BoundarySpec>,
	#[serde(default)]
	pub gotchas: Vec<GotchaSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundarySpec {
	pub id: String,
	#[serde(default)]
	pub owns: Vec<String>,
	#[serde(default)]
	pub forbids: Vec<String>,
	#[serde(default)]
	pub forbid_rules: Vec<String>,
	#[serde(default)]
	pub rationale: Option<String>,
	#[serde(default)]
	pub symbols: Vec<String>,
	#[serde(default)]
	pub rules: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GotchaSpec {
	pub id: String,
	pub rationale: String,
	#[serde(default)]
	pub symbols: Vec<String>,
	#[serde(default)]
	pub rules: Vec<String>,
	#[serde(default)]
	pub check: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonikerDisplay {
	None,
	Compact,
	Uri,
}

impl MonikerDisplay {
	pub fn parse(value: Option<&str>) -> anyhow::Result<Self> {
		match value.unwrap_or("none") {
			"none" => Ok(Self::None),
			"compact" => Ok(Self::Compact),
			"uri" => Ok(Self::Uri),
			value => {
				anyhow::bail!("unknown moniker_format `{value}`; expected none, compact, or uri")
			}
		}
	}

	pub fn render(self, uri: &str) -> Option<String> {
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
pub struct RenderOptions {
	pub moniker_display: MonikerDisplay,
	pub context_lines: usize,
	pub include_code: bool,
}

fn default_scope() -> String {
	".".to_string()
}
