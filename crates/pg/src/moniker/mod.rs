use core::ffi::CStr;
use core::ptr::addr_of_mut;
use std::sync::OnceLock;

use pgrx::callconv::{Arg, ArgAbi, BoxRet, FcInfo};
use pgrx::datum::{Datum as PgrxDatum, FromDatum, IntoDatum, UnboxDatum};
use pgrx::memcxt::PgMemoryContexts;
use pgrx::prelude::*;
use pgrx::{InOutFuncs, StringInfo, set_varsize_4b, varlena_to_byte_slice};

use crate::registry::with_current_config;
use crate::util::resolve_type_oid;
use code_moniker_core::core::moniker::{Moniker as CoreMoniker, MonikerView};
use code_moniker_core::core::uri::{from_uri, to_uri};

mod compact;
mod gist;
mod index;
mod query;

// code-moniker: ignore[name-pascalcase] — pgrx maps the Rust struct name 1:1 to the SQL type name, which must be `moniker`.
#[allow(non_camel_case_types)]
#[derive(PostgresType, Debug)]
#[inoutfuncs]
#[bikeshed_postgres_type_manually_impl_from_into_datum]
pub struct moniker {
	storage: MonikerStorage,
}

#[derive(Debug)]
enum MonikerStorage {
	Owned(Vec<u8>),
	Borrowed { ptr: *const u8, len: u32 },
}

impl moniker {
	pub(super) fn from_owned_bytes(bytes: Vec<u8>) -> Self {
		Self {
			storage: MonikerStorage::Owned(bytes),
		}
	}

	pub(super) fn from_core(m: CoreMoniker) -> Self {
		Self::from_owned_bytes(m.into_bytes())
	}

	pub(super) fn into_core(self) -> CoreMoniker {
		match self.storage {
			MonikerStorage::Owned(v) => CoreMoniker::from_canonical_bytes(v),
			MonikerStorage::Borrowed { ptr, len } => {
				let bytes = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
				CoreMoniker::from_canonical_bytes(bytes.to_vec())
			}
		}
	}

	pub(super) fn to_core(&self) -> CoreMoniker {
		CoreMoniker::from_canonical_bytes(self.as_bytes().to_vec())
	}

	pub(super) fn view(&self) -> MonikerView<'_> {
		// SAFETY: both storage variants produce canonical bytes upholding from_canonical_bytes' precondition.
		unsafe { MonikerView::from_canonical_bytes(self.as_bytes()) }
	}

	pub(super) fn as_bytes(&self) -> &[u8] {
		match self.storage {
			MonikerStorage::Owned(ref v) => v.as_slice(),
			// SAFETY: borrowed slice points inside a detoasted varlena that outlives `&self`.
			MonikerStorage::Borrowed { ptr, len } => unsafe {
				core::slice::from_raw_parts(ptr, len as usize)
			},
		}
	}
}

impl Clone for moniker {
	fn clone(&self) -> Self {
		Self::from_owned_bytes(self.as_bytes().to_vec())
	}
}

impl PartialEq for moniker {
	fn eq(&self, other: &Self) -> bool {
		self.as_bytes() == other.as_bytes()
	}
}

impl Eq for moniker {}

impl std::hash::Hash for moniker {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.as_bytes().hash(state);
	}
}

impl InOutFuncs for moniker {
	fn input(input: &CStr) -> Self {
		let s = input
			.to_str()
			.unwrap_or_else(|_| error!("moniker text must be valid UTF-8"));
		let m = with_current_config(|cfg| from_uri(s, cfg))
			.unwrap_or_else(|e| error!("moniker parse error: {e}"));
		moniker::from_core(m)
	}

	fn output(&self, buffer: &mut StringInfo) {
		let m = self.to_core();
		let s = with_current_config(|cfg| to_uri(&m, cfg))
			.unwrap_or_else(|e| error!("moniker serialize error: {e}"));
		buffer.push_str(&s);
	}
}

