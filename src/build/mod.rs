//! Build-system manifest parsers. Pure Rust, no pgrx. The pgrx
//! wrappers in `pg/build.rs` expose these as SQL functions returning
//! `(name, version, dep_kind)` row sets that consumers ingest into
//! their own linkage tables.
//!
//! Adding a new ecosystem: drop a parser in this module, expose a
//! single `parse(content) -> Result<Vec<Dep>, Error>` function, and
//! add the SQL wrapper.

pub mod cargo;

/// One row produced by a manifest parser. `dep_kind` distinguishes
/// the package itself (`"package"`) from its dependency rows (e.g.
/// `"normal"`, `"dev"`, `"build"` for Cargo). `import_root` is the
/// form the dependency takes inside source code — for Cargo, hyphens
/// become underscores (`tree-sitter` → `tree_sitter`); for ecosystems
/// where source-form and manifest-form coincide it equals `name`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}
