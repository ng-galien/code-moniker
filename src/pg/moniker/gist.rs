use core::ffi::c_int;

use pgrx::datum::Internal;
use pgrx::pg_sys;
use pgrx::prelude::*;

use super::moniker;
use super::{palloc_varlena_from_slice, varlena_to_borrowed_bytes};
use crate::core::moniker::encoding::{read_u16, write_u16, HEADER_FIXED_LEN, VERSION};
use crate::core::moniker::query::bare_callable_name;

extension_sql!(
	r#"
	CREATE OPERATOR CLASS moniker_gist_ops
		DEFAULT FOR TYPE moniker USING gist AS
			OPERATOR 3  =,
			OPERATOR 8  @>,
			OPERATOR 10 <@,
			OPERATOR 11 ?=,
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
		bind_match,
		moniker_gist_consistent,
		moniker_gist_union,
		moniker_gist_compress,
		moniker_gist_decompress,
		moniker_gist_penalty,
		moniker_gist_picksplit,
		moniker_gist_equal,
	]
);

const STRAT_EQUAL: u32 = 3;
const STRAT_CONTAINS: u32 = 8;
const STRAT_CONTAINED_BY: u32 = 10;
const STRAT_BIND_MATCH: u32 = 11;

fn parse_sig(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
	if bytes.len() < HEADER_FIXED_LEN || bytes[0] != VERSION {
		return None;
	}
	let project_len = read_u16(bytes, 1) as usize;
	let segs_off = HEADER_FIXED_LEN + project_len;
	if bytes.len() < segs_off {
		return None;
	}
	Some((&bytes[HEADER_FIXED_LEN..segs_off], &bytes[segs_off..]))
}

fn lcp_len(a: &[u8], b: &[u8]) -> usize {
	a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn sig_bytes(project: &[u8], segs: &[u8]) -> Vec<u8> {
	let mut buf = Vec::with_capacity(HEADER_FIXED_LEN + project.len() + segs.len());
	buf.push(VERSION);
	write_u16(&mut buf, project.len() as u16);
	buf.extend_from_slice(project);
	buf.extend_from_slice(segs);
	buf
}

unsafe fn build_sig(project: &[u8], segs: &[u8]) -> pg_sys::Datum {
	unsafe { palloc_varlena_from_slice(&sig_bytes(project, segs)) }
}

unsafe fn build_wildcard() -> pg_sys::Datum {
	unsafe { palloc_varlena_from_slice(&[0u8]) }
}

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
		let key_bytes = varlena_to_borrowed_bytes(entry_ref.key);
		let recheck_ptr = recheck
			.unwrap()
			.expect("gist recheck out-arg is null")
			.cast_mut_ptr::<bool>();

		let q = parse_sig(query.as_bytes()).expect("query moniker has canonical header");
		let q_project = q.0;
		let q_segs = q.1;

		let key = parse_sig(key_bytes);
		let (k_project, k_segs) = match key {
			None => {
				*recheck_ptr = true;
				return true;
			}
			Some(parts) => parts,
		};

		if k_project != q_project {
			*recheck_ptr = !entry_ref.leafkey;
			return false;
		}

		if entry_ref.leafkey {
			*recheck_ptr = strategy as u32 == STRAT_BIND_MATCH;
			match strategy as u32 {
				STRAT_EQUAL => k_segs == q_segs,
				STRAT_CONTAINS => q_segs.starts_with(k_segs),
				STRAT_CONTAINED_BY => k_segs.starts_with(q_segs),
				STRAT_BIND_MATCH => bind_match_segs(k_segs, q_segs),
				_ => false,
			}
		} else {
			*recheck_ptr = true;
			match strategy as u32 {
				STRAT_EQUAL | STRAT_CONTAINS => q_segs.starts_with(k_segs),
				STRAT_CONTAINED_BY => k_segs.starts_with(q_segs) || q_segs.starts_with(k_segs),
				STRAT_BIND_MATCH => {
					match parent_prefix_bytes(q_segs) {
						Some(qp) => k_segs.starts_with(qp) || qp.starts_with(k_segs),
						None => false,
					}
				}
				_ => false,
			}
		}
	}
}

fn parent_prefix_bytes(segs: &[u8]) -> Option<&[u8]> {
	let mut cursor = 0usize;
	let mut last_start: Option<usize> = None;
	while cursor < segs.len() {
		last_start = Some(cursor);
		if segs.len() < cursor + 2 {
			return None;
		}
		let kind_len = u16::from_le_bytes([segs[cursor], segs[cursor + 1]]) as usize;
		cursor += 2 + kind_len;
		if segs.len() < cursor + 2 {
			return None;
		}
		let name_len = u16::from_le_bytes([segs[cursor], segs[cursor + 1]]) as usize;
		cursor += 2 + name_len;
		if cursor > segs.len() {
			return None;
		}
	}
	last_start.map(|s| &segs[..s])
}

fn bind_match_segs(left: &[u8], right: &[u8]) -> bool {
	let lp = match parent_prefix_bytes(left) {
		Some(p) => p,
		None => return false,
	};
	let rp = match parent_prefix_bytes(right) {
		Some(p) => p,
		None => return false,
	};
	if lp != rp {
		return false;
	}
	let l_last = &left[lp.len()..];
	let r_last = &right[rp.len()..];
	let l_name = last_segment_name(l_last);
	let r_name = last_segment_name(r_last);
	if l_name == r_name {
		return true;
	}
	match (l_name, r_name) {
		(Some(l), Some(r)) => bare_callable_name(l) == bare_callable_name(r),
		_ => false,
	}
}