#[pg_operator(immutable, parallel_safe)]
#[opname(=)]
#[commutator(=)]
#[hashes]
#[merges]
fn moniker_eq(a: moniker, b: moniker) -> bool {
	a.as_bytes() == b.as_bytes()
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_to_bytea(m: moniker) -> Vec<u8> {
	m.as_bytes().to_vec()
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_from_bytea(bytes: &[u8]) -> moniker {
	if MonikerView::from_bytes(bytes).is_err() {
		error!("moniker_from_bytea: invalid moniker bytes");
	}
	moniker::from_owned_bytes(bytes.to_vec())
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_to_cbor(m: moniker) -> Vec<u8> {
	serde_cbor::to_vec(&m.to_core()).unwrap_or_else(|e| error!("moniker_to_cbor: {e}"))
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_from_cbor(bytes: &[u8]) -> moniker {
	let core: CoreMoniker =
		serde_cbor::from_slice(bytes).unwrap_or_else(|e| error!("moniker_from_cbor: {e}"));
	moniker::from_core(core)
}

#[pg_extern(immutable, parallel_safe)]
fn project_of(m: moniker) -> String {
	String::from_utf8(m.view().project().to_vec()).expect("project must be UTF-8")
}

#[pg_extern(immutable, parallel_safe)]
fn lang_of(m: moniker) -> String {
	let view = m.view();
	for seg in view.segments() {
		if seg.kind == code_moniker_core::lang::kinds::LANG {
			return String::from_utf8(seg.name.to_vec()).expect("lang must be UTF-8");
		}
	}
	String::new()
}

#[pg_extern(immutable, parallel_safe)]
fn depth(m: moniker) -> i32 {
	m.view().segment_count() as i32
}

pub(crate) unsafe fn palloc_varlena_from_slice(bytes: &[u8]) -> pg_sys::Datum {
	let len = bytes.len().saturating_add(pg_sys::VARHDRSZ);
	assert!(
		len < (u32::MAX as usize >> 2),
		"moniker exceeds 1 GiB varlena cap"
	);
	unsafe {
		let varlena = pg_sys::palloc(len) as *mut pg_sys::varlena;
		let varattrib_4b: *mut _ = &mut varlena
			.cast::<pg_sys::varattrib_4b>()
			.as_mut()
			.unwrap_unchecked()
			.va_4byte;
		set_varsize_4b(varlena, len as i32);
		std::ptr::copy_nonoverlapping(
			bytes.as_ptr(),
			addr_of_mut!((&mut *varattrib_4b).va_data).cast::<u8>(),
			bytes.len(),
		);
		pg_sys::Datum::from(varlena)
	}
}

pub(crate) unsafe fn varlena_to_borrowed_bytes<'a>(datum: pg_sys::Datum) -> &'a [u8] {
	unsafe {
		let detoasted = pg_sys::pg_detoast_datum_packed(datum.cast_mut_ptr());
		varlena_to_byte_slice(detoasted)
	}
}

static MONIKER_TYPE_OID: OnceLock<pg_sys::Oid> = OnceLock::new();

fn moniker_type_oid() -> pg_sys::Oid {
	*MONIKER_TYPE_OID.get_or_init(|| resolve_type_oid("moniker"))
}

impl IntoDatum for moniker {
	fn into_datum(self) -> Option<pg_sys::Datum> {
		Some(unsafe { palloc_varlena_from_slice(self.as_bytes()) })
	}

	fn type_oid() -> pg_sys::Oid {
		moniker_type_oid()
	}
}

unsafe impl BoxRet for moniker {
	unsafe fn box_into<'fcx>(self, fcinfo: &mut FcInfo<'fcx>) -> PgrxDatum<'fcx> {
		match IntoDatum::into_datum(self) {
			None => fcinfo.return_null(),
			Some(datum) => unsafe { fcinfo.return_raw_datum(datum) },
		}
	}
}

impl FromDatum for moniker {
	unsafe fn from_polymorphic_datum(
		datum: pg_sys::Datum,
		is_null: bool,
		_typoid: pg_sys::Oid,
	) -> Option<Self> {
		if is_null || datum.is_null() {
			return None;
		}
		let bytes = unsafe { varlena_to_borrowed_bytes(datum) };
		Some(moniker {
			storage: MonikerStorage::Borrowed {
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
				Some(moniker::from_owned_bytes(bytes.to_vec()))
			})
		}
	}
}

unsafe impl UnboxDatum for moniker {
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

unsafe impl<'fcx> ArgAbi<'fcx> for moniker
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
