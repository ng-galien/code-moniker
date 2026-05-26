use anyhow::{Context, anyhow, bail};
use common_model::risk::{self, RiskPolicy};
use common_model::{Auditable, CustomerId, OrderTotal, Region};
use serde::{Deserialize, Serialize as Ser};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::write;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Instant;
use roxmltree::Node as XmlNode;
use tree_sitter::Node;

use proptest::prelude::*;

pub mod types;
pub mod feature;
pub mod module_group;

use self::types::*;
use crate::module_group::nested::{self, marker as nested_marker};
use crate::module_group::nested::inner::marker as inner_marker;

pub struct OrderService;

pub struct Cli;

pub struct LocalGraph;

impl LocalGraph {
	pub fn with_capacity() -> Self {
		Self
	}

	pub fn add_def(&mut self) {}
}

pub struct OrderKey(String);

impl OrderKey {
	pub fn from_raw(raw: String) -> Self {
		Self(raw)
	}
}

impl PartialEq for OrderKey {
	fn eq(&self, other: &Self) -> bool {
		self.0 == other.0
	}
}

impl Eq for OrderKey {}

pub trait ThreadSafeOrder: Send + Sync {}

#[derive(PartialEq)]
pub enum SubmissionState {
	Match,
	NoMatch,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireState {
	Ready,
}

#[derive(Ser)]
pub struct SerializableWireState {
	state: String,
}

#[derive(serde::Serialize)]
pub struct QualifiedSerializableWireState {
	state: String,
}

#[derive(Default)]
pub enum DefaultWireState {
	#[default]
	Ready,
}

pub enum OrderError {
	Duplicate { id: String },
	Io { path: String },
}

pub mod errors {
	pub struct LocalError;
}

pub mod constants {
	pub use common_model::DEFAULT_REGION;
}

pub struct PathCollector {
	paths: Vec<PathBuf>,
}

impl PathCollector {
	pub fn new() -> Self {
		Self { paths: Vec::new() }
	}

	pub fn push_path(&mut self, path: PathBuf) {
		self.paths.push(path);
	}
}

impl Default for PathCollector {
	fn default() -> Self {
		Self::new()
	}
}

impl Auditable for OrderService {}

impl OrderService {
	pub fn submit(customer: CustomerId, total: OrderTotal) -> RiskPolicy {
		let _total = total;
		let normalized = common_model::normalize_customer(customer);
		risk::assess(&normalized)
	}

	pub fn accepted(state: SubmissionState) -> bool {
		state == SubmissionState::Match
	}
}

pub fn normalized_paths(raw: Vec<PathBuf>) -> Result<Option<PathBuf>, String> {
	let mut paths = Vec::new();
	for path in raw {
		paths.push(path.clone());
	}
	Ok(paths.pop())
}

pub fn collect_path(path: PathBuf) -> PathCollector {
	let mut collector = PathCollector::new();
	collector.push_path(path);
	collector
}

pub fn child_kind(node: Node<'_>) -> Option<&str> {
	node.child_by_field_name("name").map(|child| child.kind())
}

pub fn child_kind_by_ref(node: &Node<'_>) -> &str {
	node.kind()
}

pub fn xml_tag_name_by_ref(node: &XmlNode<'_, '_>) {
	let _ = node.tag_name();
}

pub fn external_for_items(root: XmlNode<'_, '_>) {
	for node in root.descendants() {
		let _ = node.tag_name();
	}
}

pub fn max_retry_count() -> u32 {
	u32::MAX
}

pub fn successful_paths(raw: Vec<Result<PathBuf, String>>) -> Vec<PathBuf> {
	raw.into_iter().filter_map(Result::ok).collect()
}

pub fn path_iterator(raw: Vec<PathBuf>) -> impl Iterator<Item = PathBuf> {
	raw.into_iter()
}

pub fn optional_path(path: Option<PathBuf>) -> Option<PathBuf> {
	path.and_then(Some)
}

pub fn first_path(raw: Vec<PathBuf>) -> Option<PathBuf> {
	raw.get(0).cloned()
}

pub fn owned_label() -> String {
	"order".into()
}

pub fn empty_label() -> String {
	String::new()
}

pub fn boxed_label() -> Box<String> {
	Box::new(String::new())
}

pub fn parse_cli() {
	let _ = Cli::parse_from(["order"]);
}

pub fn local_graph_methods() {
	let mut graph = LocalGraph::with_capacity();
	graph.add_def();
}

pub fn local_type_std_trait_method() {
	let graph = LocalGraph::with_capacity();
	let _ = graph.to_string();
}

pub fn local_enum_glob(state: SubmissionState) -> bool {
	use SubmissionState::*;
	matches!(state, Match | SubmissionState::NoMatch)
}

pub fn imported_enum_glob(state: ImportedState) -> bool {
	use ImportedState::*;
	matches!(state, Open | ImportedState::Closed)
}

pub fn nested_module_marker() -> &'static str {
	nested_marker()
}

