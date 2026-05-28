use std::path::PathBuf;
use std::time::{Duration, Instant};

use code_moniker_core::lang::Lang;
use code_moniker_workspace::code::{CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions};
use code_moniker_workspace::extract::{JavaExtractionPipeline, RustExtractionPipeline};
use code_moniker_workspace::linkage::LocalLinkage;
use code_moniker_workspace::snapshot::WorkspaceRequest;
use code_moniker_workspace::source::{
	LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort,
};

fn main() -> anyhow::Result<()> {
	let options = BenchOptions::parse()?;
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
	let linkage = timed_linkage.graph;
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
		"semantic_prepare\t{:.3}",
		millis(timed_linkage.timings.semantic_prepare)
	);
	println!(
		"semantic_enhance\t{:.3}",
		millis(timed_linkage.timings.semantic_enhance)
	);
	println!(
		"project_report\t{:.3}",
		millis(timed_linkage.timings.project_report)
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
	for call_name in &options.debug_calls {
		print_call_defs(&index, &debug_cache, call_name);
	}
	if let Some(limit) = options.unresolved_groups {
		print_unresolved_groups(&index, &linkage, limit);
	}
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
				"--rust-pipeline" => {
					options.rust_pipeline =
						parse_rust_pipeline(&next_value(&mut args, "--rust-pipeline")?)?;
				}
				"--java-pipeline" => {
					options.java_pipeline =
						parse_java_pipeline(&next_value(&mut args, "--java-pipeline")?)?;
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
		let mut options = LocalSourceCatalogOptions::new(self.paths.clone(), self.project.clone())
			.with_rust_pipeline(self.rust_pipeline)
			.with_java_pipeline(self.java_pipeline);
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
			rust_pipeline: RustExtractionPipeline::Sdk,
			java_pipeline: JavaExtractionPipeline::Sdk,
			exclude_path_fragments: Vec::new(),
			unresolved_groups: None,
			debug_calls: Vec::new(),
		}
	}
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
		"bench_linkage [--project NAME] [--cache-dir PATH] [--lang TAG] [--rust-pipeline legacy|sdk] [--java-pipeline legacy|sdk] [--exclude PATH_FRAGMENT] [--unresolved-groups N] [--debug-call NAME] [PATH]..."
	);
}

fn millis(duration: Duration) -> f64 {
	duration.as_secs_f64() * 1000.0
}

fn eligible_refs(linkage: &code_moniker_workspace::snapshot::LinkageGraph) -> usize {
	linkage.resolved_refs + linkage.manifest_blocked_refs + linkage.unresolved_refs
}

fn linkage_score_percent(linkage: &code_moniker_workspace::snapshot::LinkageGraph) -> f64 {
	let eligible = eligible_refs(linkage);
	if eligible == 0 {
		return 0.0;
	}
	(linkage.resolved_refs as f64 * 100.0) / eligible as f64
}

fn single_target_score_percent(linkage: &code_moniker_workspace::snapshot::LinkageGraph) -> f64 {
	let eligible = eligible_refs(linkage);
	if eligible == 0 {
		return 0.0;
	}
	let single_target = linkage.resolved_refs.saturating_sub(linkage.ambiguous_refs);
	(single_target as f64 * 100.0) / eligible as f64
}

fn print_unresolved_groups(
	index: &code_moniker_workspace::snapshot::CodeIndex,
	linkage: &code_moniker_workspace::snapshot::LinkageGraph,
	limit: usize,
) {
	let refs_by_id = index
		.references
		.iter()
		.map(|reference| (reference.id.as_str(), reference))
		.collect::<std::collections::BTreeMap<_, _>>();
	let mut groups = std::collections::BTreeMap::<String, UnresolvedGroup>::new();
	for unresolved in &linkage.unresolved {
		let Some(reference) = refs_by_id.get(unresolved.reference.as_str()) else {
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
	for symbol in &index.symbols {
		if symbol.name == name || symbol.identity.contains(&format!(":{name}")) {
			println!(
				"symbol\tkind={}\tname={}\tidentity={}",
				symbol.kind, symbol.name, symbol.identity
			);
		}
	}
	for reference in &index.references {
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
