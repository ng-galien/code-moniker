use std::collections::{BTreeMap, HashSet};
use std::mem;
#[cfg(target_os = "macos")]
use std::os::raw::{c_int, c_void};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use code_moniker_core::core::code_graph::{DefRecord, RefRecord};
use code_moniker_core::lang::Lang;
use code_moniker_workspace::changes::{ChangeOverlayPort, LocalChangeOverlay};
use code_moniker_workspace::code::{CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions};
use code_moniker_workspace::extract::{JavaExtractionPipeline, RustExtractionPipeline};
use code_moniker_workspace::linkage::{LinkagePort, LocalLinkage};
use code_moniker_workspace::snapshot::{
	ChangeOverlay, ChangeRecord, ChangeResource, CodeIndex, LinkageEdge, LinkageSnapshot,
	ReferenceRecord, SourceCatalog, SourceFileRecord, SourceUnit, SymbolRecord,
	UnresolvedReference, WorkspaceRequest,
};
use code_moniker_workspace::source::{
	CodeIndexMaterial, IndexedSourceFile, LocalResourceCache, LocalSourceCatalog,
	LocalSourceCatalogOptions, SourceCatalogPort,
};

#[cfg(feature = "heap-profile")]
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() -> anyhow::Result<()> {
	#[cfg(feature = "heap-profile")]
	let _profiler = dhat::Profiler::new_heap();

	let options = BenchOptions::parse()?;
	let cache = LocalResourceCache::default();
	let source_options = options.source_options()?;
	let mut catalog_port = LocalSourceCatalog::new(source_options, cache.clone());
	let mut index_port =
		LocalCodeIndex::new(LocalCodeIndexOptions::new(options.cache_dir), cache.clone());
	let mut linkage_port = LocalLinkage::new(cache.clone());
	let mut change_port = LocalChangeOverlay::new(cache.clone());
	let request = WorkspaceRequest::new("bench-memory");

	let started = Instant::now();
	let mut samples = Vec::new();
	samples.push(MemorySample::capture(
		"start",
		started.elapsed(),
		WorkspaceMemoryRefs::default(),
	));

	let catalog_timer = Instant::now();
	let catalog = catalog_port
		.load_catalog(&request)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	samples.push(MemorySample::capture(
		"catalog",
		catalog_timer.elapsed(),
		WorkspaceMemoryRefs {
			catalog: Some(&catalog),
			..WorkspaceMemoryRefs::default()
		},
	));

	let index_timer = Instant::now();
	let index = index_port
		.build_index(&catalog)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let material = cache.index_material(index.generation);
	samples.push(MemorySample::capture(
		"index",
		index_timer.elapsed(),
		WorkspaceMemoryRefs {
			catalog: Some(&catalog),
			index: Some(&index),
			material: material.as_deref(),
			..WorkspaceMemoryRefs::default()
		},
	));

	let linkage_timer = Instant::now();
	let linkage = linkage_port
		.resolve_linkage(&index)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let material = cache.index_material(index.generation);
	samples.push(MemorySample::capture(
		"linkage",
		linkage_timer.elapsed(),
		WorkspaceMemoryRefs {
			catalog: Some(&catalog),
			index: Some(&index),
			linkage: Some(&linkage),
			material: material.as_deref(),
			..WorkspaceMemoryRefs::default()
		},
	));

	let changes = if options.skip_changes {
		None
	} else {
		let changes_timer = Instant::now();
		let changes = change_port
			.build_change_overlay(&catalog, &index)
			.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
		let material = cache.index_material(index.generation);
		samples.push(MemorySample::capture(
			"changes",
			changes_timer.elapsed(),
			WorkspaceMemoryRefs {
				catalog: Some(&catalog),
				index: Some(&index),
				linkage: Some(&linkage),
				changes: Some(&changes),
				material: material.as_deref(),
			},
		));
		Some(changes)
	};

	let final_material = cache.index_material(index.generation);
	let final_estimate = MemoryEstimate::from_workspace(WorkspaceMemoryRefs {
		catalog: Some(&catalog),
		index: Some(&index),
		linkage: Some(&linkage),
		changes: changes.as_ref(),
		material: final_material.as_deref(),
	});
	print_samples(&samples, &catalog, &index, &linkage, changes.as_ref());
	print_categories(&final_estimate);
	print_usage_hint();
	Ok(())
}

