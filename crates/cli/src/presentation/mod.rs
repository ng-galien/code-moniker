use minijinja::{AutoEscape, Environment};
use serde::Serialize;
use serde_json::Value;

use code_moniker_workspace::code::compact_identity;

pub(crate) mod navigation;
pub(crate) mod notes;
pub(crate) mod problem;
pub(crate) mod query;
pub(crate) mod relationships;
pub(crate) mod rules;
pub(crate) mod symbols;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderOptions<'a> {
	pub(crate) compact: bool,
	pub(crate) scheme: &'a str,
	pub(crate) runtime: Option<&'a str>,
}

#[derive(Debug)]
pub(crate) struct TemplateOutput {
	name: &'static str,
	source: &'static str,
	context: Value,
	monikers: Vec<String>,
}

impl TemplateOutput {
	pub(crate) fn new<T>(
		name: &'static str,
		source: &'static str,
		context: &T,
	) -> anyhow::Result<Self>
	where
		T: Serialize,
	{
		Ok(Self {
			name,
			source,
			context: serde_json::to_value(context)?,
			monikers: Vec::new(),
		})
	}

	pub(crate) fn with_monikers<'a>(mut self, monikers: impl IntoIterator<Item = &'a str>) -> Self {
		self.monikers
			.extend(monikers.into_iter().map(str::to_owned));
		self
	}

	pub(crate) fn render(&self, options: RenderOptions<'_>) -> anyhow::Result<String> {
		render_value(
			self.name,
			self.source,
			&self.context,
			&self.monikers,
			options,
		)
	}

	pub(crate) fn context(&self) -> &Value {
		&self.context
	}
}

pub(crate) fn render<T>(
	name: &'static str,
	source: &'static str,
	context: &T,
) -> anyhow::Result<String>
where
	T: Serialize,
{
	TemplateOutput::new(name, source, context)?.render(RenderOptions {
		compact: false,
		scheme: crate::DEFAULT_SCHEME,
		runtime: None,
	})
}

fn render_value(
	name: &'static str,
	source: &'static str,
	context: &Value,
	monikers: &[String],
	options: RenderOptions<'_>,
) -> anyhow::Result<String> {
	let mut environment = Environment::new();
	environment.set_auto_escape_callback(|_| AutoEscape::None);
	environment.set_trim_blocks(true);
	environment.set_lstrip_blocks(true);
	environment.set_keep_trailing_newline(true);
	environment.add_global("runtime", options.runtime.unwrap_or_default().to_owned());
	let moniker_scheme = options.scheme.to_owned();
	let compact_monikers = options.compact;
	environment.add_filter("moniker", move |value: String| {
		compact_presentation_uri(value, compact_monikers, &moniker_scheme, false)
	});
	let call_scheme = options.scheme.to_owned();
	let compact_calls = options.compact;
	environment.add_filter("call_uri", move |value: String| {
		compact_presentation_uri(value, compact_calls, &call_scheme, true)
	});
	let text_scheme = options.scheme.to_owned();
	let compact_text = options.compact;
	let text_monikers = monikers.to_vec();
	environment.add_filter("moniker_text", move |value: String| {
		compact_presentation_text(value, compact_text, &text_scheme, &text_monikers)
	});
	environment.add_filter("code_block", markdown_code_block);
	environment.add_filter("shell_code_block", markdown_shell_code_block);
	environment.add_filter("json_code_block", markdown_json_code_block);
	environment.add_filter("md_text", markdown_text);
	environment.add_filter("md_heading", markdown_heading);
	environment.add_filter("table_cell", markdown_table_cell);
	environment.add_filter("inline_code", markdown_inline_code);
	environment.add_filter("md_block", markdown_block);
	environment.add_template(name, source)?;
	Ok(environment.get_template(name)?.render(context)?)
}

fn markdown_code_block(value: String) -> String {
	markdown_fenced_code_block(value, "text")
}

fn markdown_shell_code_block(value: String) -> String {
	markdown_fenced_code_block(value, "sh")
}

fn markdown_fenced_code_block(value: String, language: &str) -> String {
	let longest_run = value
		.split(|character| character != '`')
		.map(str::len)
		.max()
		.unwrap_or(0);
	let fence = "`".repeat(longest_run.saturating_add(1).max(3));
	format!("{fence}{language}\n{}\n{fence}", value.trim_end())
}

fn markdown_json_code_block(value: minijinja::Value) -> Result<String, minijinja::Error> {
	serde_json::to_string_pretty(&value)
		.map(markdown_code_block)
		.map_err(|error| {
			minijinja::Error::new(
				minijinja::ErrorKind::InvalidOperation,
				format!("cannot serialize template value as JSON: {error}"),
			)
		})
}