pub fn nested_self_module_marker() -> &'static str {
	nested::marker()
}

pub fn nested_inner_module_marker() -> &'static str {
	inner_marker()
}

pub fn local_report_shape() -> usize {
	#[derive(serde::Serialize)]
	struct Summary {
		total: usize,
	}
	#[derive(serde::Serialize)]
	struct Entry<'a> {
		summary: &'a Summary,
	}
	let summary = Summary { total: 1 };
	let entry = Entry { summary: &summary };
	entry.summary.total
}

pub fn marker_path() -> &'static std::path::Path {
	std::path::Path::new("marker")
}

pub fn write_marker(path: &PathBuf) -> std::io::Result<()> {
	write(path, b"marker")
}

pub fn write_marker_qualified(path: &PathBuf) -> std::io::Result<()> {
	std::fs::write(path, b"marker")
}

pub fn decoded_label(bytes: Vec<u8>) -> Result<String, std::string::FromUtf8Error> {
	String::from_utf8(bytes)
}

pub fn wildcard_type(value: WildcardType) -> WildcardType {
	value
}

pub fn crate_path_type(value: crate::types::WildcardType) -> crate::types::WildcardType {
	value
}

pub fn accepts_path_visitor<F: FnMut(PathBuf)>(_visitor: F) {}

pub fn duplicate_error(id: String) -> OrderError {
	OrderError::Duplicate { id }
}

pub fn scoped_local_error(value: errors::LocalError) -> errors::LocalError {
	value
}

pub fn imported_region() -> Region {
	Region::Eu
}

pub fn reexported_region_code() -> &'static str {
	constants::DEFAULT_REGION
}

pub fn customer_formatter() -> fn(&CustomerId) -> &'static str {
	CustomerId::tag
}

pub fn macro_error(flag: bool) -> anyhow::Result<()> {
	if flag {
		bail!("flagged");
	}
	Err(anyhow!("flagged"))
}

pub fn contextual_error(flag: bool) -> anyhow::Result<()> {
	if flag {
		return Ok(());
	}
	Err(anyhow!("missing")).with_context(|| "order context")
}

pub fn external_error_macro() {
	error!("order failed");
}

pub fn snapshot_macro_path() {
	insta::assert_json_snapshot!("order");
}

proptest! {
	#[test]
	fn generated_order_id(value in 0u8..10) {
		let _ = value;
	}
}

pub fn external_language_constant() {
	let _language = tree_sitter_rust::LANGUAGE;
}

pub fn external_builder_chain() {
	let _block = ratatui::widgets::Block::default()
		.borders(ratatui::widgets::Borders::ALL)
		.title("order");
}

pub fn external_qualified_frame(frame: &mut ratatui::Frame<'_>) {
	frame.render_widget("order", ratatui::layout::Rect::default());
}

