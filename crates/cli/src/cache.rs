use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use code_moniker_core::core::code_graph::CodeGraph;
use code_moniker_core::core::code_graph::encoding::{self, LAYOUT_VERSION};
use rustc_hash::FxHasher;

use crate::extract;
use code_moniker_core::lang::Lang;

const CACHE_MAGIC: u32 = 0xC0DE_2106;
// Bump when cached graph semantics change, even if the binary layout stays stable.
const CACHE_FORMAT_VERSION: u32 = 4;
const OFF_MAGIC: usize = 0;
const OFF_FORMAT: usize = 4;
const OFF_MTIME: usize = 8;
const OFF_SIZE: usize = 16;
const OFF_ANCHOR: usize = 24;
const OFF_CONTEXT: usize = 32;
const OFF_PATH_LEN: usize = 40;
const HEADER_FIXED: usize = OFF_PATH_LEN + 4;

static TMP_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct CacheKey {
	pub abs_path: PathBuf,
	pub mtime: u64,
	pub size: u64,
	pub anchor_hash: u64,
	pub context_hash: u64,
}

impl CacheKey {
	pub fn from_path(path: &Path, anchor: &Path) -> io::Result<Self> {
		Self::from_path_with_context(path, anchor, &extract::Context::default())
	}

	pub fn from_path_with_context(
		path: &Path,
		anchor: &Path,
		ctx: &extract::Context,
	) -> io::Result<Self> {
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
			context_hash: hash_context(ctx),
		})
	}

	fn path_hash(&self) -> u64 {
		hash_path(&self.abs_path)
	}

	fn shard(&self) -> String {
		format!("{:02x}", (self.path_hash() & 0xff) as u8)
	}

	fn filename(&self) -> String {
		format!(
			"{:016x}_{:016x}_{:016x}.bin",
			self.path_hash(),
			self.anchor_hash,
			self.context_hash,
		)
	}

	fn full_path(&self, root: &Path) -> PathBuf {
		root.join(format!("v{LAYOUT_VERSION}_{CACHE_FORMAT_VERSION}"))
			.join(self.shard())
			.join(self.filename())
	}

	fn abs_path_bytes(&self) -> &[u8] {
		path_bytes(&self.abs_path)
	}
}

pub fn load(cache_dir: &Path, key: &CacheKey) -> Option<CodeGraph> {
	let path = key.full_path(cache_dir);
	let bytes = fs::read(&path).ok()?;
	let body = validate_header(&bytes, key)?;
	match encoding::decode(body) {
		Ok(g) => Some(g),
		Err(e) => {
			eprintln!(
				"code-moniker: cache decode failed at {} ({e}); ignoring",
				path.display(),
			);
			None
		}
	}
}

pub fn store(cache_dir: &Path, key: &CacheKey, graph: &CodeGraph) {
	let _ = try_store(cache_dir, key, graph);
}

fn try_store(cache_dir: &Path, key: &CacheKey, graph: &CodeGraph) -> io::Result<()> {
	let path = key.full_path(cache_dir);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let body = encoding::encode(graph)
		.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
	let path_bytes = key.abs_path_bytes();
	let mut buf = Vec::with_capacity(HEADER_FIXED + path_bytes.len() + body.len());
	buf.extend_from_slice(&CACHE_MAGIC.to_le_bytes());
	buf.extend_from_slice(&CACHE_FORMAT_VERSION.to_le_bytes());
	buf.extend_from_slice(&key.mtime.to_le_bytes());
	buf.extend_from_slice(&key.size.to_le_bytes());
	buf.extend_from_slice(&key.anchor_hash.to_le_bytes());
	buf.extend_from_slice(&key.context_hash.to_le_bytes());
	buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
	buf.extend_from_slice(path_bytes);
	buf.extend_from_slice(&body);

	let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
	let tmp = path.with_extension(format!("tmp.{}.{nonce}", std::process::id()));
	let write_result = (|| -> io::Result<()> {
		let mut f = fs::File::create(&tmp)?;
		f.write_all(&buf)?;
		f.sync_data()?;
		Ok(())
	})();
	if let Err(e) = write_result {
		let _ = fs::remove_file(&tmp);
		return Err(e);
	}
	fs::rename(&tmp, &path)
}

pub fn load_or_extract(
	path: &Path,
	anchor: &Path,
	lang: Lang,
	cache_dir: Option<&Path>,
	ctx: &extract::Context,
) -> Option<(CodeGraph, Option<String>)> {
	load_or_extract_result(path, anchor, lang, cache_dir, ctx).ok()
}