fn markdown_text(value: String) -> String {
	let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
	let mut escaped = String::with_capacity(value.len());
	for character in value.chars() {
		if matches!(character, '\\' | '`' | '*' | '[' | ']' | '<' | '>') {
			escaped.push('\\');
		}
		escaped.push(character);
	}
	escaped
}

fn markdown_heading(value: String) -> String {
	markdown_text(value)
}

fn markdown_table_cell(value: String) -> String {
	markdown_text(value).replace('|', "\\|")
}

fn markdown_inline_code(value: String) -> String {
	let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
	let longest_run = value
		.split(|character| character != '`')
		.map(str::len)
		.max()
		.unwrap_or(0);
	let fence = "`".repeat(longest_run.saturating_add(1).max(1));
	if value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']) {
		format!("{fence} {value} {fence}")
	} else {
		format!("{fence}{value}{fence}")
	}
}

fn markdown_block(value: String) -> String {
	value
		.trim()
		.lines()
		.map(str::trim)
		.map(markdown_block_line)
		.collect::<Vec<_>>()
		.join("\n")
}

fn markdown_block_line(value: &str) -> String {
	let mut escaped = markdown_text(value.to_owned());
	let first = escaped.chars().next();
	if matches!(first, Some('#' | '>' | '-' | '+' | '_' | '=' | '~' | '|')) {
		escaped.insert(0, '\\');
		return escaped;
	}

	let ordered_marker = escaped
		.char_indices()
		.take_while(|(_, character)| character.is_ascii_digit())
		.last()
		.map(|(index, character)| index + character.len_utf8())
		.filter(|index| *index > 0)
		.filter(|index| matches!(escaped[*index..].chars().next(), Some('.' | ')')));
	if let Some(index) = ordered_marker {
		escaped.insert(index, '\\');
	}
	escaped
}

fn compact_presentation_text(
	mut value: String,
	compact: bool,
	scheme: &str,
	monikers: &[String],
) -> String {
	if !compact {
		return value;
	}
	let mut replacements = monikers
		.iter()
		.filter_map(|moniker| {
			let compact = compact_presentation_uri(moniker.to_owned(), true, scheme, false);
			if compact == *moniker {
				None
			} else {
				Some((moniker, compact))
			}
		})
		.collect::<Vec<_>>();
	replacements.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
	for (moniker, compact) in replacements {
		value = value.replace(moniker, &compact);
	}
	value
}

fn compact_presentation_uri(value: String, compact: bool, scheme: &str, call: bool) -> String {
	if !compact {
		return value;
	}
	if call {
		let workspace = format!("{scheme}workspace");
		if let Some(suffix) = value.strip_prefix(&workspace)
			&& (suffix.is_empty() || suffix.starts_with('/'))
		{
			return format!("workspace{suffix}");
		}
	}
	compact_identity(&value, scheme).unwrap_or(value)
}

#[cfg(test)]
pub(crate) mod tests {
	use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

	pub(crate) fn validate_agent_markdown(
		markdown: &str,
		expected_title: &str,
		expect_table: bool,
	) -> Result<(), String> {
		fn heading_level(level: HeadingLevel) -> u8 {
			match level {
				HeadingLevel::H1 => 1,
				HeadingLevel::H2 => 2,
				HeadingLevel::H3 => 3,
				HeadingLevel::H4 => 4,
				HeadingLevel::H5 => 5,
				HeadingLevel::H6 => 6,
			}
		}

		if !markdown.ends_with('\n') {
			return Err("Markdown output must end with a newline".to_string());
		}
		if markdown.contains("{%") || markdown.contains("{{") {
			return Err(format!("unrendered template syntax in output:\n{markdown}"));
		}
		let parser = Parser::new_ext(markdown, Options::ENABLE_TABLES);
		let mut headings = Vec::<(u8, String)>::new();
		let mut current_heading = None::<(u8, String)>;
		let mut saw_table = false;
		for event in parser {
			match event {
				Event::Start(Tag::Heading { level, .. }) => {
					current_heading = Some((heading_level(level), String::new()));
				}
				Event::Start(Tag::Table(_)) => saw_table = true,
				Event::Text(text) | Event::Code(text) => {
					if let Some((_, title)) = &mut current_heading {
						title.push_str(&text);
					}
				}
				Event::End(TagEnd::Heading(_)) => {
					headings.push(current_heading.take().expect("heading start"));
				}
				Event::Html(html) | Event::InlineHtml(html) => {
					return Err(format!(
						"raw HTML is not part of the agent Markdown contract: {html}"
					));
				}
				_ => {}
			}
		}
		let first_heading = headings
			.first()
			.map(|(level, title)| (*level, title.as_str()));
		if first_heading != Some((1, expected_title)) {
			return Err(format!(
				"the document must begin with H1 `{expected_title}`: {markdown}"
			));
		}
		if headings.iter().filter(|(level, _)| *level == 1).count() != 1 {
			return Err(format!(
				"agent Markdown must contain exactly one H1: {markdown}"
			));
		}
		for pair in headings.windows(2) {
			if pair[1].0 > pair[0].0 + 1 {
				return Err(format!(
					"Markdown heading hierarchy skips from H{} to H{}: {markdown}",
					pair[0].0, pair[1].0
				));
			}
		}
		if saw_table != expect_table {
			return Err("unexpected Markdown table contract".to_string());
		}
		Ok(())
	}

