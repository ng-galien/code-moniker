use std::error::Error;
use std::sync::OnceLock;

use pgrx::callconv::{Arg, ArgAbi, BoxRet, FcInfo};
use pgrx::datum::{Datum as PgrxDatum, FromDatum, IntoDatum, UnboxDatum};
use pgrx::iter::TableIterator;
use pgrx::memcxt::PgMemoryContexts;
use pgrx::prelude::*;
use pgrx::{InOutFuncs, StringInfo, default, name, varlena_to_byte_slice};

use crate::moniker::{moniker, palloc_varlena_from_slice, varlena_to_borrowed_bytes};
use crate::util::resolve_type_oid;
use code_moniker_core::core::code_graph::encoding;
use code_moniker_core::core::code_graph::{CodeGraph as CoreGraph, Position};

// code-moniker: ignore[name-pascalcase] — pgrx maps the Rust struct name 1:1 to the SQL type name, which must be `code_graph`.
#[allow(non_camel_case_types)]
#[derive(PostgresType, Debug)]
#[inoutfuncs]
#[bikeshed_postgres_type_manually_impl_from_into_datum]
pub struct code_graph {
	storage: GraphStorage,
}

impl InOutFuncs for code_graph {
	fn input(_input: &core::ffi::CStr) -> Self {
		error!("code_graph has no text input form");
	}

	fn output(&self, buffer: &mut StringInfo) {
		let core = self.to_core();
		buffer.push_str(&format!(
			"code_graph(defs={}, refs={})",
			core.def_count(),
			core.ref_count()
		));
	}
}

#[derive(Debug)]
enum GraphStorage {
	Owned(Vec<u8>),
	Borrowed { ptr: *const u8, len: u32 },
}

impl code_graph {
	pub(super) fn from_core(inner: CoreGraph) -> Self {
		let bytes = encoding::encode(&inner).unwrap_or_else(|e| error!("code_graph encode: {e}"));
		Self {
			storage: GraphStorage::Owned(bytes),
		}
	}

	fn from_owned_bytes(bytes: Vec<u8>) -> Self {
		Self {
			storage: GraphStorage::Owned(bytes),
		}
	}

	fn as_bytes(&self) -> &[u8] {
		match self.storage {
			GraphStorage::Owned(ref v) => v.as_slice(),
			GraphStorage::Borrowed { ptr, len } => unsafe {
				core::slice::from_raw_parts(ptr, len as usize)
			},
		}
	}

	pub(super) fn to_core(&self) -> CoreGraph {
		encoding::decode(self.as_bytes()).unwrap_or_else(|e| error!("code_graph decode: {e}"))
	}
}

impl Clone for code_graph {
	fn clone(&self) -> Self {
		Self::from_owned_bytes(self.as_bytes().to_vec())
	}
}

static CODE_GRAPH_TYPE_OID: OnceLock<pg_sys::Oid> = OnceLock::new();

fn code_graph_type_oid() -> pg_sys::Oid {
	*CODE_GRAPH_TYPE_OID.get_or_init(|| resolve_type_oid("code_graph"))
}

impl IntoDatum for code_graph {
	fn into_datum(self) -> Option<pg_sys::Datum> {
		Some(unsafe { palloc_varlena_from_slice(self.as_bytes()) })
	}

	fn type_oid() -> pg_sys::Oid {
		code_graph_type_oid()
	}
}

unsafe impl BoxRet for code_graph {
	unsafe fn box_into<'fcx>(self, fcinfo: &mut FcInfo<'fcx>) -> PgrxDatum<'fcx> {
		match IntoDatum::into_datum(self) {
			None => fcinfo.return_null(),
			Some(datum) => unsafe { fcinfo.return_raw_datum(datum) },
		}
	}
}

impl FromDatum for code_graph {
	unsafe fn from_polymorphic_datum(
		datum: pg_sys::Datum,
		is_null: bool,
		_typoid: pg_sys::Oid,
	) -> Option<Self> {
		if is_null || datum.is_null() {
			return None;
		}
		let bytes = unsafe { varlena_to_borrowed_bytes(datum) };
		Some(code_graph {
			storage: GraphStorage::Borrowed {
				ptr: bytes.as_ptr(),
				len: bytes.len() as u32,
			},
		})
	}

	unsafe fn from_datum_in_memory_context(
		mut memory_context: PgMemoryContexts,
		datum: pg_sys::Datum,
		is_null: bool,
		_typoid: pg_sys::Oid,
	) -> Option<Self> {
		if is_null || datum.is_null() {
			return None;
		}
		unsafe {
			memory_context.switch_to(|_| {
				let copied = pg_sys::pg_detoast_datum_copy(datum.cast_mut_ptr());
				let bytes = varlena_to_byte_slice(copied);
				Some(code_graph::from_owned_bytes(bytes.to_vec()))
			})
		}
	}
}