#[derive(Debug)]
struct BenchOptions {
	paths: Vec<PathBuf>,
	project: Option<String>,
	cache_dir: Option<PathBuf>,
	lang: Option<Lang>,
	rust_pipeline: RustExtractionPipeline,
	java_pipeline: JavaExtractionPipeline,
	exclude_path_fragments: Vec<String>,
	skip_changes: bool,
}

impl BenchOptions {
	fn parse() -> anyhow::Result<Self> {
		let mut options = Self::default();
		let mut args = std::env::args().skip(1);
		while let Some(arg) = args.next() {
			match arg.as_str() {
				"--project" => {
					options.project = Some(next_value(&mut args, "--project")?);
				}
				"--cache-dir" => {
					options.cache_dir = Some(PathBuf::from(next_value(&mut args, "--cache-dir")?));
				}
				"--lang" => {
					let tag = next_value(&mut args, "--lang")?;
					options.lang = Some(
						Lang::from_tag(&tag)
							.ok_or_else(|| anyhow::anyhow!("unknown language tag `{tag}`"))?,
					);
				}
				"--exclude" => {
					options
						.exclude_path_fragments
						.push(next_value(&mut args, "--exclude")?);
				}
				"--rust-pipeline" => {
					options.rust_pipeline =
						parse_rust_pipeline(&next_value(&mut args, "--rust-pipeline")?)?;
				}
				"--java-pipeline" => {
					options.java_pipeline =
						parse_java_pipeline(&next_value(&mut args, "--java-pipeline")?)?;
				}
				"--skip-changes" => {
					options.skip_changes = true;
				}
				"--help" | "-h" => {
					print_usage();
					std::process::exit(0);
				}
				value if value.starts_with('-') => {
					anyhow::bail!("unknown option `{value}`");
				}
				path => options.paths.push(PathBuf::from(path)),
			}
		}
		if options.paths.is_empty() {
			options.paths.push(PathBuf::from("."));
		}
		Ok(options)
	}

	fn source_options(&self) -> anyhow::Result<LocalSourceCatalogOptions> {
		let mut options = LocalSourceCatalogOptions::new(self.paths.clone(), self.project.clone())
			.with_rust_pipeline(self.rust_pipeline)
			.with_java_pipeline(self.java_pipeline);
		if self.lang.is_some() || !self.exclude_path_fragments.is_empty() {
			options = options.with_files(self.filtered_files()?);
		}
		Ok(options)
	}

	fn filtered_files(&self) -> anyhow::Result<Vec<PathBuf>> {
		let [root] = self.paths.as_slice() else {
			anyhow::bail!("--lang/--exclude filters require exactly one benchmark root");
		};
		if !root.is_dir() {
			anyhow::bail!("--lang/--exclude filters require a directory benchmark root");
		}
		let mut files = Vec::new();
		for entry in ignore::WalkBuilder::new(root)
			.build()
			.filter_map(|entry| entry.ok())
		{
			if !entry
				.file_type()
				.is_some_and(|file_type| file_type.is_file())
			{
				continue;
			}
			let path = entry.into_path();
			let Ok(lang) = code_moniker_workspace::lang::path_to_lang(&path) else {
				continue;
			};
			if self.lang.is_some_and(|expected| expected != lang) {
				continue;
			}
			let rel = path.strip_prefix(root).unwrap_or(&path);
			let normalized = rel.to_string_lossy().replace('\\', "/");
			if self
				.exclude_path_fragments
				.iter()
				.any(|fragment| normalized.contains(fragment))
			{
				continue;
			}
			files.push(rel.to_path_buf());
		}
		files.sort();
		Ok(files)
	}
}

impl Default for BenchOptions {
	fn default() -> Self {
		Self {
			paths: Vec::new(),
			project: None,
			cache_dir: None,
			lang: None,
			rust_pipeline: RustExtractionPipeline::Sdk,
			java_pipeline: JavaExtractionPipeline::Sdk,
			exclude_path_fragments: Vec::new(),
			skip_changes: false,
		}
	}
}

struct MemorySample {
	phase: &'static str,
	elapsed: Duration,
	rss_kb: Option<u64>,
	estimated_heap: usize,
}

#[derive(Default)]
struct WorkspaceMemoryRefs<'a> {
	catalog: Option<&'a SourceCatalog>,
	index: Option<&'a CodeIndex>,
	linkage: Option<&'a LinkageSnapshot>,
	changes: Option<&'a ChangeOverlay>,
	material: Option<&'a CodeIndexMaterial>,
}