	#[test]
	fn markdown_filters_prevent_project_values_from_injecting_structure() {
		let rendered = super::render(
			"adversarial.md.j2",
			concat!(
				"# {{ title | md_heading }}\n\n",
				"- value: {{ text | md_text }}\n\n",
				"| Name | Count |\n| --- | ---: |\n",
				"| {{ cell | table_cell }} | 1 |\n\n",
				"- selector: {{ code | inline_code }}\n",
			),
			&serde_json::json!({
				"title": "Safe\n## Injected",
				"text": "*unsafe* <tag>",
				"cell": "left | right",
				"code": "name`with markup",
			}),
		)
		.expect("render adversarial Markdown");

		assert!(!rendered.contains("\n## Injected"));
		assert!(rendered.contains("left \\| right"));
		assert!(rendered.contains("``name`with markup``"));
		validate_agent_markdown(&rendered, "Safe ## Injected", true).expect("valid agent Markdown");
	}

	#[test]
	fn md_block_is_literal_multiline_project_prose() {
		assert_eq!(
			super::markdown_block(
				"# heading\n<script>unsafe</script>\n1. list\n> quote\n---".to_string()
			),
			"\\# heading\n\\<script\\>unsafe\\</script\\>\n1\\. list\n\\> quote\n\\---"
		);
	}

	#[test]
	fn agent_templates_delegate_inline_code_fencing_to_the_filter() {
		let templates = [
			(
				"navigation/context.md.j2",
				include_str!("../../templates/navigation/context.md.j2"),
			),
			(
				"navigation/read-ast.md.j2",
				include_str!("../../templates/navigation/read-ast.md.j2"),
			),
			(
				"navigation/read-explorer.md.j2",
				include_str!("../../templates/navigation/read-explorer.md.j2"),
			),
			(
				"navigation/read-symbol.md.j2",
				include_str!("../../templates/navigation/read-symbol.md.j2"),
			),
			(
				"navigation/read-view-detail.md.j2",
				include_str!("../../templates/navigation/read-view-detail.md.j2"),
			),
			(
				"navigation/read-view-list.md.j2",
				include_str!("../../templates/navigation/read-view-list.md.j2"),
			),
			(
				"notes/mcp.md.j2",
				include_str!("../../templates/notes/mcp.md.j2"),
			),
			(
				"problem/mcp.md.j2",
				include_str!("../../templates/problem/mcp.md.j2"),
			),
			(
				"query/mcp.md.j2",
				include_str!("../../templates/query/mcp.md.j2"),
			),
			(
				"relationships/diff.md.j2",
				include_str!("../../templates/relationships/diff.md.j2"),
			),
			(
				"relationships/graph.md.j2",
				include_str!("../../templates/relationships/graph.md.j2"),
			),
			(
				"relationships/usages.md.j2",
				include_str!("../../templates/relationships/usages.md.j2"),
			),
			(
				"rules/learn-index.md.j2",
				include_str!("../../templates/rules/learn-index.md.j2"),
			),
			(
				"rules/learn-topic.md.j2",
				include_str!("../../templates/rules/learn-topic.md.j2"),
			),
			(
				"rules/mcp-list.md.j2",
				include_str!("../../templates/rules/mcp-list.md.j2"),
			),
			(
				"rules/mcp-run.md.j2",
				include_str!("../../templates/rules/mcp-run.md.j2"),
			),
			(
				"rules/show.md.j2",
				include_str!("../../templates/rules/show.md.j2"),
			),
			(
				"symbols/insights.md.j2",
				include_str!("../../templates/symbols/insights.md.j2"),
			),
			(
				"symbols/list.md.j2",
				include_str!("../../templates/symbols/list.md.j2"),
			),
			(
				"symbols/search.md.j2",
				include_str!("../../templates/symbols/search.md.j2"),
			),
		];

		for (name, source) in templates {
			assert!(
				!source.contains('`'),
				"{name} must use the inline_code filter instead of manual fences"
			);
			for line in source.lines() {
				let inline_control = !line.trim_start().starts_with("{%")
					&& (line.ends_with("{% endif %}") || line.ends_with("{% endfor %}"));
				assert!(
					!inline_control,
					"{name} must preserve the newline after an inline control block: {line}"
				);
			}
		}
	}

