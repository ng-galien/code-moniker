use pgrx::iter::TableIterator;
use pgrx::prelude::*;

use crate::lang::java::build as pom_xml;
use crate::lang::python::build as pyproject;
use crate::lang::rs::build as cargo;
use crate::lang::ts::build as package_json;

#[pg_extern(immutable, parallel_safe)]
fn extract_cargo(
	content: &str,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let deps = cargo::parse(content).unwrap_or_else(|e| error!("{e}"));
	rows_from(deps.into_iter().map(Into::into))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_package_json(
	content: &str,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let deps = package_json::parse(content).unwrap_or_else(|e| error!("{e}"));
	rows_from(deps.into_iter().map(Into::into))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_pom_xml(
	content: &str,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let deps = pom_xml::parse(content).unwrap_or_else(|e| error!("{e}"));
	rows_from(deps.into_iter().map(Into::into))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_pyproject(
	content: &str,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let deps = pyproject::parse(content).unwrap_or_else(|e| error!("{e}"));
	rows_from(deps.into_iter().map(Into::into))
}

fn rows_from<I: Iterator<Item = Dep>>(
	deps: I,
) -> TableIterator<
	'static,
	(
		name!(name, String),
		name!(version, Option<String>),
		name!(dep_kind, String),
		name!(import_root, String),
	),
> {
	let rows = deps
		.map(|d| (d.name, d.version, d.dep_kind, d.import_root))
		.collect::<Vec<_>>();
	TableIterator::new(rows)
}

struct Dep {
	name: String,
	version: Option<String>,
	dep_kind: String,
	import_root: String,
}

impl From<cargo::Dep> for Dep {
	fn from(d: cargo::Dep) -> Self {
		Self {
			name: d.name,
			version: d.version,
			dep_kind: d.dep_kind,
			import_root: d.import_root,
		}
	}
}

impl From<package_json::Dep> for Dep {
	fn from(d: package_json::Dep) -> Self {
		Self {
			name: d.name,
			version: d.version,
			dep_kind: d.dep_kind,
			import_root: d.import_root,
		}
	}
}

impl From<pom_xml::Dep> for Dep {
	fn from(d: pom_xml::Dep) -> Self {
		Self {
			name: d.name,
			version: d.version,
			dep_kind: d.dep_kind,
			import_root: d.import_root,
		}
	}
}

impl From<pyproject::Dep> for Dep {
	fn from(d: pyproject::Dep) -> Self {
		Self {
			name: d.name,
			version: d.version,
			dep_kind: d.dep_kind,
			import_root: d.import_root,
		}
	}
}
