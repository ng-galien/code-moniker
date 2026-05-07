//! GiST opclass on `moniker`. Inner-page signatures share the canonical
//! header with leaves: `[version=1][project_len LE][project][seg_count
//! LE][segments_lcp]`. The segments region of an inner key is the
//! longest common byte prefix of all leaf segment regions below; it
//! may end mid-segment, which is fine because we never parse it.
//! Cross-project unions degrade to a single-byte sentinel ("wildcard")
//! that matches everything with recheck. Compress/decompress are
//! identity. Picksplit sorts entries by their segment-region bytes and
//! halves them — naive but predictable; penalty is `len(orig_segs) -
//! lcp(orig_segs, new_segs)` as float4, plus a big constant on
//! cross-project insertion.

use core::ffi::c_int;

use pgrx::datum::Internal;
use pgrx::pg_sys;
use pgrx::prelude::*;

use super::moniker;
use super::{palloc_varlena_from_slice, varlena_to_borrowed_bytes, varlena_to_owned_bytes};
use crate::core::moniker::encoding::{read_u16, HEADER_FIXED_LEN, VERSION};

extension_sql!(
	r#"
	CREATE OPERATOR CLASS moniker_gist_ops
		DEFAULT FOR TYPE moniker USING gist AS
			OPERATOR 3  =,
			OPERATOR 8  @>,
			OPERATOR 10 <@,
			FUNCTION 1  moniker_gist_consistent(internal, moniker, smallint, oid, internal),
			FUNCTION 2  moniker_gist_union(internal, internal),
			FUNCTION 3  moniker_gist_compress(internal),
			FUNCTION 4  moniker_gist_decompress(internal),
			FUNCTION 5  moniker_gist_penalty(internal, internal, internal),
			FUNCTION 6  moniker_gist_picksplit(internal, internal),
			FUNCTION 7  moniker_gist_equal(moniker, moniker, internal);
	"#,
	name = "moniker_gist_opclass",
	requires = [
		moniker_eq,
		moniker_ancestor_of,
		moniker_descendant_of,
		moniker_gist_consistent,
		moniker_gist_union,
		moniker_gist_compress,
		moniker_gist_decompress,
		moniker_gist_penalty,
		moniker_gist_picksplit,
		moniker_gist_equal,
	]
);

// Strategy numbers from the OPERATOR lines above. Local consts: PG's
// global RT* constants happen to use different numbers (RTContains = 7,
// RTContainedBy = 8) so reusing them would mislead.
const STRAT_EQUAL: u32 = 3;
const STRAT_CONTAINS: u32 = 8;
const STRAT_CONTAINED_BY: u32 = 10;

/// Decoded view of a signature varlena: `Some((project, segs))` for a
/// canonical-header signature, `None` for the wildcard sentinel
/// produced by cross-project unions.
fn parse_sig(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
	if bytes.len() < HEADER_FIXED_LEN || bytes[0] != VERSION {
		return None;
	}
	let project_len = read_u16(bytes, 1) as usize;
	let segs_off = 3 + project_len + 2;
	if bytes.len() < segs_off {
		return None;
	}
	Some((&bytes[3..3 + project_len], &bytes[segs_off..]))
}