impl MemorySample {
	fn capture(phase: &'static str, elapsed: Duration, workspace: WorkspaceMemoryRefs<'_>) -> Self {
		let estimated_heap = MemoryEstimate::from_workspace(workspace).total();
		Self {
			phase,
			elapsed,
			rss_kb: current_rss_kb(),
			estimated_heap,
		}
	}
}

#[derive(Clone)]
struct MemoryRow {
	category: String,
	bytes: usize,
	detail: String,
}

#[derive(Default)]
struct MemoryEstimate {
	rows: Vec<MemoryRow>,
}

impl MemoryEstimate {
	fn from_workspace(workspace: WorkspaceMemoryRefs<'_>) -> Self {
		let mut estimate = Self::default();
		if let Some(catalog) = workspace.catalog {
			estimate_catalog(catalog, &mut estimate);
		}
		if let Some(index) = workspace.index {
			estimate_index(index, &mut estimate);
		}
		if let Some(linkage) = workspace.linkage {
			estimate_linkage(linkage, &mut estimate);
		}
		if let Some(changes) = workspace.changes {
			estimate_changes(changes, &mut estimate);
		}
		if let Some(material) = workspace.material {
			estimate_material(material, &mut estimate);
		}
		estimate
	}

	fn add(&mut self, category: impl Into<String>, bytes: usize, detail: impl Into<String>) {
		if bytes == 0 {
			return;
		}
		self.rows.push(MemoryRow {
			category: category.into(),
			bytes,
			detail: detail.into(),
		});
	}

	fn add_inline<T>(&mut self, category: &str, len: usize, capacity: usize) {
		self.add(
			category,
			mem::size_of::<T>() * capacity,
			format!(
				"{} records, capacity {}, {} B/record",
				len,
				capacity,
				mem::size_of::<T>()
			),
		);
	}

	fn total(&self) -> usize {
		self.rows.iter().map(|row| row.bytes).sum()
	}

	fn by_category(&self) -> Vec<MemoryRow> {
		let mut grouped = BTreeMap::<String, MemoryRow>::new();
		for row in &self.rows {
			grouped
				.entry(row.category.clone())
				.and_modify(|current| {
					current.bytes += row.bytes;
					if !current.detail.contains(&row.detail) {
						current.detail.push_str("; ");
						current.detail.push_str(&row.detail);
					}
				})
				.or_insert_with(|| row.clone());
		}
		let mut rows = grouped.into_values().collect::<Vec<_>>();
		rows.sort_by_key(|row| std::cmp::Reverse(row.bytes));
		rows
	}
}

fn estimate_catalog(catalog: &SourceCatalog, estimate: &mut MemoryEstimate) {
	estimate.add_inline::<SourceUnit>(
		"snapshot.catalog.records",
		catalog.sources.len(),
		catalog.sources.capacity(),
	);
	let payload = catalog
		.sources
		.iter()
		.map(|source| {
			source.id.as_str().len()
				+ source.display_name.capacity()
				+ source.language.as_ref().map_or(0, String::capacity)
		})
		.sum();
	estimate.add(
		"snapshot.catalog.strings",
		payload,
		format!("{} source ids/names/languages", catalog.sources.len()),
	);
}

fn estimate_index(index: &CodeIndex, estimate: &mut MemoryEstimate) {
	estimate.add_inline::<SourceFileRecord>(
		"snapshot.index.source_records",
		index.sources.len(),
		index.sources.capacity(),
	);
	estimate.add_inline::<SymbolRecord>(
		"snapshot.index.symbol_records",
		index.symbols.len(),
		index.symbols.capacity(),
	);
	estimate.add_inline::<ReferenceRecord>(
		"snapshot.index.reference_records",
		index.references.len(),
		index.references.capacity(),
	);
	estimate.add(
		"snapshot.index.source_strings",
		index
			.sources
			.iter()
			.map(source_record_payload)
			.sum::<usize>(),
		format!("{} source records", index.sources.len()),
	);
	estimate.add(
		"snapshot.index.symbol_strings",
		index.symbols.iter().map(symbol_payload).sum::<usize>(),
		format!("{} symbols", index.symbols.len()),
	);
	estimate.add(
		"snapshot.index.reference_strings",
		index
			.references
			.iter()
			.map(reference_owned_payload)
			.sum::<usize>(),
		format!("{} references", index.references.len()),
	);
	let target_pool = unique_arc_str_payload(index.references.iter().map(|r| &r.target_identity));
	estimate.add(
		"snapshot.index.reference_target_pool",
		target_pool,
		"unique Arc<str> reference targets",
	);
}