pub fn load_or_extract_result(
	path: &Path,
	anchor: &Path,
	lang: Lang,
	cache_dir: Option<&Path>,
	ctx: &extract::Context,
) -> io::Result<(CodeGraph, Option<String>)> {
	if let Some(dir) = cache_dir
		&& let Ok(key) = CacheKey::from_path_with_context(path, anchor, ctx)
	{
		if let Some(g) = load(dir, &key) {
			return Ok((g, None));
		}
		let source = fs::read_to_string(path)?;
		let graph = extract::extract_with(lang, &source, anchor, ctx);
		store(dir, &key, &graph);
		return Ok((graph, Some(source)));
	}
	let source = fs::read_to_string(path)?;
	let graph = extract::extract_with(lang, &source, anchor, ctx);
	Ok((graph, Some(source)))
}

fn validate_header<'a>(bytes: &'a [u8], key: &CacheKey) -> Option<&'a [u8]> {
	if bytes.len() < HEADER_FIXED {
		return None;
	}
	let magic = u32::from_le_bytes(bytes[OFF_MAGIC..OFF_FORMAT].try_into().ok()?);
	if magic != CACHE_MAGIC {
		return None;
	}
	let version = u32::from_le_bytes(bytes[OFF_FORMAT..OFF_MTIME].try_into().ok()?);
	if version != CACHE_FORMAT_VERSION {
		return None;
	}
	let mtime = u64::from_le_bytes(bytes[OFF_MTIME..OFF_SIZE].try_into().ok()?);
	let size = u64::from_le_bytes(bytes[OFF_SIZE..OFF_ANCHOR].try_into().ok()?);
	let anchor_hash = u64::from_le_bytes(bytes[OFF_ANCHOR..OFF_CONTEXT].try_into().ok()?);
	let context_hash = u64::from_le_bytes(bytes[OFF_CONTEXT..OFF_PATH_LEN].try_into().ok()?);
	if mtime != key.mtime
		|| size != key.size
		|| anchor_hash != key.anchor_hash
		|| context_hash != key.context_hash
	{
		return None;
	}
	let path_len = u32::from_le_bytes(bytes[OFF_PATH_LEN..HEADER_FIXED].try_into().ok()?) as usize;
	if HEADER_FIXED + path_len > bytes.len() {
		return None;
	}
	let stored_path = &bytes[HEADER_FIXED..HEADER_FIXED + path_len];
	if stored_path != key.abs_path_bytes() {
		return None;
	}
	Some(&bytes[HEADER_FIXED + path_len..])
}

#[cfg(unix)]
fn path_bytes(p: &Path) -> &[u8] {
	use std::os::unix::ffi::OsStrExt;
	p.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_bytes(p: &Path) -> &[u8] {
	// non-unix fallback; PG-supported hosts are all unix
	p.to_str().map(|s| s.as_bytes()).unwrap_or(&[])
}

fn hash_path(p: &Path) -> u64 {
	let mut h = FxHasher::default();
	path_bytes(p).hash(&mut h);
	h.finish()
}

fn hash_context(ctx: &extract::Context) -> u64 {
	let mut h = FxHasher::default();
	ctx.project.hash(&mut h);
	ctx.ts.aliases.len().hash(&mut h);
	for alias in &ctx.ts.aliases {
		alias.pattern.hash(&mut h);
		alias.substitution.hash(&mut h);
	}
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

		store(tmp.path(), &key, &g);
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
		store(tmp.path(), &key, &graph_with_one_def());

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
		store(tmp.path(), &key1, &graph_with_one_def());
		assert!(load(tmp.path(), &key1).is_some());
		assert!(load(tmp.path(), &key2).is_none());
	}

	#[test]
	fn load_misses_when_context_changes() {
		let tmp = tempfile::tempdir().unwrap();
		let src = tmp.path().join("src.ts");
		std::fs::write(&src, b"export class Foo {}\n").unwrap();
		let anchor = tmp.path().join("anchor");
		let ctx_one = extract::Context {
			project: Some("one".into()),
			..extract::Context::default()
		};
		let ctx_two = extract::Context {
			project: Some("two".into()),
			..extract::Context::default()
		};
		let key1 = CacheKey::from_path_with_context(&src, &anchor, &ctx_one).unwrap();
		let key2 = CacheKey::from_path_with_context(&src, &anchor, &ctx_two).unwrap();

		store(tmp.path(), &key1, &graph_with_one_def());

		assert!(load(tmp.path(), &key1).is_some());
		assert!(load(tmp.path(), &key2).is_none());
		assert_ne!(key1.full_path(tmp.path()), key2.full_path(tmp.path()));
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
		assert!(full.parent().unwrap().file_name().unwrap().len() == 2);
	}
}