fn lcp_len(a: &[u8], b: &[u8]) -> usize {
	a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Build the byte payload of a constrained signature
/// (`[VERSION][project_len LE][project][0xFFFF][segs]`). The 0xFFFF
/// seg_count is a debug hint; it's never parsed because inner sigs
/// don't go through the regular moniker decoder.
fn sig_bytes(project: &[u8], segs: &[u8]) -> Vec<u8> {
	let mut buf = Vec::with_capacity(HEADER_FIXED_LEN + project.len() + segs.len());
	buf.push(VERSION);
	buf.extend_from_slice(&(project.len() as u16).to_le_bytes());
	buf.extend_from_slice(project);
	buf.extend_from_slice(&[0xFF, 0xFF]);
	buf.extend_from_slice(segs);
	buf
}

unsafe fn build_sig(project: &[u8], segs: &[u8]) -> pg_sys::Datum {
	unsafe { palloc_varlena_from_slice(&sig_bytes(project, segs)) }
}

unsafe fn build_wildcard() -> pg_sys::Datum {
	unsafe { palloc_varlena_from_slice(&[0u8]) }
}

// ---------------------------------------------------------------------
// consistent(internal entry, moniker query, smallint strategy,
//            oid subtype, internal recheck) -> bool
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_consistent(
	entry: Internal,
	query: moniker,
	strategy: i16,
	_subtype: pg_sys::Oid,
	recheck: Internal,
) -> bool {
	unsafe {
		let entry_ptr = entry.unwrap().expect("gist entry is null").cast_mut_ptr::<pg_sys::GISTENTRY>();
		let entry_ref = &*entry_ptr;
		let key_bytes = varlena_to_owned_bytes(entry_ref.key);
		let recheck_ptr = recheck
			.unwrap()
			.expect("gist recheck out-arg is null")
			.cast_mut_ptr::<bool>();

		let q = parse_sig(&query.bytes).expect("query moniker has canonical header");
		let q_project = q.0;
		let q_segs = q.1;

		let key = parse_sig(&key_bytes);
		// Wildcard inner key: must recheck and accept everything.
		let (k_project, k_segs) = match key {
			None => {
				*recheck_ptr = true;
				return true;
			}
			Some(parts) => parts,
		};

		// Project gate: different project ⇒ no descendant relation possible,
		// and `=` is also impossible.
		if k_project != q_project {
			*recheck_ptr = !entry_ref.leafkey;
			return false;
		}

		if entry_ref.leafkey {
			*recheck_ptr = false;
			match strategy as u32 {
				STRAT_EQUAL => k_segs == q_segs,
				STRAT_CONTAINS => q_segs.starts_with(k_segs),
				STRAT_CONTAINED_BY => k_segs.starts_with(q_segs),
				_ => false,
			}
		} else {
			*recheck_ptr = true;
			match strategy as u32 {
				STRAT_EQUAL | STRAT_CONTAINS => q_segs.starts_with(k_segs),
				STRAT_CONTAINED_BY => k_segs.starts_with(q_segs) || q_segs.starts_with(k_segs),
				_ => false,
			}
		}
	}
}

// ---------------------------------------------------------------------
// union(internal entryvec, internal sizep) -> moniker
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_union(entryvec: Internal, sizep: Internal) -> moniker {
	unsafe {
		let vec_ptr = entryvec
			.unwrap()
			.expect("gist union entryvec is null")
			.cast_mut_ptr::<pg_sys::GistEntryVector>();
		let vec_ref = &*vec_ptr;
		let n = vec_ref.n as usize;
		let arr = vec_ref.vector.as_ptr();
		assert!(n > 0, "gist union called with empty entryvec");

		let first = varlena_to_owned_bytes((*arr).key);
		let bytes = match union_fold(first, (1..n).map(|i| varlena_to_owned_bytes((*arr.add(i)).key))) {
			SigAcc::Wildcard => vec![0u8],
			SigAcc::Constrained { project, segs } => sig_bytes(&project, &segs),
		};

		if let Some(sz_datum) = sizep.unwrap() {
			let sz_ptr = sz_datum.cast_mut_ptr::<c_int>();
			*sz_ptr = (bytes.len() + pg_sys::VARHDRSZ) as c_int;
		}

		moniker { bytes }
	}
}

enum SigAcc {
	Wildcard,
	Constrained { project: Vec<u8>, segs: Vec<u8> },
}

fn union_fold(first: Vec<u8>, rest: impl Iterator<Item = Vec<u8>>) -> SigAcc {
	let mut acc = match parse_sig(&first) {
		None => return rest.fold(SigAcc::Wildcard, |w, _| w),
		Some((p, s)) => SigAcc::Constrained {
			project: p.to_vec(),
			segs: s.to_vec(),
		},
	};
	for cur in rest {
		acc = match (acc, parse_sig(&cur)) {
			(SigAcc::Wildcard, _) | (_, None) => SigAcc::Wildcard,
			(SigAcc::Constrained { project, segs }, Some((cp, cs))) => {
				if project != cp {
					SigAcc::Wildcard
				} else {
					let l = lcp_len(&segs, cs);
					let mut new_segs = segs;
					new_segs.truncate(l);
					SigAcc::Constrained { project, segs: new_segs }
				}
			}
		};
	}
	acc
}

// ---------------------------------------------------------------------
// compress / decompress: identity. Return the same entry pointer.
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_compress(entry: Internal) -> Internal {
	entry
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_decompress(entry: Internal) -> Internal {
	entry
}

// ---------------------------------------------------------------------
// penalty(internal orig, internal new, internal *float result) -> internal
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_penalty(orig: Internal, new: Internal, result: Internal) -> Internal {
	unsafe {
		let orig_entry = &*orig
			.unwrap()
			.expect("gist penalty orig is null")
			.cast_mut_ptr::<pg_sys::GISTENTRY>();
		let new_entry = &*new
			.unwrap()
			.expect("gist penalty new is null")
			.cast_mut_ptr::<pg_sys::GISTENTRY>();
		// Borrow the varlena payloads in place: penalty is per-candidate-page
		// per insert, so the per-call clone in `varlena_to_owned_bytes` would
		// add up. The borrow is valid for the rest of this frame because both
		// GISTENTRYs (and their .key Datums) live until the function returns.
		let orig_bytes = varlena_to_borrowed_bytes(orig_entry.key);
		let new_bytes = varlena_to_borrowed_bytes(new_entry.key);

		let penalty: f32 = match (parse_sig(orig_bytes), parse_sig(new_bytes)) {
			(None, _) => 0.0,
			(Some(_), None) => 1e9,
			(Some((op, os)), Some((np, ns))) => {
				if op != np {
					1e6
				} else {
					(os.len() - lcp_len(os, ns)) as f32
				}
			}
		};

		let result_datum = result.unwrap().expect("gist penalty result is null");
		let result_ptr = result_datum.cast_mut_ptr::<f32>();
		*result_ptr = penalty;
		Internal::from(Some(result_datum))
	}
}

// ---------------------------------------------------------------------
// picksplit(internal entryvec, internal *GIST_SPLITVEC) -> internal
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_picksplit(entryvec: Internal, splitvec: Internal) -> Internal {
	unsafe {
		let vec_ptr = entryvec
			.unwrap()
			.expect("gist picksplit entryvec is null")
			.cast_mut_ptr::<pg_sys::GistEntryVector>();
		let vec_ref = &*vec_ptr;
		let n = vec_ref.n as usize;
		let arr = vec_ref.vector.as_ptr();
		// Per gistsplit.c: vector[0] is unused; real entries are at
		// offsets [1, n-1] (FirstOffsetNumber..=maxoff).
		let maxoff = (n - 1) as pg_sys::OffsetNumber;
		let first = pg_sys::FirstOffsetNumber;

		let mut indexed: Vec<(pg_sys::OffsetNumber, Vec<u8>)> = (first..=maxoff)
			.map(|i| (i, varlena_to_owned_bytes((*arr.add(i as usize)).key)))
			.collect();
		// Sort by the segments region so logically-related monikers cluster
		// together. Wildcards (which have no canonical header) sink to the
		// front.
		indexed.sort_by(|a, b| {
			let sa = parse_sig(&a.1).map(|(_, s)| s).unwrap_or(&[]);
			let sb = parse_sig(&b.1).map(|(_, s)| s).unwrap_or(&[]);
			sa.cmp(sb)
		});

		let total = indexed.len();
		assert!(total >= 2, "picksplit requires >= 2 entries, got {}", total);
		let mid = (total + 1) / 2;

		let split_datum = splitvec.unwrap().expect("gist picksplit splitvec is null");
		let split = &mut *split_datum.cast_mut_ptr::<pg_sys::GIST_SPLITVEC>();

		let nbytes = (n + 1) * core::mem::size_of::<pg_sys::OffsetNumber>();
		split.spl_left = pg_sys::palloc(nbytes) as *mut pg_sys::OffsetNumber;
		split.spl_right = pg_sys::palloc(nbytes) as *mut pg_sys::OffsetNumber;
		split.spl_nleft = 0;
		split.spl_nright = 0;

		let mut left_bytes: Vec<Vec<u8>> = Vec::new();
		let mut right_bytes: Vec<Vec<u8>> = Vec::new();

		for (slot, (off, bytes)) in indexed.into_iter().enumerate() {
			if slot < mid {
				*split.spl_left.add(split.spl_nleft as usize) = off;
				split.spl_nleft += 1;
				left_bytes.push(bytes);
			} else {
				*split.spl_right.add(split.spl_nright as usize) = off;
				split.spl_nright += 1;
				right_bytes.push(bytes);
			}
		}

		split.spl_ldatum = side_union(left_bytes);
		split.spl_rdatum = side_union(right_bytes);

		Internal::from(Some(split_datum))
	}
}

unsafe fn side_union(mut entries: Vec<Vec<u8>>) -> pg_sys::Datum {
	debug_assert!(!entries.is_empty(), "picksplit half cannot be empty");
	let first = entries.remove(0);
	let acc = union_fold(first, entries.into_iter());
	match acc {
		SigAcc::Wildcard => unsafe { build_wildcard() },
		SigAcc::Constrained { project, segs } => unsafe { build_sig(&project, &segs) },
	}
}

// ---------------------------------------------------------------------
// equal(moniker a, moniker b, internal *bool result) -> internal
// ---------------------------------------------------------------------
#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_equal(a: moniker, b: moniker, result: Internal) -> Internal {
	unsafe {
		let result_datum = result.unwrap().expect("gist equal result is null");
		let result_ptr = result_datum.cast_mut_ptr::<bool>();
		*result_ptr = a.bytes == b.bytes;
		Internal::from(Some(result_datum))
	}
}
