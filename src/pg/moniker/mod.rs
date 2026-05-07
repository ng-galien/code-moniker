//! PostgreSQL type wrapping [`crate::core::moniker::Moniker`].
//!
//! Text I/O uses the SCIP URI format. Binary representation is opaque
//! (pgrx default cbor wrapper around the inner byte buffer).

use core::ffi::CStr;

use pgrx::prelude::*;
use pgrx::{InOutFuncs, StringInfo};
use serde::{Deserialize, Serialize};

use crate::core::moniker::{Moniker as CoreMoniker, MonikerView};
use crate::core::uri::{from_uri, to_uri};
use crate::pg::registry::{with_registry, DEFAULT_CONFIG};

mod query;

#[allow(non_camel_case_types)]
#[derive(PostgresType, Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
#[inoutfuncs]
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
		CoreMoniker::from_bytes(self.bytes.clone()).expect("invalid moniker bytes")
	}

	pub(super) fn view(&self) -> MonikerView<'_> {
		MonikerView::from_bytes(&self.bytes).expect("invalid moniker bytes")
	}
}

impl InOutFuncs for moniker {
	fn input(input: &CStr) -> Self {
		let s = input
			.to_str()
			.unwrap_or_else(|_| error!("moniker text must be valid UTF-8"));
		let m = with_registry(|reg| from_uri(s, reg, &DEFAULT_CONFIG))
			.unwrap_or_else(|e| error!("moniker parse error: {e}"));
		moniker::from_core(m)
	}

	fn output(&self, buffer: &mut StringInfo) {
		let m = self.to_core();
		let s = with_registry(|reg| to_uri(&m, reg, &DEFAULT_CONFIG))
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
