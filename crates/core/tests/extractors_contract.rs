use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_core::lang::{self, LangExtractor};
use toml::Value;

fn anchor() -> Moniker {
	MonikerBuilder::new().project(b"app").build()
}

fn check<L: LangExtractor>(path: &Path) {
	let spec_path = expectation_path(path);
	assert!(
		spec_path.exists(),
		"{} has no expectation fixture {}",
		path.display(),
		spec_path.display()
	);

	let src = std::fs::read_to_string(path).expect("read fixture");
	let spec_src = std::fs::read_to_string(&spec_path).expect("read expectation fixture");
	let spec = spec_src.parse::<Value>().unwrap_or_else(|err| {
		panic!("parse expectation {}: {err}", spec_path.display());
	});
	assert_markers(path, &spec_path, &src, &spec);

	let uri = path
		.file_name()
		.expect("fixture has file name")
		.to_string_lossy()
		.into_owned();
	let a = anchor();
	let graph = L::extract(&uri, &src, &a, false, &L::Presets::default());
	assert_expectations(path, &spec_path, &graph, &spec);
}

fn expectation_path(path: &Path) -> PathBuf {
	let file_name = path
		.file_name()
		.expect("fixture has file name")
		.to_string_lossy();
	path.with_file_name(format!("{file_name}.expect.toml"))
}

fn assert_expectations(fixture: &Path, spec_path: &Path, graph: &CodeGraph, spec: &Value) {
	let root = render(graph.root());
	if let Some(expected_root) = string_at(spec, "root") {
		assert_eq!(
			root,
			expected_root,
			"{} root expectation from {}",
			fixture.display(),
			spec_path.display()
		);
	}

	for (idx, entry) in array_at(spec, "defs").iter().enumerate() {
		let matcher = Matcher::from_value(entry, &format!("defs[{idx}]"), spec_path);
		assert!(
			graph.defs().any(|def| matcher.matches_def(graph, def)),
			"{} missing expected def from {}: {}",
			fixture.display(),
			spec_path.display(),
			matcher.describe()
		);
	}

	for (idx, entry) in array_at(spec, "refs").iter().enumerate() {
		let matcher = Matcher::from_value(entry, &format!("refs[{idx}]"), spec_path);
		assert!(
			graph.refs().any(|r| matcher.matches_ref(graph, r)),
			"{} missing expected ref from {}: {}",
			fixture.display(),
			spec_path.display(),
			matcher.describe()
		);
	}

	if let Some(absent) = spec.get("absent") {
		for (idx, entry) in array_at(absent, "defs").iter().enumerate() {
			let matcher = Matcher::from_value(entry, &format!("absent.defs[{idx}]"), spec_path);
			assert!(
				!graph.defs().any(|def| matcher.matches_def(graph, def)),
				"{} found forbidden def from {}: {}",
				fixture.display(),
				spec_path.display(),
				matcher.describe()
			);
		}
		for (idx, entry) in array_at(absent, "refs").iter().enumerate() {
			let matcher = Matcher::from_value(entry, &format!("absent.refs[{idx}]"), spec_path);
			assert!(
				!graph.refs().any(|r| matcher.matches_ref(graph, r)),
				"{} found forbidden ref from {}: {}",
				fixture.display(),
				spec_path.display(),
				matcher.describe()
			);
		}
	}
}

fn assert_markers(fixture: &Path, spec_path: &Path, source: &str, spec: &Value) {
	let markers = collect_markers(source);
	let labels = collect_spec_labels(spec, spec_path);

	for (label, line) in &markers {
		assert!(
			labels.contains_key(label),
			"{} marker `{label}` at line {line} has no matching `label` in {}",
			fixture.display(),
			spec_path.display()
		);
	}
	for (label, section) in &labels {
		assert!(
			markers.contains_key(label),
			"{} `{label}` in {section} has no `cm:` marker in {}",
			spec_path.display(),
			fixture.display()
		);
	}
}

