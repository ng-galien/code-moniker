use std::collections::BTreeMap;
use std::path::PathBuf;

use code_moniker_core::core::shape::Shape;

#[derive(Clone, Debug)]
pub struct SessionOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DefLocation {
	pub file: usize,
	pub def: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RefLocation {
	pub file: usize,
	pub reference: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionStats {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
	pub by_lang: BTreeMap<&'static str, LangTotals>,
	pub by_shape: BTreeMap<&'static str, usize>,
	pub by_def_kind: BTreeMap<String, usize>,
	pub by_ref_kind: BTreeMap<String, usize>,
	pub scan_ms: u64,
	pub extract_ms: u64,
	pub index_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LangTotals {
	pub files: usize,
	pub defs: usize,
	pub refs: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewFilter {
	pub kind: Option<String>,
	pub name: Option<String>,
	pub shape: Option<Shape>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckSummary {
	pub files_scanned: usize,
	pub files_with_violations: usize,
	pub total_violations: usize,
	pub errors: Vec<CheckError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
	pub path: PathBuf,
	pub error: String,
}