unsafe impl UnboxDatum for code_graph {
	type As<'dat>
		= Self
	where
		Self: 'dat;
	unsafe fn unbox<'dat>(datum: PgrxDatum<'dat>) -> Self::As<'dat>
	where
		Self: 'dat,
	{
		unsafe {
			<Self as FromDatum>::from_datum(
				::core::mem::transmute::<PgrxDatum<'dat>, pgrx::pg_sys::Datum>(datum),
				false,
			)
			.unwrap()
		}
	}
}

unsafe impl<'fcx> ArgAbi<'fcx> for code_graph
where
	Self: 'fcx,
{
	unsafe fn unbox_arg_unchecked(arg: Arg<'_, 'fcx>) -> Self {
		let index = arg.index();
		unsafe {
			arg.unbox_arg_using_from_datum()
				.unwrap_or_else(|| panic!("argument {index} must not be null"))
		}
	}
}

type PgError = Box<dyn Error + Send + Sync + 'static>;

#[pg_extern(immutable, parallel_safe)]
fn code_graph_to_bytea(graph: code_graph) -> Vec<u8> {
	graph.as_bytes().to_vec()
}

#[pg_extern(immutable, parallel_safe)]
fn code_graph_from_bytea(bytes: &[u8]) -> Result<code_graph, PgError> {
	encoding::decode(bytes)?;
	Ok(code_graph::from_owned_bytes(bytes.to_vec()))
}

#[pg_extern(immutable, parallel_safe)]
fn code_graph_to_cbor(graph: code_graph) -> Result<Vec<u8>, PgError> {
	Ok(serde_cbor::to_vec(&graph.to_core())?)
}

