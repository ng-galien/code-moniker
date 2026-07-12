use std::path::PathBuf;
use std::time::{Duration, Instant};

use code_moniker_core::lang::Lang;
use code_moniker_workspace::code::{
	CodeIndexPort, CodeIndexRefresh, LocalCodeIndex, LocalCodeIndexOptions,
};
use code_moniker_workspace::linkage::{
	LinkageGraphDelta, LinkageMemoryMetrics, LinkageRefreshImpact, LocalLinkage,
	TimedLinkageRefresh,
};
use code_moniker_workspace::snapshot::{LinkageSnapshot, WorkspaceRequest};
use code_moniker_workspace::source::{
	LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort,
};

fn main() -> anyhow::Result<()> {
	let options = BenchOptions::parse()?;
	let mut symbol_edit =
		SymbolEditBench::prepare(options.incremental_symbol_edit, &options.incremental_paths)?;
	let cache = LocalResourceCache::default();
	let source_options = options.source_options()?;
	let mut catalog_port = LocalSourceCatalog::new(source_options, cache.clone());
	let mut index_port =
		LocalCodeIndex::new(LocalCodeIndexOptions::new(options.cache_dir), cache.clone());
	let debug_cache = cache.clone();
	let mut linkage_port = LocalLinkage::new(cache);
	let request = WorkspaceRequest::new("bench-linkage");

	let catalog_timer = Instant::now();
	let catalog = catalog_port
		.load_catalog(&request)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let catalog_elapsed = catalog_timer.elapsed();

	let index_timer = Instant::now();
	let index = index_port
		.build_index(&catalog)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let index_elapsed = index_timer.elapsed();

	let linkage_timer = Instant::now();
	let timed_linkage = linkage_port
		.resolve_linkage_with_timings(&index)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let linkage = timed_linkage.snapshot;
	let linkage_elapsed = linkage_timer.elapsed();

	println!("phase\tms");
	println!("catalog\t{:.3}", millis(catalog_elapsed));
	println!("index\t{:.3}", millis(index_elapsed));
	println!("linkage\t{:.3}", millis(linkage_elapsed));
	println!(
		"total\t{:.3}",
		millis(catalog_elapsed + index_elapsed + linkage_elapsed)
	);
	println!("linkage_pass\tms");
	println!(
		"candidate_index\t{:.3}",
		millis(timed_linkage.timings.candidate_index)
	);
	println!(
		"manifest_policy\t{:.3}",
		millis(timed_linkage.timings.manifest_policy)
	);
	println!(
		"resolve_references\t{:.3}",
		millis(timed_linkage.timings.resolve_references)
	);
	println!(
		"semantic_enhance\t{:.3}",
		millis(timed_linkage.timings.semantic_enhance)
	);
	println!(
		"store_index\t{:.3}",
		millis(timed_linkage.timings.store_index)
	);
	println!(
		"project_snapshot\t{:.3}",
		millis(timed_linkage.timings.project_snapshot)
	);
	println!();
	println!("metric\tcount");
	println!("sources\t{}", catalog.sources.len());
	println!("symbols\t{}", index.symbols.len());
	println!("references\t{}", index.references.len());
	println!("resolved_refs\t{}", linkage.resolved_refs);
	println!("external_refs\t{}", linkage.external_refs);
	println!("manifest_blocked_refs\t{}", linkage.manifest_blocked_refs);
	println!("unresolved_refs\t{}", linkage.unresolved_refs);
	println!("ambiguous_refs\t{}", linkage.ambiguous_refs);
	println!("eligible_refs\t{}", eligible_refs(&linkage));
	println!(
		"linkage_score_percent\t{:.2}",
		linkage_score_percent(&linkage)
	);
	println!(
		"single_target_score_percent\t{:.2}",
		single_target_score_percent(&linkage)
	);
	print_linkage_memory("linkage_memory", &timed_linkage.memory);
	for call_name in &options.debug_calls {
		print_call_defs(&index, &debug_cache, call_name);
	}
	if let Some(limit) = options.unresolved_groups {
		print_unresolved_groups(&index, &linkage, limit);
	}
	if let Some(symbol_edit) = &mut symbol_edit {
		symbol_edit.apply_refresh_edit()?;
	}
	if !options.incremental_paths.is_empty() {
		print_incremental_refresh(
			&mut index_port,
			&mut linkage_port,
			&index,
			&linkage,
			&options.incremental_paths,
		)?;
	}
	Ok(())
}

