//! PostgreSQL type wrapping [`crate::core::moniker::Moniker`].
//!
//! Text I/O uses the canonical typed URI (`<scheme>+moniker://<project>/<kind>:<name>...`).
//! Binary representation is the v2 canonical encoding wrapped in a
//! standard PG varlena (4-byte header + payload), plugged in via manual
//! `IntoDatum`/`FromDatum` impls. No CBOR framing.

use core::ffi::CStr;
use core::ptr::addr_of_mut;

use pgrx::callconv::{Arg, ArgAbi, BoxRet, FcInfo};
use pgrx::datum::{Datum as PgrxDatum, FromDatum, IntoDatum, UnboxDatum};
use pgrx::memcxt::PgMemoryContexts;
use pgrx::prelude::*;
use pgrx::{set_varsize_4b, varlena_to_byte_slice, InOutFuncs, StringInfo};

use crate::core::moniker::{Moniker as CoreMoniker, MonikerView};
use crate::core::uri::{from_uri, to_uri};
use crate::pg::registry::DEFAULT_CONFIG;

mod gist;
mod index;
mod query;

#[allow(non_camel_case_types)]
#[derive(PostgresType, Clone, Eq, PartialEq, Hash, Debug)]
#[inoutfuncs]
#[bikeshed_postgres_type_manually_impl_from_into_datum]
pub struct moniker {
	bytes: Vec<u8>,
}

impl moniker {
	pub(super) fn from_core(m: CoreMoniker) -> Self {
		Self {
			bytes: m.into_bytes(),
		}
	}

	pub(super) fn to_core(&self) -> CoreMoniker {
		// Bytes were validated when the Datum was first constructed
		// (either by moniker_in or by IntoDatum from a builder result).
		CoreMoniker::from_canonical_bytes(self.bytes.clone())
	}

	pub(super) fn view(&self) -> MonikerView<'_> {
		// SAFETY: see `to_core`.
		unsafe { MonikerView::from_canonical_bytes(&self.bytes) }
	}
}

impl InOutFuncs for moniker {
	fn input(input: &CStr) -> Self {
		let s = input
			.to_str()
			.unwrap_or_else(|_| error!("moniker text must be valid UTF-8"));
		let m = from_uri(s, &DEFAULT_CONFIG)
			.unwrap_or_else(|e| error!("moniker parse error: {e}"));
		moniker::from_core(m)
	}

	fn output(&self, buffer: &mut StringInfo) {
		let m = self.to_core();
		let s = to_uri(&m, &DEFAULT_CONFIG)
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
	a.bytes == b.bytes
}

#[pg_extern(immutable, parallel_safe)]
fn project_of(m: moniker) -> String {
	String::from_utf8(m.view().project().to_vec()).expect("project must be UTF-8")
}

#[pg_extern(immutable, parallel_safe)]
fn depth(m: moniker) -> i32 {
	m.view().segment_count() as i32
}

pub(super) unsafe fn palloc_varlena_from_slice(bytes: &[u8]) -> pg_sys::Datum {
	let len = bytes.len().saturating_add(pg_sys::VARHDRSZ);
	assert!(len < (u32::MAX as usize >> 2), "moniker exceeds 1 GiB varlena cap");
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

pub(super) unsafe fn varlena_to_owned_bytes(datum: pg_sys::Datum) -> Vec<u8> {
	unsafe { varlena_to_borrowed_bytes(datum).to_vec() }
}

/// The borrow lives as long as the underlying varlena Datum stays in
/// scope; callers must keep the source GISTENTRY (or other holder) alive
/// for the borrow's duration.
pub(super) unsafe fn varlena_to_borrowed_bytes<'a>(datum: pg_sys::Datum) -> &'a [u8] {
	unsafe {
		let detoasted = pg_sys::pg_detoast_datum_packed(datum.cast_mut_ptr());
		varlena_to_byte_slice(detoasted)
	}
}

impl IntoDatum for moniker {
	fn into_datum(self) -> Option<pg_sys::Datum> {
		Some(unsafe { palloc_varlena_from_slice(&self.bytes) })
	}

	fn type_oid() -> pg_sys::Oid {
		::pgrx::wrappers::rust_regtypein::<Self>()
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
		Some(moniker { bytes: unsafe { varlena_to_owned_bytes(datum) } })
	}

	unsafe fn from_datum_in_memory_context(
		mut memory_context: PgMemoryContexts,
		datum: pg_sys::Datum,
		is_null: bool,
		typoid: pg_sys::Oid,
	) -> Option<Self> {
		if is_null || datum.is_null() {
			return None;
		}
		unsafe {
			memory_context.switch_to(|_| {
				let copied = pg_sys::pg_detoast_datum_copy(datum.cast_mut_ptr());
				<Self as FromDatum>::from_polymorphic_datum(
					pg_sys::Datum::from(copied),
					false,
					typoid,
				)
			})
		}
	}
}

unsafe impl UnboxDatum for moniker {
	type As<'dat> = Self where Self: 'dat;
	unsafe fn unbox<'dat>(datum: PgrxDatum<'dat>) -> Self::As<'dat>
	where
		Self: 'dat,
	{
		unsafe {
			<Self as FromDatum>::from_datum(::core::mem::transmute(datum), false).unwrap()
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
