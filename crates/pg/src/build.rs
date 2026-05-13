#![allow(clippy::type_complexity)]

use std::error::Error;

use pgrx::iter::TableIterator;
use pgrx::prelude::*;

use code_moniker_core::lang::build_manifest::{Dep, Manifest, parse};

use crate::moniker::moniker;

type PgError = Box<dyn Error + Send + Sync + 'static>;

fn rows_for(
	anchor: &moniker,
	manifest: Manifest,
	content: &str,
) -> Result<Vec<(moniker, String, Option<String>, String, String)>, PgError> {
	let view = anchor.view();
	let deps = parse(manifest, view.project(), content)?;
	Ok(deps
		.into_iter()
		.map(|d: Dep| {
			(
				moniker::from_core(d.package_moniker),
				d.name,
				d.version,
				d.dep_kind,
				d.import_root,
			)
		})
		.collect())
}

#[pg_extern(immutable, parallel_safe)]
fn extract_cargo(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::Cargo,
		content,
	)?))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_package_json(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::PackageJson,
		content,
	)?))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_pom_xml(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::PomXml,
		content,
	)?))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_pyproject(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::Pyproject,
		content,
	)?))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_go_mod(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::GoMod,
		content,
	)?))
}

#[pg_extern(immutable, parallel_safe)]
fn extract_csproj(
	anchor: moniker,
	content: &str,
) -> Result<
	TableIterator<
		'static,
		(
			name!(package_moniker, moniker),
			name!(name, String),
			name!(version, Option<String>),
			name!(dep_kind, String),
			name!(import_root, String),
		),
	>,
	PgError,
> {
	Ok(TableIterator::new(rows_for(
		&anchor,
		Manifest::Csproj,
		content,
	)?))
}