fn estimate_linkage(linkage: &LinkageSnapshot, estimate: &mut MemoryEstimate) {
	estimate.add_inline::<LinkageEdge>(
		"snapshot.linkage.resolved_edges",
		linkage.resolved.len(),
		linkage.resolved.capacity(),
	);
	estimate.add_inline::<UnresolvedReference>(
		"snapshot.linkage.unresolved_records",
		linkage.unresolved.len(),
		linkage.unresolved.capacity(),
	);
	estimate.add(
		"snapshot.linkage.resolved_strings",
		linkage
			.resolved
			.iter()
			.map(|edge| edge.reference.as_str().len() + edge.target.as_str().len())
			.sum(),
		format!("{} resolved edges", linkage.resolved.len()),
	);
	estimate.add(
		"snapshot.linkage.unresolved_strings",
		linkage
			.unresolved
			.iter()
			.map(|reference| reference.reference.as_str().len())
			.sum(),
		format!("{} unresolved refs", linkage.unresolved.len()),
	);
	estimate.add(
		"snapshot.linkage.unresolved_target_pool",
		unique_arc_str_payload(linkage.unresolved.iter().map(|r| &r.target_identity)),
		"unique Arc<str> unresolved targets",
	);
}

fn estimate_changes(changes: &ChangeOverlay, estimate: &mut MemoryEstimate) {
	estimate.add_inline::<ChangeResource>(
		"snapshot.changes.resource_records",
		changes.resources.len(),
		changes.resources.capacity(),
	);
	estimate.add_inline::<String>(
		"snapshot.changes.diagnostic_records",
		changes.diagnostics.len(),
		changes.diagnostics.capacity(),
	);
	estimate.add_inline::<ChangeRecord>(
		"snapshot.changes.change_records",
		changes.changes.len(),
		changes.changes.capacity(),
	);
	estimate.add(
		"snapshot.changes.changed_symbol_strings",
		changes
			.changed_symbols
			.iter()
			.map(|symbol| symbol.as_str().len())
			.sum(),
		format!("{} changed symbols", changes.changed_symbols.len()),
	);
	estimate.add(
		"snapshot.changes.strings",
		changes
			.resources
			.iter()
			.map(change_resource_payload)
			.sum::<usize>()
			+ changes
				.diagnostics
				.iter()
				.map(String::capacity)
				.sum::<usize>()
			+ changes.changes.iter().map(change_payload).sum::<usize>(),
		format!("{} changes", changes.changes.len()),
	);
}

fn estimate_material(material: &CodeIndexMaterial, estimate: &mut MemoryEstimate) {
	estimate.add_inline::<IndexedSourceFile>(
		"material.files.records",
		material.files.len(),
		material.files.capacity(),
	);
	estimate.add(
		"material.files.source_text",
		material
			.files
			.iter()
			.map(|file| file.source.capacity())
			.sum(),
		format!("{} retained source texts", material.files.len()),
	);
	estimate.add(
		"material.files.metadata_strings",
		material
			.files
			.iter()
			.map(|file| indexed_source_payload(file))
			.sum(),
		format!("{} indexed source files", material.files.len()),
	);
	let def_count = material
		.files
		.iter()
		.map(|file| file.graph.defs().count())
		.sum::<usize>();
	let ref_count = material
		.files
		.iter()
		.map(|file| file.graph.refs().count())
		.sum::<usize>();
	estimate.add(
		"material.graph.def_records",
		def_count * mem::size_of::<DefRecord>(),
		format!(
			"{def_count} def records, {} B/record",
			mem::size_of::<DefRecord>()
		),
	);
	estimate.add(
		"material.graph.ref_records",
		ref_count * mem::size_of::<RefRecord>(),
		format!(
			"{ref_count} ref records, {} B/record",
			mem::size_of::<RefRecord>()
		),
	);
	let mut def_payload = 0usize;
	let mut ref_payload = 0usize;
	let mut graph_index_payload = 0usize;
	for file in &material.files {
		for def in file.graph.defs() {
			def_payload += def_payload_bytes(def);
			graph_index_payload += def.moniker.as_encoded().len();
		}
		for reference in file.graph.refs() {
			ref_payload += ref_payload_bytes(reference);
		}
	}
	estimate.add(
		"material.graph.def_payload",
		def_payload,
		"monikers and boxed def attrs",
	);
	estimate.add(
		"material.graph.ref_payload",
		ref_payload,
		"targets and boxed ref attrs",
	);
	estimate.add(
		"material.graph.def_index_moniker_keys",
		graph_index_payload,
		"private lookup index duplicates def moniker keys",
	);
	estimate.add(
		"material.lookup.symbols_by_moniker",
		material
			.symbols_by_moniker
			.iter()
			.map(|(moniker, symbol)| moniker.as_encoded().len() + symbol.as_str().len())
			.sum(),
		format!(
			"{} moniker -> symbol entries",
			material.symbols_by_moniker.len()
		),
	);
}