#[pg_extern(immutable, parallel_safe)]
fn code_graph_from_cbor(bytes: &[u8]) -> Result<code_graph, PgError> {
	let core: CoreGraph = serde_cbor::from_slice(bytes)?;
	Ok(code_graph::from_core(core))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_create(root: moniker, kind: &str) -> code_graph {
	code_graph::from_core(CoreGraph::new(root.into_core(), kind.as_bytes()))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_def(
	graph: code_graph,
	def: moniker,
	kind: &str,
	parent: moniker,
	start_byte: default!(Option<i32>, "NULL"),
	end_byte: default!(Option<i32>, "NULL"),
) -> Result<code_graph, PgError> {
	let mut next = graph.to_core();
	next.add_def(
		def.into_core(),
		kind.as_bytes(),
		&parent.to_core(),
		pos_from_args(start_byte, end_byte),
	)?;
	Ok(code_graph::from_core(next))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_ref(
	graph: code_graph,
	source: moniker,
	target: moniker,
	kind: &str,
	start_byte: default!(Option<i32>, "NULL"),
	end_byte: default!(Option<i32>, "NULL"),
) -> Result<code_graph, PgError> {
	let mut next = graph.to_core();
	next.add_ref(
		&source.to_core(),
		target.into_core(),
		kind.as_bytes(),
		pos_from_args(start_byte, end_byte),
	)?;
	Ok(code_graph::from_core(next))
}

fn pos_from_args(start: Option<i32>, end: Option<i32>) -> Option<Position> {
	match (start, end) {
		(Some(s), Some(e)) if s >= 0 && e >= 0 => Some((s as u32, e as u32)),
		_ => None,
	}
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_defs(
	graph: code_graph,
	defs: Vec<moniker>,
	kinds: Vec<String>,
	parents: Vec<moniker>,
) -> Result<code_graph, PgError> {
	if defs.len() != kinds.len() || defs.len() != parents.len() {
		return Err("graph_add_defs: arrays must have the same length".into());
	}
	let mut next = graph.to_core();
	for ((d, k), p) in defs.into_iter().zip(kinds).zip(parents) {
		next.add_def(d.into_core(), k.as_bytes(), &p.to_core(), None)?;
	}
	Ok(code_graph::from_core(next))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_add_refs(
	graph: code_graph,
	sources: Vec<moniker>,
	targets: Vec<moniker>,
	kinds: Vec<String>,
) -> Result<code_graph, PgError> {
	if sources.len() != targets.len() || sources.len() != kinds.len() {
		return Err("graph_add_refs: arrays must have the same length".into());
	}
	let mut next = graph.to_core();
	for ((s, t), k) in sources.into_iter().zip(targets).zip(kinds) {
		next.add_ref(&s.to_core(), t.into_core(), k.as_bytes(), None)?;
	}
	Ok(code_graph::from_core(next))
}

#[pg_extern(immutable, parallel_safe)]
fn graph_locate(
	graph: code_graph,
	m: moniker,
) -> TableIterator<'static, (name!(start_byte, Option<i32>), name!(end_byte, Option<i32>))> {
	let core = graph.to_core();
	let row = core.locate(&m.to_core()).map(|p| {
		let (s, e) = position_to_i32(Some(p));
		(s, e)
	});
	TableIterator::new(row)
}

#[pg_extern(immutable, parallel_safe)]
fn graph_root(graph: code_graph) -> Result<moniker, PgError> {
	let root = encoding::decode_root(graph.as_bytes())?;
	Ok(moniker::from_core(root))
}

#[pg_operator(immutable, parallel_safe)]
#[opname(@>)]
fn graph_contains(graph: code_graph, m: moniker) -> bool {
	graph.to_core().contains(&m.to_core())
}

#[pg_extern(immutable, parallel_safe)]
fn graph_def_monikers(graph: code_graph) -> Vec<moniker> {
	graph
		.to_core()
		.def_monikers()
		.iter()
		.map(|m| moniker::from_core(m.clone()))
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_ref_targets(graph: code_graph) -> Vec<moniker> {
	graph
		.to_core()
		.ref_targets()
		.iter()
		.map(|m| moniker::from_core(m.clone()))
		.collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_export_monikers(graph: code_graph) -> Vec<moniker> {
	use code_moniker_core::core::kinds::{BIND_EXPORT, BIND_INJECT};
	let core = graph.to_core();
	let mut out: Vec<code_moniker_core::core::moniker::Moniker> = core
		.defs()
		.filter(|d| d.binding == BIND_EXPORT || d.binding == BIND_INJECT)
		.map(|d| d.moniker.clone())
		.collect();
	out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
	out.into_iter().map(moniker::from_core).collect()
}

#[pg_extern(immutable, parallel_safe)]
fn graph_import_targets(graph: code_graph) -> Vec<moniker> {
	use code_moniker_core::core::kinds::{BIND_IMPORT, BIND_INJECT};
	let core = graph.to_core();
	let mut out: Vec<code_moniker_core::core::moniker::Moniker> = core
		.refs()
		.filter(|r| r.binding == BIND_IMPORT || r.binding == BIND_INJECT)
		.map(|r| r.target.clone())
		.collect();
	out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
	out.into_iter().map(moniker::from_core).collect()
}

fn kind_text(bytes: &[u8]) -> String {
	String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| {
		error!("graph kind tag must be UTF-8");
	})
}

#[pg_extern(immutable, parallel_safe)]
#[allow(clippy::type_complexity)]
fn graph_defs(
	graph: code_graph,
) -> TableIterator<
	'static,
	(
		name!(moniker, moniker),
		name!(kind, String),
		name!(shape, Option<String>),
		name!(opens_scope, bool),
		name!(visibility, Option<String>),
		name!(signature, Option<String>),
		name!(binding, Option<String>),
		name!(origin, Option<String>),
		name!(start_byte, Option<i32>),
		name!(end_byte, Option<i32>),
	),
> {
	let core = graph.to_core();
	let rows: Vec<(
		moniker,
		String,
		Option<String>,
		bool,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<i32>,
		Option<i32>,
	)> = core
		.defs()
		.map(|d| {
			let (start, end) = position_to_i32(d.position);
			(
				moniker::from_core(d.moniker.clone()),
				kind_text(&d.kind),
				d.shape().map(|s| s.as_str().to_string()),
				d.opens_scope(),
				bytes_to_opt_string(&d.visibility),
				bytes_to_opt_string(&d.signature),
				bytes_to_opt_string(&d.binding),
				bytes_to_opt_string(&d.origin),
				start,
				end,
			)
		})
		.collect();
	TableIterator::new(rows)
}

#[pg_extern(immutable, parallel_safe)]
#[allow(clippy::type_complexity)]
fn graph_refs(
	graph: code_graph,
) -> TableIterator<
	'static,
	(
		name!(source, moniker),
		name!(target, moniker),
		name!(kind, String),
		name!(receiver_hint, Option<String>),
		name!(alias, Option<String>),
		name!(confidence, Option<String>),
		name!(binding, Option<String>),
		name!(start_byte, Option<i32>),
		name!(end_byte, Option<i32>),
	),
> {
	let core = graph.to_core();
	let defs: Vec<_> = core.defs().collect();
	let rows: Vec<(
		moniker,
		moniker,
		String,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<String>,
		Option<i32>,
		Option<i32>,
	)> = core
		.refs()
		.map(|r| {
			let source_def = defs
				.get(r.source)
				.unwrap_or_else(|| error!("ref source index {} out of bounds", r.source));
			let (start, end) = position_to_i32(r.position);
			(
				moniker::from_core(source_def.moniker.clone()),
				moniker::from_core(r.target.clone()),
				kind_text(&r.kind),
				bytes_to_opt_string(&r.receiver_hint),
				bytes_to_opt_string(&r.alias),
				bytes_to_opt_string(&r.confidence),
				bytes_to_opt_string(&r.binding),
				start,
				end,
			)
		})
		.collect();
	TableIterator::new(rows)
}

fn bytes_to_opt_string(b: &[u8]) -> Option<String> {
	(!b.is_empty()).then(|| kind_text(b))
}

fn position_to_i32(p: Option<Position>) -> (Option<i32>, Option<i32>) {
	let clamp = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
	match p {
		None => (None, None),
		Some((s, e)) => (Some(clamp(s)), Some(clamp(e))),
	}
}
