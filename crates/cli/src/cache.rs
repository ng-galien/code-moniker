use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::code_graph::encoding::{self, LAYOUT_VERSION};
use rustc_hash::FxHasher;

const CACHE_MAGIC: u32 = 0xC0DE_2106;
const CACHE_FORMAT_VERSION: u32 = 1;
const HEADER_FIXED: usize = 4 + 4 + 8 + 8 + 8 + 4;

#[derive(Clone, Debug)]
pub struct CacheKey {
	pub abs_path: PathBuf,
	pub mtime: u64,
	pub size: u64,
	pub anchor_hash: u64,
}

impl CacheKey {
	pub fn from_path(path: &Path, anchor: &Path) -> io::Result<Self> {
		let abs_path = path.canonicalize()?;
		let meta = fs::metadata(&abs_path)?;
		let mtime = meta
			.modified()?
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_nanos() as u64)
			.unwrap_or(0);
		Ok(Self {
			abs_path,
			mtime,
			size: meta.len(),
			anchor_hash: hash_path(anchor),
		})
	}

	fn path_hash(&self) -> u64 {
		hash_path(&self.abs_path)
	}

	fn shard(&self) -> String {
		format!("{:02x}", (self.path_hash() & 0xff) as u8)
	}

	fn filename(&self) -> String {
		format!("{:016x}_{:016x}.bin", self.path_hash(), self.anchor_hash)
	}

	fn full_path(&self, root: &Path) -> PathBuf {
		root.join(format!("v{LAYOUT_VERSION}_{CACHE_FORMAT_VERSION}"))
			.join(self.shard())
			.join(self.filename())
	}
}

pub fn load(cache_dir: &Path, key: &CacheKey) -> Option<CodeGraph> {
	let path = key.full_path(cache_dir);
	let bytes = fs::read(&path).ok()?;
	let body = validate_header(&bytes, key)?;
	encoding::decode(body).ok()
}

pub fn store(cache_dir: &Path, key: &CacheKey, graph: &CodeGraph) -> io::Result<()> {
	let path = key.full_path(cache_dir);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let body = encoding::encode(graph)
		.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
	let mut buf = Vec::with_capacity(HEADER_FIXED + key.abs_path_bytes().len() + body.len());
	buf.extend_from_slice(&CACHE_MAGIC.to_le_bytes());
	buf.extend_from_slice(&CACHE_FORMAT_VERSION.to_le_bytes());
	buf.extend_from_slice(&key.mtime.to_le_bytes());
	buf.extend_from_slice(&key.size.to_le_bytes());
	buf.extend_from_slice(&key.anchor_hash.to_le_bytes());
	let path_bytes = key.abs_path_bytes();
	buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
	buf.extend_from_slice(path_bytes);
	buf.extend_from_slice(&body);

	let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
	{
		let mut f = fs::File::create(&tmp)?;
		f.write_all(&buf)?;
		f.sync_data()?;
	}
	fs::rename(&tmp, &path)?;
	Ok(())
}

fn validate_header<'a>(bytes: &'a [u8], key: &CacheKey) -> Option<&'a [u8]> {
	if bytes.len() < HEADER_FIXED {
		return None;
	}
	let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
	if magic != CACHE_MAGIC {
		return None;
	}
	let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
	if version != CACHE_FORMAT_VERSION {
		return None;
	}
	let mtime = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
	let size = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
	let anchor_hash = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
	if mtime != key.mtime || size != key.size || anchor_hash != key.anchor_hash {
		return None;
	}
	let path_len = u32::from_le_bytes(bytes[32..36].try_into().ok()?) as usize;
	if HEADER_FIXED + path_len > bytes.len() {
		return None;
	}
	let stored_path = &bytes[HEADER_FIXED..HEADER_FIXED + path_len];
	if stored_path != key.abs_path_bytes() {
		return None;
	}
	Some(&bytes[HEADER_FIXED + path_len..])
}

impl CacheKey {
	fn abs_path_bytes(&self) -> &[u8] {
		path_bytes(&self.abs_path)
	}
}

#[cfg(unix)]
fn path_bytes(p: &Path) -> &[u8] {
	use std::os::unix::ffi::OsStrExt;
	p.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_bytes(p: &Path) -> &[u8] {
	// On non-unix, lossy UTF-8 is good enough — every PG-supported host is unix
	// in practice, but keep the build green elsewhere.
	p.to_str().map(|s| s.as_bytes()).unwrap_or(&[])
}

fn hash_path(p: &Path) -> u64 {
	let mut h = FxHasher::default();
	path_bytes(p).hash(&mut h);
	h.finish()
}

#[cfg(test)]
mod tests {
	use super::*;
	use code_moniker_core::core::moniker::MonikerBuilder;

	fn graph_with_one_def() -> CodeGraph {
		let root = MonikerBuilder::new()
			.project(b"app")
			.segment(b"path", b"root")
			.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let child = MonikerBuilder::new()
			.project(b"app")
			.segment(b"path", b"root")
			.segment(b"class", b"Foo")
			.build();
		g.add_def(child, b"class", &root, Some((0, 10))).unwrap();
		g
	}

	#[test]
	fn store_then_load_roundtrips() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"export class Foo {}\n").unwrap();
		let anchor = tmp.path().join("anchor");
		let key = CacheKey::from_path(&src, &anchor).unwrap();
		let g = graph_with_one_def();

		store(tmp.path(), &key, &g).unwrap();
		let back = load(tmp.path(), &key).expect("should hit");
		assert_eq!(back.def_count(), g.def_count());
	}

	#[test]
	fn load_misses_when_mtime_changes() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"a").unwrap();
		let anchor = tmp.path().join("anchor");
		let key = CacheKey::from_path(&src, &anchor).unwrap();
		store(tmp.path(), &key, &graph_with_one_def()).unwrap();

		std::thread::sleep(std::time::Duration::from_millis(10));
		std::fs::write(&src, b"ab").unwrap();
		let key2 = CacheKey::from_path(&src, &anchor).unwrap();
		assert!(key2.mtime != key.mtime || key2.size != key.size);
		assert!(load(tmp.path(), &key2).is_none());
	}

	#[test]
	fn load_misses_when_anchor_changes() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"a").unwrap();
		let anchor1 = tmp.path().join("anchor1");
		let anchor2 = tmp.path().join("anchor2");
		let key1 = CacheKey::from_path(&src, &anchor1).unwrap();
		let key2 = CacheKey::from_path(&src, &anchor2).unwrap();
		store(tmp.path(), &key1, &graph_with_one_def()).unwrap();
		assert!(load(tmp.path(), &key1).is_some());
		assert!(load(tmp.path(), &key2).is_none());
	}

	#[test]
	fn load_returns_none_on_empty_dir() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"a").unwrap();
		let key = CacheKey::from_path(&src, tmp.path()).unwrap();
		assert!(load(tmp.path(), &key).is_none());
	}

	#[test]
	fn cache_path_is_versioned_and_sharded() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"a").unwrap();
		let key = CacheKey::from_path(&src, tmp.path()).unwrap();
		let full = key.full_path(tmp.path());
		let s = full.to_string_lossy();
		assert!(s.contains(&format!("v{LAYOUT_VERSION}_{CACHE_FORMAT_VERSION}")));
		assert!(full.parent().unwrap().file_name().unwrap().len() == 2); // shard
	}
}