fn collect_markers(source: &str) -> BTreeMap<String, usize> {
	let mut out = BTreeMap::new();
	for (idx, line) in source.lines().enumerate() {
		let Some(rest) = line.split_once("cm:").map(|(_, rest)| rest.trim()) else {
			continue;
		};
		let Some((kind, label)) = rest.split_once(char::is_whitespace) else {
			continue;
		};
		if !matches!(kind, "def" | "ref" | "absent") {
			continue;
		}
		let label = label.trim();
		if label.is_empty() {
			continue;
		}
		assert!(
			out.insert(label.to_owned(), idx + 1).is_none(),
			"duplicate cm marker label `{label}` at line {}",
			idx + 1
		);
	}
	out
}

fn collect_spec_labels(spec: &Value, spec_path: &Path) -> BTreeMap<String, String> {
	let mut labels = BTreeMap::new();
	collect_labels_from_array(spec, "defs", spec_path, &mut labels);
	collect_labels_from_array(spec, "refs", spec_path, &mut labels);
	if let Some(absent) = spec.get("absent") {
		collect_labels_from_array(absent, "defs", spec_path, &mut labels);
		collect_labels_from_array(absent, "refs", spec_path, &mut labels);
	}
	labels
}

fn collect_labels_from_array(
	value: &Value,
	key: &str,
	spec_path: &Path,
	labels: &mut BTreeMap<String, String>,
) {
	for (idx, entry) in array_at(value, key).iter().enumerate() {
		let Some(label) = table_string(entry, "label") else {
			continue;
		};
		let section = format!("{key}[{idx}]");
		assert!(
			labels.insert(label.clone(), section.clone()).is_none(),
			"{} duplicate label `{label}` in {section}",
			spec_path.display()
		);
	}
}

#[derive(Debug)]
struct Matcher {
	label: Option<String>,
	kind: Option<String>,
	moniker: Option<String>,
	moniker_contains: Option<String>,
	parent: Option<String>,
	from: Option<String>,
	to: Option<String>,
	to_contains: Option<String>,
	visibility: Option<String>,
	signature: Option<String>,
	binding: Option<String>,
	origin: Option<String>,
	confidence: Option<String>,
	alias: Option<String>,
	receiver_hint: Option<String>,
}

impl Matcher {
	fn from_value(value: &Value, label: &str, spec_path: &Path) -> Self {
		let table = value.as_table().unwrap_or_else(|| {
			panic!("{} {label} must be a table", spec_path.display());
		});
		for key in table.keys() {
			assert!(
				ALLOWED_MATCHER_FIELDS.contains(&key.as_str()),
				"{} {label}.{key} is not a supported expectation field",
				spec_path.display()
			);
		}
		let field = |name| -> Option<String> {
			table.get(name).map(|value| {
				value
					.as_str()
					.unwrap_or_else(|| {
						panic!("{} {label}.{name} must be a string", spec_path.display());
					})
					.to_owned()
			})
		};
		Self {
			label: field("label"),
			kind: field("kind"),
			moniker: field("moniker"),
			moniker_contains: field("moniker_contains"),
			parent: field("parent"),
			from: field("from"),
			to: field("to"),
			to_contains: field("to_contains"),
			visibility: field("visibility"),
			signature: field("signature"),
			binding: field("binding"),
			origin: field("origin"),
			confidence: field("confidence"),
			alias: field("alias"),
			receiver_hint: field("receiver_hint"),
		}
	}

	fn matches_def(&self, graph: &CodeGraph, def: &DefRecord) -> bool {
		let defs: Vec<&DefRecord> = graph.defs().collect();
		let moniker = render(&def.moniker);
		let parent = def
			.parent
			.and_then(|idx| defs.get(idx).copied())
			.map(|parent| render(&parent.moniker));

		let _ = &self.label;
		self.matches_text("kind", &self.kind, bytes(&def.kind))
			&& self.matches_text("moniker", &self.moniker, &moniker)
			&& self.matches_contains("moniker_contains", &self.moniker_contains, &moniker)
			&& self.matches_optional_text("parent", &self.parent, parent.as_deref())
			&& self.matches_text("visibility", &self.visibility, bytes(&def.visibility))
			&& self.matches_text("signature", &self.signature, bytes(&def.signature))
			&& self.matches_text("binding", &self.binding, bytes(&def.binding))
			&& self.matches_text("origin", &self.origin, bytes(&def.origin))
	}

