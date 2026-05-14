use std::path::Path;

use code_moniker_core::core::code_graph::{CodeGraph, DefRecord, RefRecord};
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_core::lang::{self, LangExtractor};
use serde_json::{Map, Value, json};

fn anchor() -> Moniker {
	MonikerBuilder::new().project(b"app").build()
}

fn snap<L: LangExtractor>(path: &Path) -> String {
	let src = std::fs::read_to_string(path).expect("read fixture");
	let uri = path
		.file_name()
		.expect("fixture has file name")
		.to_string_lossy()
		.into_owned();
	let a = anchor();
	let g = L::extract(&uri, &src, &a, false, &L::Presets::default());
	serde_json::to_string_pretty(&dump(&g)).expect("pretty json")
}

fn dump(g: &CodeGraph) -> Value {
	let cfg = UriConfig::default();
	let defs: Vec<&DefRecord> = g.defs().collect();
	let render =
		|m: &Moniker| -> String { to_uri(m, &cfg).unwrap_or_else(|e| format!("<urierr:{e}>")) };

	let defs_json: Vec<Value> = defs
		.iter()
		.skip(1)
		.map(|d| {
			let parent = d
				.parent
				.and_then(|i| defs.get(i).copied())
				.map(|p| &p.moniker);
			def_entry(d, parent, &render)
		})
		.collect();

	let refs_json: Vec<Value> = g
		.refs()
		.map(|r| {
			let from = defs.get(r.source).map(|d| &d.moniker);
			ref_entry(r, from, &render)
		})
		.collect();

	json!({
		"root":  render(g.root()),
		"defs":  defs_json,
		"refs":  refs_json,
	})
}

fn def_entry<F: Fn(&Moniker) -> String>(
	d: &DefRecord,
	parent: Option<&Moniker>,
	render: &F,
) -> Value {
	let mut m = Map::with_capacity(7);
	m.insert("kind".into(), s(&d.kind));
	m.insert("moniker".into(), Value::String(render(&d.moniker)));
	if let Some(p) = parent {
		m.insert("parent".into(), Value::String(render(p)));
	}
	m.insert("visibility".into(), s(&d.visibility));
	m.insert("signature".into(), s(&d.signature));
	m.insert("binding".into(), s(&d.binding));
	m.insert("origin".into(), s(&d.origin));
	Value::Object(m)
}

fn ref_entry<F: Fn(&Moniker) -> String>(
	r: &RefRecord,
	from: Option<&Moniker>,
	render: &F,
) -> Value {
	let mut m = Map::with_capacity(7);
	m.insert("kind".into(), s(&r.kind));
	m.insert(
		"from".into(),
		Value::String(from.map(render).unwrap_or_else(|| "<unknown>".into())),
	);
	m.insert("to".into(), Value::String(render(&r.target)));
	m.insert("confidence".into(), s(&r.confidence));
	m.insert("binding".into(), s(&r.binding));
	m.insert("alias".into(), s(&r.alias));
	m.insert("receiver_hint".into(), s(&r.receiver_hint));
	Value::Object(m)
}

fn s(bytes: &[u8]) -> Value {
	Value::String(String::from_utf8_lossy(bytes).into_owned())
}

macro_rules! snapshot_lang {
	($name:ident, $glob:literal, $lang:ty) => {
		#[test]
		fn $name() {
			insta::glob!($glob, |p| {
				let body = snap::<$lang>(p);
				insta::assert_snapshot!(body);
			});
		}
	};
}

snapshot_lang!(snapshot_ts, "fixtures/ts/*.ts", lang::ts::Lang);
snapshot_lang!(snapshot_rs, "fixtures/rs/*.rs", lang::rs::Lang);
snapshot_lang!(snapshot_python, "fixtures/python/*.py", lang::python::Lang);
snapshot_lang!(snapshot_go, "fixtures/go/*.go", lang::go::Lang);
snapshot_lang!(snapshot_java, "fixtures/java/*.java", lang::java::Lang);
snapshot_lang!(snapshot_cs, "fixtures/cs/*.cs", lang::cs::Lang);
snapshot_lang!(snapshot_sql, "fixtures/sql/*.sql", lang::sql::Lang);
