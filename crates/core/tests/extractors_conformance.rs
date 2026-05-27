use std::path::Path;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::lang::{self, LangExtractor, assert_conformance};

fn anchor() -> Moniker {
	MonikerBuilder::new().project(b"app").build()
}

fn check<L: LangExtractor>(path: &Path) {
	let src = std::fs::read_to_string(path).expect("read fixture");
	let uri = path
		.file_name()
		.expect("fixture has file name")
		.to_string_lossy()
		.into_owned();
	let a = anchor();
	let g = L::extract(&uri, &src, &a, false, &L::Presets::default());
	assert_conformance::<L>(&g, &a);
}

macro_rules! conformance_lang {
	($name:ident, $glob:literal, $lang:ty) => {
		#[test]
		fn $name() {
			insta::glob!($glob, |p| check::<$lang>(p));
		}
	};
}

conformance_lang!(
	conformance_ts,
	"fixtures/extractors/ts/*.{ts,tsx,jsx}",
	lang::ts::Lang
);
conformance_lang!(
	conformance_rs,
	"fixtures/extractors/rs/*.rs",
	lang::rs::Lang
);
conformance_lang!(
	conformance_python,
	"fixtures/extractors/python/*.py",
	lang::python::Lang
);
conformance_lang!(
	conformance_go,
	"fixtures/extractors/go/*.go",
	lang::go::Lang
);
conformance_lang!(
	conformance_java,
	"fixtures/extractors/java/*.java",
	lang::java::Lang
);
conformance_lang!(
	conformance_cs,
	"fixtures/extractors/cs/*.cs",
	lang::cs::Lang
);
conformance_lang!(
	conformance_sql,
	"fixtures/extractors/sql/*.sql",
	lang::sql::Lang
);