fn source_record_payload(source: &SourceFileRecord) -> usize {
	source.id.as_str().len()
		+ source.uri.capacity()
		+ source.path.capacity()
		+ source.rel_path.capacity()
		+ source.anchor.capacity()
		+ source.language.capacity()
		+ source.text.capacity()
}

fn symbol_payload(symbol: &SymbolRecord) -> usize {
	symbol.id.as_str().len()
		+ symbol.source.as_str().len()
		+ symbol.identity.capacity()
		+ symbol.name.capacity()
		+ symbol.kind.capacity()
		+ symbol.signature.capacity()
		+ symbol
			.parent
			.as_ref()
			.map_or(0, |parent| parent.as_str().len())
}

fn reference_owned_payload(reference: &ReferenceRecord) -> usize {
	reference.id.as_str().len()
		+ reference.source.as_str().len()
		+ reference.source_symbol.as_str().len()
		+ reference.kind.capacity()
		+ reference.call_name.as_ref().map_or(0, String::capacity)
		+ reference.confidence.as_ref().map_or(0, String::capacity)
		+ reference.receiver.as_ref().map_or(0, String::capacity)
		+ reference.alias.as_ref().map_or(0, String::capacity)
}

fn change_resource_payload(resource: &ChangeResource) -> usize {
	resource.label.capacity() + resource.message.capacity()
}

fn change_payload(change: &ChangeRecord) -> usize {
	change.id.as_str().len()
		+ change
			.source
			.as_ref()
			.map_or(0, |source| source.as_str().len())
		+ change.source_uri.as_ref().map_or(0, String::capacity)
		+ change
			.symbol
			.as_ref()
			.map_or(0, |symbol| symbol.as_str().len())
		+ change.identity.capacity()
		+ change.language.capacity()
		+ change.file_path.capacity()
		+ change.name.capacity()
		+ change.kind.capacity()
}

fn indexed_source_payload(file: &IndexedSourceFile) -> usize {
	file.source_id.as_str().len()
		+ file.source_uri.capacity()
		+ file.path.as_os_str().len()
		+ file.rel_path.as_os_str().len()
		+ file.anchor.as_os_str().len()
}

fn def_payload_bytes(def: &DefRecord) -> usize {
	def.moniker.as_encoded().len()
		+ def.kind.len()
		+ def.visibility.len()
		+ def.signature.len()
		+ def.call_name.len()
		+ def.binding.len()
		+ def.origin.len()
}

fn ref_payload_bytes(reference: &RefRecord) -> usize {
	reference.target.as_encoded().len()
		+ reference.kind.len()
		+ reference.receiver_hint.len()
		+ reference.alias.len()
		+ reference.confidence.len()
		+ reference.call_name.len()
		+ reference.binding.len()
}

fn unique_arc_str_payload<'a>(values: impl Iterator<Item = &'a Arc<str>>) -> usize {
	let mut seen = HashSet::<(usize, usize)>::new();
	let mut bytes = 0usize;
	for value in values {
		let key = (value.as_ptr() as usize, value.len());
		if seen.insert(key) {
			bytes += value.len();
		}
	}
	bytes
}

#[cfg(target_os = "macos")]
fn current_rss_kb() -> Option<u64> {
	const PROC_PIDTASKINFO: c_int = 4;
	let mut info = ProcTaskInfo::default();
	let size = mem::size_of::<ProcTaskInfo>() as c_int;
	let result = unsafe {
		proc_pidinfo(
			std::process::id() as c_int,
			PROC_PIDTASKINFO,
			0,
			&mut info as *mut ProcTaskInfo as *mut c_void,
			size,
		)
	};
	(result == size).then_some(info.pti_resident_size / 1024)
}

