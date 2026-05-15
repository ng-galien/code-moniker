use regex::Regex;

#[derive(Clone, Debug)]
pub(super) struct NavFilter {
	raw: String,
	name_pattern: Option<String>,
	name: Option<Regex>,
	kind: Option<String>,
}

impl NavFilter {
	pub(super) fn matches(&self, kind: &str, name: &str) -> bool {
		if let Some(expected) = &self.kind
			&& kind != expected
		{
			return false;
		}
		if let Some(re) = &self.name
			&& !re.is_match(name)
		{
			return false;
		}
		true
	}

	pub(super) fn describe(&self) -> String {
		match (&self.kind, &self.name_pattern) {
			(Some(kind), Some(name)) => format!("kind:{kind} /{name}"),
			(Some(kind), None) => format!("kind:{kind}"),
			(None, Some(name)) => format!("/{name}"),
			(None, None) => self.raw.clone(),
		}
	}
}

pub(super) fn parse_filter(raw: &str) -> Result<Option<NavFilter>, regex::Error> {
	let raw = raw.trim();
	if raw.is_empty() {
		return Ok(None);
	}
	let mut kind = None;
	let mut name_parts = Vec::new();
	for token in raw.split_whitespace() {
		if let Some(value) = token
			.strip_prefix("kind:")
			.or_else(|| token.strip_prefix("kind="))
			.or_else(|| token.strip_prefix("k:"))
			.or_else(|| token.strip_prefix("k="))
		{
			if !value.is_empty() {
				kind = Some(value.to_ascii_lowercase());
			}
			continue;
		}
		if let Some(value) = token
			.strip_prefix("name:")
			.or_else(|| token.strip_prefix("name="))
			.or_else(|| token.strip_prefix("n:"))
			.or_else(|| token.strip_prefix("n="))
		{
			if !value.is_empty() {
				name_parts.push(value.to_string());
			}
			continue;
		}
		name_parts.push(token.to_string());
	}
	let name_pattern = (!name_parts.is_empty()).then(|| name_parts.join(" "));
	let name = name_pattern.as_deref().map(Regex::new).transpose()?;
	Ok(Some(NavFilter {
		raw: raw.to_string(),
		name_pattern,
		name,
		kind,
	}))
}