	#[test]
	fn next_call_with_html_and_backticks_remains_inline_code() {
		let output = crate::presentation::query::mcp(&serde_json::json!({
			"uri": "code+moniker://workspace/query",
			"completeness": "partial",
			"mode": "execute",
			"volume": "small",
			"results": [{
				"number": 1,
				"operation": "symbols",
				"body": {"rows": []},
				"next_call": " expression=\"<tag>`unsafe\" cursor=\"next\""
			}]
		}))
		.expect("adversarial query template")
		.render(super::RenderOptions {
			compact: false,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("render adversarial query result");

		assert!(output.contains("``code_moniker_query expression=\"<tag>`unsafe\""));
		validate_agent_markdown(&output, "Query results", false)
			.expect("the complete continuation call remains valid inline code");
	}

	#[test]
	fn usages_conditions_preserve_separate_markdown_items() {
		let output = crate::presentation::relationships::usages(&serde_json::json!({
			"uri": "code+moniker://./lang:rs/module:lib/fn:target()",
			"partial": false,
			"direction": "incoming",
			"target_scope": "exact",
			"targets": 1,
			"limit": 20,
			"volume": "small",
			"target": {
				"kind": "fn",
				"name": "target()",
				"file": "src/lib.rs",
				"line_range": null,
				"language": "rs"
			},
			"scope": {"paths": [], "langs": []},
			"show_incoming": true,
			"incoming_summary": {
				"refs": 2,
				"files": 1,
				"contexts": 1,
				"prefixes": 1,
				"dominant_prefix": "src",
				"kinds": [{"name": "calls", "count": 2}],
				"top_actors": [{"name": "caller()", "count": 2}],
				"top_prefixes": [{"name": "src", "count": 2}],
				"shared_helper_signal": "localized_not_shared"
			},
			"show_outgoing": false,
			"compact_map": null,
			"usages": [],
			"next_calls": []
		}))
		.expect("usages template")
		.render(super::RenderOptions {
			compact: false,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("render usages");

		let lines = output.lines().collect::<Vec<_>>();
		for item in [
			"- completeness: full",
			"- direction: `incoming`",
			"- file: `src/lib.rs`",
			"- language: `rs`",
			"- path: *",
			"- kinds: `calls`=2",
			"- top actors: `caller()`=2",
			"- top prefixes: `src`=2",
		] {
			assert!(
				lines.contains(&item),
				"missing separate item `{item}`:\n{output}"
			);
		}
		for fused in [
			"full- direction",
			"`src/lib.rs`- language",
			"- path: *- language",
			"`calls`=2- top actors",
			"`caller()`=2- top prefixes",
		] {
			assert!(
				!output.contains(fused),
				"fused Markdown items `{fused}`:\n{output}"
			);
		}
		validate_agent_markdown(&output, "Symbol usages", false)
			.expect("usages metadata remains valid CommonMark");
	}

	#[test]
	fn md_block_neutralizes_html_and_headings_in_an_agent_template() {
		let output = crate::presentation::relationships::diff(&serde_json::json!({
			"volume": "small",
			"max_items": 50,
			"result": {
				"scope": "HEAD..worktree\n# injected scope",
				"summary": {
					"files": 1,
					"analyzable_files": 1,
					"symbol_changes": 1,
					"ref_changes": 0,
					"retargeted_refs": 0,
					"residual_files": 0
				},
				"diagnostics": ["<script>alert(1)</script>\n## injected diagnostic"]
			},
			"files": [{
				"path": "src/safe.rs\n# injected file heading",
				"disposition": "modified <b>unsafe</b>",
				"analyzable": true,
				"coverage_explained": true
			}],
			"files_omitted": 0,
			"symbols": [{
				"change_kind": "added\n# injected symbol heading",
				"symbol_kind": "fn",
				"identity": "code+moniker://./fn:safe()\n<h1>unsafe</h1>",
				"confidence": "certain"
			}],
			"symbols_omitted": 0,
			"refs": null,
			"refs_omitted": 0,
			"next_call": null
		}))
		.expect("adversarial diff template")
		.render(super::RenderOptions {
			compact: false,
			scheme: "code+moniker://",
			runtime: None,
		})
		.expect("render adversarial diff");

		validate_agent_markdown(&output, "Semantic diff", false)
			.expect("project values cannot inject CommonMark structure");
		assert!(!output.lines().any(|line| matches!(
			line,
			"# injected scope"
				| "# injected file heading"
				| "# injected symbol heading"
				| "## injected diagnostic"
		)));
		assert!(!output.contains("<script>"));
	}
}