pub fn standard_method_surface(
	mut labels: Vec<String>,
	maybe: Option<String>,
	number: u32,
	mut map: BTreeMap<String, String>,
) -> Result<usize, String> {
	let count = labels.iter().count();
	let label = maybe
		.ok_or_else(|| "missing".to_string())
		.or_else(|_| Ok(String::new()))?;
	labels.push(label);
	let mut text = String::new();
	text.push_str("order");
	let _chars = text.chars().count();
	let _bytes = number.to_le_bytes();
	let _entry = map.entry("order".to_string()).or_default();
	let _removed = map.remove("order");
	let _parts = "a,b".split(',').count();
	Ok(count)
}

pub fn path_display(path: &std::path::Path) -> String {
	path.display().to_string()
}

pub fn unwrap_error(value: Result<(), String>) -> String {
	value.unwrap_err()
}

pub fn remaining_std_methods(
	maybe: Option<String>,
	mut map: BTreeMap<String, String>,
	path: &std::path::Path,
	root: &std::path::Path,
) -> Result<(), String> {
	let _ = maybe.ok_or_else(|| "missing".to_string())?;
	let _ = map.entry("order".to_string());
	let _ = map.remove("order");
	let _ = Err::<(), String>("error".to_string()).unwrap_err();
	let _ = Ordering::Equal.then_with(|| Ordering::Less);
	let mut out = Vec::new();
	out.write_all(b"order").map_err(|err| err.to_string())?;
	let _ = Instant::now().elapsed();
	let labels = vec!["a".to_string(), "b".to_string()];
	let _ = labels.iter().find_map(|label| label.starts_with('a').then_some(label));
	let _ = labels.first();
	let _ = path.strip_prefix(root);
	Ok(())
}

pub fn extended_std_methods(
	path: &std::path::Path,
	mutex: &Mutex<String>,
	lock: &RwLock<Vec<u8>>,
) -> Result<(), String> {
	let byte = b'a';
	let _ = byte.is_ascii_lowercase();
	let _ = byte.is_ascii_uppercase();
	let _ = byte.is_ascii_alphabetic();
	let _ = byte.is_ascii_whitespace();
	let _converted: u8 = 1u16.try_into().map_err(|err| err.to_string())?;
	let mut bytes = [0u8; 4];
	bytes.copy_from_slice(&[1, 2, 3, 4]);
	let value: Result<u8, String> = Ok(1);
	let _ = value.is_ok();
	let mut nums = vec![3, 1, 2];
	nums.sort_by(|left, right| left.cmp(right));
	let _ = nums.binary_search(&1);
	let _ = nums.iter().rev().take(1).zip(nums.iter()).count();
	let _ = nums.windows(2);
	let _ = nums.split_first();
	for n in nums.iter_mut() {
		*n = (*n).clamp(0, 9);
	}
	nums.retain(|n| *n > 0);
	let _ = "Order".to_ascii_lowercase();
	let _ = "a,b".rsplit(',').nth(0);
	let _ = "abc".trim_end_matches('c');
	let _ = "abc".bytes().count();
	let _ = "abc".char_indices().count();
	let nested = [vec![1], vec![2]];
	let _ = nested.iter().flat_map(|items| items.iter()).sum::<i32>();
	let mut map = BTreeMap::new();
	map.insert("order".to_string(), "ready".to_string());
	let _ = map.contains_key("order");
	let _ = map.get_mut("order");
	let _ = map.keys().count();
	let cell = OnceLock::new();
	let _ = cell.get_or_init(|| "order".to_string());
	let cow: std::borrow::Cow<'_, str> = "order".into();
	let _ = cow.into_owned();
	let _ = Instant::now().elapsed().as_nanos();
	let _ = nums.as_ptr();
	let _ = path.is_dir();
	let _ = path.is_absolute();
	let _guard = mutex.lock().map_err(|err| err.to_string())?;
	let _read = lock.read().map_err(|err| err.to_string())?;
	Ok(())
}

#[pg_operator(immutable, parallel_safe)]
#[opname(@>)]
#[commutator(<@)]
#[negator(?)]
pub fn contains_marker(left: i32, right: i32) -> bool {
	left >= right
}

#[cfg(test)]
mod tests {
	#[test]
	#[should_panic]
	fn panic_contract() {
		panic!("expected");
	}
}