#[cfg(target_os = "linux")]
fn current_rss_kb() -> Option<u64> {
	const SC_PAGESIZE: i64 = 30;
	let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
	let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
	let page_size = unsafe { sysconf(SC_PAGESIZE) };
	(page_size > 0).then_some(resident_pages * page_size as u64 / 1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_rss_kb() -> Option<u64> {
	None
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct ProcTaskInfo {
	pti_virtual_size: u64,
	pti_resident_size: u64,
	pti_total_user: u64,
	pti_total_system: u64,
	pti_threads_user: u64,
	pti_threads_system: u64,
	pti_policy: i32,
	pti_faults: i32,
	pti_pageins: i32,
	pti_cow_faults: i32,
	pti_messages_sent: i32,
	pti_messages_received: i32,
	pti_syscalls_mach: i32,
	pti_syscalls_unix: i32,
	pti_csw: i32,
	pti_threadnum: i32,
	pti_numrunning: i32,
	pti_priority: i32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
	fn proc_pidinfo(
		pid: c_int,
		flavor: c_int,
		arg: u64,
		buffer: *mut c_void,
		buffersize: c_int,
	) -> c_int;
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
	fn sysconf(name: i64) -> i64;
}

fn print_samples(
	samples: &[MemorySample],
	catalog: &SourceCatalog,
	index: &CodeIndex,
	linkage: &LinkageSnapshot,
	changes: Option<&ChangeOverlay>,
) {
	println!("summary\tcount");
	println!("sources\t{}", catalog.sources.len());
	println!("symbols\t{}", index.symbols.len());
	println!("references\t{}", index.references.len());
	println!("resolved_refs\t{}", linkage.resolved_refs);
	println!("external_refs\t{}", linkage.external_refs);
	println!("unresolved_refs\t{}", linkage.unresolved_refs);
	println!(
		"changes\t{}",
		changes.map_or(0, |changes| changes.changes.len())
	);
	println!();
	println!("phase\telapsed_ms\trss_mib\testimated_heap_mib");
	let baseline_rss = samples.first().and_then(|sample| sample.rss_kb);
	for sample in samples {
		let rss = sample
			.rss_kb
			.map(|kb| format!("{:.2}", kb as f64 / 1024.0))
			.unwrap_or_else(|| "-".to_string());
		let delta = match (baseline_rss, sample.rss_kb) {
			(Some(base), Some(kb)) => format!("{:+.2}", (kb as i64 - base as i64) as f64 / 1024.0),
			_ => "-".to_string(),
		};
		println!(
			"{}\t{:.3}\t{}\t{}\t# rss_delta_mib={}",
			sample.phase,
			millis(sample.elapsed),
			rss,
			mib(sample.estimated_heap),
			delta
		);
	}
	println!();
}

fn print_categories(estimate: &MemoryEstimate) {
	println!("category\testimated_mib\testimated_bytes\tdetail");
	for row in estimate.by_category() {
		println!(
			"{}\t{}\t{}\t{}",
			row.category,
			mib(row.bytes),
			row.bytes,
			row.detail
		);
	}
	println!();
	println!("estimated_total_mib\t{}", mib(estimate.total()));
}

fn print_usage_hint() {
	println!();
	println!("notes");
	println!("rss_mib is native process RSS; estimated_heap_mib is a model-level lower bound.");
	println!(
		"for allocation traces, run: cargo run -p code-moniker-workspace --features heap-profile --example bench_memory -- <path>"
	);
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> anyhow::Result<String> {
	args.next()
		.ok_or_else(|| anyhow::anyhow!("{flag} expects a value"))
}

fn parse_rust_pipeline(value: &str) -> anyhow::Result<RustExtractionPipeline> {
	match value {
		"legacy" => Ok(RustExtractionPipeline::Legacy),
		"sdk" => Ok(RustExtractionPipeline::Sdk),
		other => anyhow::bail!("unknown Rust pipeline `{other}`; expected legacy or sdk"),
	}
}

fn parse_java_pipeline(value: &str) -> anyhow::Result<JavaExtractionPipeline> {
	match value {
		"legacy" => Ok(JavaExtractionPipeline::Legacy),
		"sdk" => Ok(JavaExtractionPipeline::Sdk),
		other => anyhow::bail!("unknown Java pipeline `{other}`; expected legacy or sdk"),
	}
}

fn print_usage() {
	println!(
		"bench_memory [--project NAME] [--cache-dir PATH] [--lang TAG] [--rust-pipeline legacy|sdk] [--java-pipeline legacy|sdk] [--exclude PATH_FRAGMENT] [--skip-changes] [PATH]..."
	);
}

fn millis(duration: Duration) -> f64 {
	duration.as_secs_f64() * 1000.0
}

fn mib(bytes: usize) -> String {
	format!("{:.2}", bytes as f64 / (1024.0 * 1024.0))
}