#[derive(Debug)]
struct BenchOptions {
	paths: Vec<PathBuf>,
	project: Option<String>,
	cache_dir: Option<PathBuf>,
	lang: Option<Lang>,
	exclude_path_fragments: Vec<String>,
	incremental_paths: Vec<PathBuf>,
	incremental_symbol_edit: Option<IncrementalSymbolEdit>,
	unresolved_groups: Option<usize>,
	debug_calls: Vec<String>,
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
				"--incremental-path" => {
					options
						.incremental_paths
						.push(PathBuf::from(next_value(&mut args, "--incremental-path")?));
				}
				"--incremental-symbol-edit" => {
					options.incremental_symbol_edit = Some(parse_incremental_symbol_edit(
						&next_value(&mut args, "--incremental-symbol-edit")?,
					)?);
				}
				"--unresolved-groups" => {
					options.unresolved_groups =
						Some(next_value(&mut args, "--unresolved-groups")?.parse()?);
				}
				"--debug-call" => {
					options
						.debug_calls
						.push(next_value(&mut args, "--debug-call")?);
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
		let mut options = LocalSourceCatalogOptions::new(self.paths.clone(), self.project.clone());
		if self.lang.is_some() || !self.exclude_path_fragments.is_empty() {
			let files = self.filtered_files()?;
			options = options.with_files(files);
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
			exclude_path_fragments: Vec::new(),
			incremental_paths: Vec::new(),
			incremental_symbol_edit: None,
			unresolved_groups: None,
			debug_calls: Vec::new(),
		}
	}
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> anyhow::Result<String> {
	args.next()
		.ok_or_else(|| anyhow::anyhow!("{flag} expects a value"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IncrementalSymbolEdit {
	Add,
	Remove,
}

fn parse_incremental_symbol_edit(value: &str) -> anyhow::Result<IncrementalSymbolEdit> {
	match value {
		"add" => Ok(IncrementalSymbolEdit::Add),
		"remove" => Ok(IncrementalSymbolEdit::Remove),
		other => anyhow::bail!("unknown incremental symbol edit `{other}`; expected add or remove"),
	}
}

struct SymbolEditBench {
	path: PathBuf,
	original: String,
	edit: IncrementalSymbolEdit,
}

impl SymbolEditBench {
	fn prepare(
		edit: Option<IncrementalSymbolEdit>,
		paths: &[PathBuf],
	) -> anyhow::Result<Option<Self>> {
		let Some(edit) = edit else {
			return Ok(None);
		};
		let [path] = paths else {
			anyhow::bail!("--incremental-symbol-edit requires exactly one --incremental-path");
		};
		let original = std::fs::read_to_string(path)?;
		if original.contains(INCREMENTAL_SYMBOL_NAME) {
			anyhow::bail!(
				"{} already contains benchmark symbol `{}`",
				path.display(),
				INCREMENTAL_SYMBOL_NAME
			);
		}
		let bench = Self {
			path: path.clone(),
			original,
			edit,
		};
		if edit == IncrementalSymbolEdit::Remove {
			std::fs::write(&bench.path, bench.content_with_added_symbol()?)?;
		}
		Ok(Some(bench))
	}

	fn apply_refresh_edit(&mut self) -> anyhow::Result<()> {
		match self.edit {
			IncrementalSymbolEdit::Add => {
				std::fs::write(&self.path, self.content_with_added_symbol()?)?;
			}
			IncrementalSymbolEdit::Remove => {
				std::fs::write(&self.path, &self.original)?;
			}
		}
		Ok(())
	}

	fn content_with_added_symbol(&self) -> anyhow::Result<String> {
		if self.path.extension().and_then(|ext| ext.to_str()) != Some("java") {
			anyhow::bail!("--incremental-symbol-edit currently supports Java files only");
		}
		Ok(format!(
			"{}{}",
			self.original, INCREMENTAL_JAVA_SYMBOL_BLOCK
		))
	}
}

impl Drop for SymbolEditBench {
	fn drop(&mut self) {
		let _ = std::fs::write(&self.path, &self.original);
	}
}

const INCREMENTAL_SYMBOL_NAME: &str = "CodeMonikerIncrementalSymbolBench";
const INCREMENTAL_JAVA_SYMBOL_BLOCK: &str = r#"

final class CodeMonikerIncrementalSymbolBench {
	static void codeMonikerIncrementalSymbolBench() {}
}
"#;

fn print_usage() {
	println!(
		"bench_linkage [--project NAME] [--cache-dir PATH] [--lang TAG] [--exclude PATH_FRAGMENT] [--incremental-path PATH] [--incremental-symbol-edit add|remove] [--unresolved-groups N] [--debug-call NAME] [PATH]..."
	);
}

fn print_incremental_refresh(
	index_port: &mut impl CodeIndexPort,
	linkage_port: &mut LocalLinkage,
	index: &code_moniker_workspace::snapshot::CodeIndex,
	linkage: &code_moniker_workspace::snapshot::LinkageSnapshot,
	paths: &[PathBuf],
) -> anyhow::Result<()> {
	let index_timer = Instant::now();
	let refreshed = index_port
		.refresh_paths(index, paths)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let index_elapsed = index_timer.elapsed();
	let impact = LinkageRefreshImpact::with_graph_delta(
		refreshed.changed_sources.clone(),
		paths.to_vec(),
		LinkageGraphDelta::from_code_index(refreshed.graph_diff.clone()),
	);
	let linkage_timer = Instant::now();
	let timed_refresh = linkage_port
		.refresh_linkage_with_timings(linkage, &refreshed.index, impact)
		.map_err(|failure| anyhow::anyhow!("{:?}: {}", failure.resource, failure.message))?;
	let linkage_elapsed = linkage_timer.elapsed();
	let refreshed_linkage = &timed_refresh.snapshot;
	println!();
	println!("incremental_phase\tms");
	println!("index_refresh\t{:.3}", millis(index_elapsed));
	println!("linkage_refresh\t{:.3}", millis(linkage_elapsed));
	println!("total\t{:.3}", millis(index_elapsed + linkage_elapsed));
	println!("incremental_linkage_pass\tms");
	println!(
		"candidate_index\t{:.3}",
		millis(timed_refresh.timings.candidate_index)
	);
	println!(
		"plan_invalidation\t{:.3}",
		millis(timed_refresh.timings.plan_invalidation)
	);
	println!(
		"resolve_references\t{:.3}",
		millis(timed_refresh.timings.resolve_references)
	);
	println!(
		"apply_store\t{:.3}",
		millis(timed_refresh.timings.apply_store)
	);
	println!(
		"semantic_enhance\t{:.3}",
		millis(timed_refresh.timings.semantic_enhance)
	);
	println!(
		"rebuild_indexes\t{:.3}",
		millis(timed_refresh.timings.rebuild_indexes)
	);
	println!(
		"project_snapshot\t{:.3}",
		millis(timed_refresh.timings.project_snapshot)
	);
	print_incremental_metrics(paths, &refreshed, &timed_refresh, refreshed_linkage);
	print_linkage_memory("incremental_linkage_memory", &timed_refresh.memory);
	Ok(())
}

fn print_incremental_metrics(
	paths: &[PathBuf],
	refreshed: &CodeIndexRefresh,
	timed_refresh: &TimedLinkageRefresh,
	refreshed_linkage: &LinkageSnapshot,
) {
	println!("incremental_metric\tcount");
	println!("paths\t{}", paths.len());
	println!("changed_sources\t{}", refreshed.changed_sources.len());
	println!(
		"graph_changed_symbols\t{}",
		refreshed.graph_diff.changed_symbol_count()
	);
	println!(
		"graph_added_or_changed_symbols\t{}",
		refreshed.graph_diff.changed_symbols.len()
	);
	println!(
		"graph_added_symbols\t{}",
		refreshed.graph_diff.added_symbols.len()
	);
	println!(
		"graph_modified_symbols\t{}",
		refreshed.graph_diff.modified_symbols.len()
	);
	println!(
		"graph_removed_symbols\t{}",
		refreshed.graph_diff.removed_symbols.len()
	);
	println!(
		"graph_changed_references\t{}",
		refreshed.graph_diff.changed_reference_count()
	);
	println!(
		"graph_added_or_changed_references\t{}",
		refreshed.graph_diff.changed_references.len()
	);
	println!(
		"graph_removed_references\t{}",
		refreshed.graph_diff.removed_references.len()
	);
	println!(
		"symbol_id_remaps\t{}",
		refreshed.graph_diff.symbol_id_remaps.len()
	);
	println!(
		"reference_id_remaps\t{}",
		refreshed.graph_diff.reference_id_remaps.len()
	);
	println!(
		"unchanged_symbols\t{}",
		refreshed.graph_diff.unchanged_symbols
	);
	println!(
		"unchanged_references\t{}",
		refreshed.graph_diff.unchanged_references
	);
	println!("stale_refs\t{}", timed_refresh.timings.stale_refs);
	println!("changed_refs\t{}", timed_refresh.timings.changed_refs);
	println!("symbols\t{}", refreshed.index.symbols.len());
	println!("references\t{}", refreshed.index.references.len());
	println!("resolved_refs\t{}", refreshed_linkage.resolved_refs);
	println!("external_refs\t{}", refreshed_linkage.external_refs);
	println!(
		"manifest_blocked_refs\t{}",
		refreshed_linkage.manifest_blocked_refs
	);
	println!("unresolved_refs\t{}", refreshed_linkage.unresolved_refs);
	println!("ambiguous_refs\t{}", refreshed_linkage.ambiguous_refs);
	println!(
		"linkage_score_percent\t{:.2}",
		linkage_score_percent(refreshed_linkage)
	);
}

fn print_linkage_memory(label: &str, memory: &LinkageMemoryMetrics) {
	println!();
	println!("{label}\tvalue");
	println!("reference_sets\t{}", memory.reference_sets);
	println!("reference_set_values\t{}", memory.reference_set_values);
	println!(
		"reference_set_serialized_bytes\t{}",
		memory.reference_set_serialized_bytes
	);
	println!("symbol_sets\t{}", memory.symbol_sets);
	println!("symbol_set_values\t{}", memory.symbol_set_values);
	println!(
		"symbol_set_serialized_bytes\t{}",
		memory.symbol_set_serialized_bytes
	);
	println!("symbol_catalog_entries\t{}", memory.symbol_catalog_entries);
	println!("decisions\t{}", memory.decisions);
}

fn millis(duration: Duration) -> f64 {
	duration.as_secs_f64() * 1000.0
}

fn eligible_refs(linkage: &code_moniker_workspace::snapshot::LinkageSnapshot) -> usize {
	linkage.resolved_refs + linkage.manifest_blocked_refs + linkage.unresolved_refs
}

fn linkage_score_percent(linkage: &code_moniker_workspace::snapshot::LinkageSnapshot) -> f64 {
	let eligible = eligible_refs(linkage);
	if eligible == 0 {
		return 0.0;
	}
	(linkage.resolved_refs as f64 * 100.0) / eligible as f64
}

fn single_target_score_percent(linkage: &code_moniker_workspace::snapshot::LinkageSnapshot) -> f64 {
	let eligible = eligible_refs(linkage);
	if eligible == 0 {
		return 0.0;
	}
	let single_target = linkage.resolved_refs.saturating_sub(linkage.ambiguous_refs);
	(single_target as f64 * 100.0) / eligible as f64
}

fn print_unresolved_groups(
	index: &code_moniker_workspace::snapshot::CodeIndex,
	linkage: &code_moniker_workspace::snapshot::LinkageSnapshot,
	limit: usize,
) {
	let refs_by_id = index
		.references
		.iter()
		.map(|reference| (reference.id, reference))
		.collect::<std::collections::BTreeMap<_, _>>();
	let mut groups = std::collections::BTreeMap::<String, UnresolvedGroup>::new();
	for unresolved in &linkage.unresolved {
		let Some(reference) = refs_by_id.get(&unresolved.reference) else {
			continue;
		};
		let key = format!(
			"kind={} confidence={} call={} target={}",
			reference.kind,
			reference.confidence.as_deref().unwrap_or("-"),
			reference.call_name.as_deref().unwrap_or("-"),
			target_pattern(&unresolved.target_identity)
		);
		let group = groups.entry(key).or_default();
		group.count += 1;
		if group.samples.len() < 3 {
			group.samples.push(unresolved.target_identity.to_string());
		}
	}
	let mut groups = groups.into_iter().collect::<Vec<_>>();
	groups.sort_by_key(|(_, group)| std::cmp::Reverse(group.count));
	println!();
	println!("unresolved_group\tcount\tsamples");
	for (key, group) in groups.into_iter().take(limit) {
		println!("{}\t{}\t{}", key, group.count, group.samples.join(" | "));
	}
}

#[derive(Default)]
struct UnresolvedGroup {
	count: usize,
	samples: Vec<String>,
}

fn target_pattern(identity: &str) -> String {
	identity
		.split("://")
		.nth(1)
		.unwrap_or(identity)
		.split('/')
		.filter_map(segment_pattern)
		.collect::<Vec<_>>()
		.join("/")
}

fn segment_pattern(segment: &str) -> Option<String> {
	if segment == "." || segment.is_empty() {
		return None;
	}
	let (kind, _) = segment.split_once(':')?;
	Some(format!("{kind}:*"))
}

fn print_call_defs(
	index: &code_moniker_workspace::snapshot::CodeIndex,
	cache: &code_moniker_workspace::source::LocalResourceCache,
	name: &str,
) {
	println!();
	println!("debug_call\t{name}");
	if let Some(material) = cache.index_material(index.generation) {
		for file in &material.files {
			for def in file.graph.defs() {
				let Some(last) = def.moniker.as_view().segments().last() else {
					continue;
				};
				let last_name = std::str::from_utf8(last.name).unwrap_or("");
				let call_name = std::str::from_utf8(&def.call_name).unwrap_or("");
				if last_name.contains(name) || call_name == name {
					println!(
						"def\tkind={}\tcall={:?}/{:?}\tmoniker={}",
						std::str::from_utf8(&def.kind).unwrap_or(""),
						call_name,
						def.call_arity,
						file.identity.moniker_uri(&def.moniker)
					);
				}
			}
		}
	}
	for symbol in index.symbols.iter() {
		if symbol.name == name || symbol.identity.contains(&format!(":{name}")) {
			println!(
				"symbol\tkind={}\tname={}\tidentity={}",
				symbol.kind, symbol.name, symbol.identity
			);
		}
	}
	for reference in index.references.iter() {
		if reference.call_name.as_deref() == Some(name) || reference.target_identity.contains(name)
		{
			println!(
				"reference\tkind={}\tcall={:?}/{:?}\tconfidence={:?}\ttarget={}",
				reference.kind,
				reference.call_name,
				reference.call_arity,
				reference.confidence,
				reference.target_identity
			);
		}
	}
}