	fn matches_ref(&self, graph: &CodeGraph, reference: &RefRecord) -> bool {
		let defs: Vec<&DefRecord> = graph.defs().collect();
		let from = defs.get(reference.source).map(|def| render(&def.moniker));
		let to = render(&reference.target);

		let _ = &self.label;
		self.matches_text("kind", &self.kind, bytes(&reference.kind))
			&& self.matches_optional_text("from", &self.from, from.as_deref())
			&& self.matches_text("to", &self.to, &to)
			&& self.matches_contains("to_contains", &self.to_contains, &to)
			&& self.matches_text("confidence", &self.confidence, bytes(&reference.confidence))
			&& self.matches_text("binding", &self.binding, bytes(&reference.binding))
			&& self.matches_text("alias", &self.alias, bytes(&reference.alias))
			&& self.matches_text(
				"receiver_hint",
				&self.receiver_hint,
				bytes(&reference.receiver_hint),
			)
	}

	fn matches_text(&self, _field: &str, expected: &Option<String>, actual: &str) -> bool {
		expected
			.as_deref()
			.is_none_or(|expected| expected == actual)
	}

	fn matches_optional_text(
		&self,
		_field: &str,
		expected: &Option<String>,
		actual: Option<&str>,
	) -> bool {
		expected
			.as_deref()
			.is_none_or(|expected| actual == Some(expected))
	}

	fn matches_contains(&self, _field: &str, expected: &Option<String>, actual: &str) -> bool {
		expected
			.as_deref()
			.is_none_or(|expected| actual.contains(expected))
	}

	fn describe(&self) -> String {
		format!("{self:?}")
	}
}

const ALLOWED_MATCHER_FIELDS: &[&str] = &[
	"label",
	"kind",
	"moniker",
	"moniker_contains",
	"parent",
	"from",
	"to",
	"to_contains",
	"visibility",
	"signature",
	"binding",
	"origin",
	"confidence",
	"alias",
	"receiver_hint",
];

fn render(moniker: &Moniker) -> String {
	to_uri(moniker, &UriConfig::default()).unwrap_or_else(|err| format!("<urierr:{err}>"))
}

fn bytes(bytes: &[u8]) -> &str {
	std::str::from_utf8(bytes).unwrap_or("<non-utf8>")
}

fn string_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
	value.get(key).map(|value| {
		value
			.as_str()
			.unwrap_or_else(|| panic!("{key} must be a string"))
	})
}

fn array_at<'a>(value: &'a Value, key: &str) -> &'a [Value] {
	value
		.get(key)
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.unwrap_or(&[])
}

fn table_string(value: &Value, key: &str) -> Option<String> {
	value
		.as_table()
		.and_then(|table| table.get(key))
		.map(|value| {
			value
				.as_str()
				.unwrap_or_else(|| panic!("{key} must be a string"))
				.to_owned()
		})
}

macro_rules! expectation_lang {
	($name:ident, $glob:literal, $lang:ty) => {
		#[test]
		fn $name() {
			insta::glob!($glob, |p| check::<$lang>(p));
		}
	};
}

expectation_lang!(
	expect_ts,
	"fixtures/extractors/ts/*.{ts,tsx,jsx}",
	lang::ts::Lang
);
expectation_lang!(expect_rs, "fixtures/extractors/rs/*.rs", lang::rs::Lang);
expectation_lang!(
	expect_python,
	"fixtures/extractors/python/*.py",
	lang::python::Lang
);
expectation_lang!(expect_go, "fixtures/extractors/go/*.go", lang::go::Lang);
expectation_lang!(
	expect_java,
	"fixtures/extractors/java/*.java",
	lang::java::Lang
);
expectation_lang!(expect_cs, "fixtures/extractors/cs/*.cs", lang::cs::Lang);
expectation_lang!(expect_sql, "fixtures/extractors/sql/*.sql", lang::sql::Lang);

#[test]
fn rejects_unknown_expectation_fields() {
	let spec_path = Path::new("bad.expect.toml");
	let value = r#"
[[refs]]
kind = "calls"
confidnece = "resolved"
"#
	.parse::<Value>()
	.expect("test TOML parses");

	let result = std::panic::catch_unwind(|| {
		let _ = Matcher::from_value(&array_at(&value, "refs")[0], "refs[0]", spec_path);
	});
	assert!(
		result.is_err(),
		"unknown fields must fail instead of weakening the expectation"
	);
}