fn last_segment_name(seg_bytes: &[u8]) -> Option<&[u8]> {
	if seg_bytes.len() < 4 {
		return None;
	}
	let kind_len = u16::from_le_bytes([seg_bytes[0], seg_bytes[1]]) as usize;
	let name_off = 2 + kind_len + 2;
	if seg_bytes.len() < name_off {
		return None;
	}
	let name_len = u16::from_le_bytes([
		seg_bytes[2 + kind_len],
		seg_bytes[2 + kind_len + 1],
	]) as usize;
	if seg_bytes.len() < name_off + name_len {
		return None;
	}
	Some(&seg_bytes[name_off..name_off + name_len])
}

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

		let first = varlena_to_borrowed_bytes((*arr).key);
		let bytes = match union_fold(first, (1..n).map(|i| varlena_to_borrowed_bytes((*arr.add(i)).key))) {
			SigAcc::Wildcard => vec![0u8],
			SigAcc::Constrained { project, segs } => sig_bytes(project, segs),
		};

		if let Some(sz_datum) = sizep.unwrap() {
			let sz_ptr = sz_datum.cast_mut_ptr::<c_int>();
			*sz_ptr = (bytes.len() + pg_sys::VARHDRSZ) as c_int;
		}

		moniker::from_owned_bytes(bytes)
	}
}

enum SigAcc<'a> {
	Wildcard,
	Constrained { project: &'a [u8], segs: &'a [u8] },
}

fn union_fold<'a>(first: &'a [u8], rest: impl Iterator<Item = &'a [u8]>) -> SigAcc<'a> {
	let mut acc = match parse_sig(first) {
		None => return rest.fold(SigAcc::Wildcard, |w, _| w),
		Some((p, s)) => SigAcc::Constrained { project: p, segs: s },
	};
	for cur in rest {
		acc = match (acc, parse_sig(cur)) {
			(SigAcc::Wildcard, _) | (_, None) => SigAcc::Wildcard,
			(SigAcc::Constrained { project, segs }, Some((cp, cs))) => {
				if project != cp {
					SigAcc::Wildcard
				} else {
					let l = lcp_len(segs, cs);
					SigAcc::Constrained { project, segs: &segs[..l] }
				}
			}
		};
	}
	acc
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_compress(entry: Internal) -> Internal {
	entry
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_decompress(entry: Internal) -> Internal {
	entry
}

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
		let maxoff = (n - 1) as pg_sys::OffsetNumber;
		let first = pg_sys::FirstOffsetNumber;

		let mut indexed: Vec<(pg_sys::OffsetNumber, &[u8], &[u8])> = (first..=maxoff)
			.map(|i| {
				let bytes = varlena_to_borrowed_bytes((*arr.add(i as usize)).key);
				let segs = parse_sig(bytes).map(|(_, s)| s).unwrap_or(&[]);
				(i, bytes, segs)
			})
			.collect();
		indexed.sort_by(|a, b| a.2.cmp(b.2));

		let total = indexed.len();
		assert!(total >= 2, "picksplit requires >= 2 entries, got {}", total);
		let mid = total.div_ceil(2);

		let split_datum = splitvec.unwrap().expect("gist picksplit splitvec is null");
		let split = &mut *split_datum.cast_mut_ptr::<pg_sys::GIST_SPLITVEC>();

		let nbytes = (n + 1) * core::mem::size_of::<pg_sys::OffsetNumber>();
		split.spl_left = pg_sys::palloc(nbytes) as *mut pg_sys::OffsetNumber;
		split.spl_right = pg_sys::palloc(nbytes) as *mut pg_sys::OffsetNumber;
		split.spl_nleft = 0;
		split.spl_nright = 0;

		let mut left_bytes: Vec<&[u8]> = Vec::with_capacity(mid);
		let mut right_bytes: Vec<&[u8]> = Vec::with_capacity(total - mid);

		for (slot, (off, bytes, _segs)) in indexed.into_iter().enumerate() {
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

		split.spl_ldatum = side_union(&left_bytes);
		split.spl_rdatum = side_union(&right_bytes);

		Internal::from(Some(split_datum))
	}
}

unsafe fn side_union(entries: &[&[u8]]) -> pg_sys::Datum {
	debug_assert!(!entries.is_empty(), "picksplit half cannot be empty");
	let acc = union_fold(entries[0], entries[1..].iter().copied());
	match acc {
		SigAcc::Wildcard => unsafe { build_wildcard() },
		SigAcc::Constrained { project, segs } => unsafe { build_sig(project, segs) },
	}
}

#[pg_extern(immutable, parallel_safe)]
fn moniker_gist_equal(a: moniker, b: moniker, result: Internal) -> Internal {
	unsafe {
		let result_datum = result.unwrap().expect("gist equal result is null");
		let result_ptr = result_datum.cast_mut_ptr::<bool>();
		*result_ptr = a.as_bytes() == b.as_bytes();
		Internal::from(Some(result_datum))
	}
}
