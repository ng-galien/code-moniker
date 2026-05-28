use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::changes::ChangeOverlayPort;
use crate::code::CodeIndexPort;
use crate::linkage::LinkagePort;
use crate::source::SourceCatalogPort;

use super::model::{
	ChangeOverlay, CodeIndex, CodeIndexFields, LinkageGraph, ResourceGeneration, SourceCatalog,
	SourceFileRecord, SourceFileRecordFields, WorkspaceFailure, WorkspaceRequest,
	WorkspaceResource, WorkspaceResult, WorkspaceSnapshot, WorkspaceTimings, WorkspaceTransition,
};

pub struct WorkspaceSnapshotRefresh<Sources, Index, Linkage, Changes> {
	ports: WorkspaceSnapshotPorts<Sources, Index, Linkage, Changes>,
	state: WorkspaceSnapshotState,
}

struct WorkspaceSnapshotPorts<Sources, Index, Linkage, Changes> {
	source_catalog: Sources,
	code_index: Index,
	linkage: Linkage,
	change_overlay: Changes,
}

struct WorkspaceSnapshotState {
	next_generation: u64,
	snapshot: Option<Arc<WorkspaceSnapshot>>,
	last_failure: Option<super::model::WorkspaceFailure>,
}

impl<Sources, Index, Linkage, Changes> WorkspaceSnapshotRefresh<Sources, Index, Linkage, Changes>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	pub fn new(
		source_catalog: Sources,
		code_index: Index,
		linkage: Linkage,
		change_overlay: Changes,
	) -> Self {
		Self {
			ports: WorkspaceSnapshotPorts {
				source_catalog,
				code_index,
				linkage,
				change_overlay,
			},
			state: WorkspaceSnapshotState::new(),
		}
	}

	pub fn refresh(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.run_phase(|ports, _current, generation| {
			build_complete_snapshot(
				&mut ports.source_catalog,
				&mut ports.code_index,
				&mut ports.linkage,
				&mut ports.change_overlay,
				request,
				generation,
			)
		})
	}

	pub fn load_catalog(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.run_phase(|ports, _current, generation| {
			build_catalog_snapshot(&mut ports.source_catalog, request, generation)
		})
	}

	pub fn load_index(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.run_phase(|ports, current, generation| {
			let catalog_source = request
				.should_reuse_current_catalog()
				.then_some(current)
				.flatten();
			build_index_only_snapshot(
				catalog_source,
				&mut ports.source_catalog,
				&mut ports.code_index,
				request,
				generation,
			)
		})
	}

	pub fn resolve_linkage(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.run_phase(|ports, current, generation| {
			build_linkage_snapshot(
				current,
				&mut ports.linkage,
				&mut ports.change_overlay,
				request,
				generation,
			)
		})
	}

	pub fn replace_snapshot(&mut self, snapshot: WorkspaceSnapshot) {
		self.state.replace_snapshot(snapshot);
	}

	pub fn replace_snapshot_arc(&mut self, snapshot: Arc<WorkspaceSnapshot>) {
		self.state.replace_snapshot_arc(snapshot);
	}

	pub fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.state.snapshot()
	}

	pub fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.state.snapshot_arc()
	}

	pub fn last_failure(&self) -> Option<&super::model::WorkspaceFailure> {
		self.state.last_failure()
	}

	fn run_phase(
		&mut self,
		phase: impl FnOnce(
			&mut WorkspaceSnapshotPorts<Sources, Index, Linkage, Changes>,
			Option<&WorkspaceSnapshot>,
			ResourceGeneration,
		) -> WorkspaceResult<WorkspaceSnapshot>,
	) -> WorkspaceTransition {
		let generation = self.state.allocate_generation();
		let current = self.state.snapshot();
		let result = phase(&mut self.ports, current, generation);
		self.state.publish(result)
	}
}

impl WorkspaceSnapshotState {
	fn new() -> Self {
		Self {
			next_generation: 1,
			snapshot: None,
			last_failure: None,
		}
	}

	fn allocate_generation(&mut self) -> ResourceGeneration {
		let generation = ResourceGeneration::new(self.next_generation);
		self.next_generation += 1;
		generation
	}

	fn publish(&mut self, result: WorkspaceResult<WorkspaceSnapshot>) -> WorkspaceTransition {
		match result {
			Ok(snapshot) => {
				let generation = snapshot.generation;
				self.snapshot = Some(Arc::new(snapshot));
				self.last_failure = None;
				WorkspaceTransition::Ready { generation }
			}
			Err(failure) => {
				let preserved_generation =
					self.snapshot.as_ref().map(|snapshot| snapshot.generation);
				self.last_failure = Some(failure.clone());
				WorkspaceTransition::Failed {
					failure,
					preserved_generation,
				}
			}
		}
	}

	fn replace_snapshot(&mut self, snapshot: WorkspaceSnapshot) {
		self.next_generation = self.next_generation.max(snapshot.generation.value() + 1);
		self.snapshot = Some(Arc::new(snapshot));
		self.last_failure = None;
	}

	fn replace_snapshot_arc(&mut self, snapshot: Arc<WorkspaceSnapshot>) {
		self.next_generation = self.next_generation.max(snapshot.generation.value() + 1);
		self.snapshot = Some(snapshot);
		self.last_failure = None;
	}

	fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.snapshot.as_deref()
	}

	fn snapshot_arc(&self) -> Option<Arc<WorkspaceSnapshot>> {
		self.snapshot.clone()
	}

	fn last_failure(&self) -> Option<&super::model::WorkspaceFailure> {
		self.last_failure.as_ref()
	}
}

fn build_complete_snapshot(
	source_catalog: &mut impl SourceCatalogPort,
	code_index: &mut impl CodeIndexPort,
	linkage: &mut impl LinkagePort,
	change_overlay: &mut impl ChangeOverlayPort,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let catalog_timer = Instant::now();
	let catalog = source_catalog.load_catalog(&request)?;
	let catalog_elapsed = catalog_timer.elapsed();
	let index_timer = Instant::now();
	let index = code_index.build_index(&catalog)?;
	let index_elapsed = index_timer.elapsed();
	let linkage_timer = Instant::now();
	let linkage = linkage.resolve_linkage(&index)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let changes_timer = Instant::now();
	let changes = change_overlay.build_change_overlay(&catalog, &index, &linkage)?;
	let changes_elapsed = changes_timer.elapsed();
	let timings = timings(
		catalog_elapsed,
		&index,
		index_elapsed,
		linkage_elapsed,
		changes_elapsed,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

fn build_index_only_snapshot(
	current: Option<&WorkspaceSnapshot>,
	source_catalog: &mut impl SourceCatalogPort,
	code_index: &mut impl CodeIndexPort,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let (catalog, catalog_elapsed) = match current {
		Some(snapshot) => (snapshot.catalog.clone(), Duration::ZERO),
		None => {
			let catalog_timer = Instant::now();
			let catalog = source_catalog.load_catalog(&request)?;
			(catalog, catalog_timer.elapsed())
		}
	};
	let index_timer = Instant::now();
	let index = code_index.build_index(&catalog)?;
	let index_elapsed = index_timer.elapsed();
	let linkage = empty_linkage(&catalog, &index);
	let changes = empty_changes(&catalog, &index);
	let timings = timings(
		catalog_elapsed,
		&index,
		index_elapsed,
		Duration::ZERO,
		Duration::ZERO,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

fn build_linkage_snapshot(
	current: Option<&WorkspaceSnapshot>,
	linkage: &mut impl LinkagePort,
	change_overlay: &mut impl ChangeOverlayPort,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let current = current.ok_or_else(|| {
		WorkspaceFailure::new(
			WorkspaceResource::LinkageGraph,
			format!("{} requires an indexed workspace snapshot", request.label),
		)
	})?;
	let linkage_timer = Instant::now();
	let linkage = linkage.resolve_linkage(&current.index)?;
	let linkage_elapsed = linkage_timer.elapsed();
	let changes_timer = Instant::now();
	let changes =
		change_overlay.build_change_overlay(&current.catalog, &current.index, &linkage)?;
	let changes_elapsed = changes_timer.elapsed();
	let total = current.timings.source_catalog
		+ current.timings.code_index
		+ linkage_elapsed
		+ changes_elapsed;
	let timings = timings(
		current.timings.source_catalog,
		&current.index,
		current.timings.code_index,
		linkage_elapsed,
		changes_elapsed,
		total,
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog: current.catalog.clone(),
		index: current.index.clone(),
		linkage,
		changes,
		timings,
	})
}

fn build_catalog_snapshot(
	source_catalog: &mut impl SourceCatalogPort,
	request: WorkspaceRequest,
	generation: ResourceGeneration,
) -> WorkspaceResult<WorkspaceSnapshot> {
	let total_timer = Instant::now();
	let catalog_timer = Instant::now();
	let catalog = source_catalog.load_catalog(&request)?;
	let catalog_elapsed = catalog_timer.elapsed();
	let index = catalog_index(&catalog);
	let linkage = empty_linkage(&catalog, &index);
	let changes = empty_changes(&catalog, &index);
	let timings = timings(
		catalog_elapsed,
		&index,
		Duration::ZERO,
		Duration::ZERO,
		Duration::ZERO,
		total_timer.elapsed(),
	);
	Ok(WorkspaceSnapshot {
		generation,
		catalog,
		index,
		linkage,
		changes,
		timings,
	})
}

fn catalog_index(catalog: &SourceCatalog) -> CodeIndex {
	CodeIndex::from_fields(CodeIndexFields {
		generation: catalog.generation,
		catalog_generation: catalog.generation,
		identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
		sources: catalog
			.sources
			.iter()
			.enumerate()
			.map(|(idx, source)| {
				SourceFileRecord::from_fields(SourceFileRecordFields {
					id: source.id.clone(),
					uri: source.id.as_str().to_string(),
					source_root: idx,
					path: source.display_name.clone(),
					rel_path: source.display_name.clone(),
					anchor: source.display_name.clone(),
					language: source.language.clone().unwrap_or_default(),
					text: String::new(),
				})
			})
			.collect(),
		symbols: Vec::new(),
		references: Vec::new(),
		timings: Default::default(),
	})
}

fn empty_linkage(catalog: &SourceCatalog, index: &CodeIndex) -> LinkageGraph {
	LinkageGraph::new(catalog.generation, index.generation, 0, 0)
}

fn empty_changes(catalog: &SourceCatalog, index: &CodeIndex) -> ChangeOverlay {
	ChangeOverlay::new(
		catalog.generation,
		catalog.generation,
		index.generation,
		Vec::new(),
	)
}

fn timings(
	source_catalog: Duration,
	index: &CodeIndex,
	code_index: Duration,
	linkage: Duration,
	change_overlay: Duration,
	total: Duration,
) -> WorkspaceTimings {
	WorkspaceTimings {
		source_catalog,
		extract_sources: index.timings.extract_sources,
		semantic_index: index.timings.semantic_index,
		code_index,
		linkage,
		change_overlay,
		total,
	}
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::rc::Rc;

	use super::*;
	use crate::snapshot::{
		ChangeOverlay, CodeIndex, LinkageGraph, SourceCatalog, SourceUnit, SymbolId, SymbolRecord,
		WorkspaceFailure, WorkspaceResource,
	};

	#[derive(Default)]
	struct FakeState {
		log: Vec<String>,
		catalog_generation: u64,
		source_name: String,
		index_failure: Option<WorkspaceFailure>,
	}

	type SharedState = Rc<RefCell<FakeState>>;

	#[derive(Clone)]
	struct FakeSourceCatalog {
		state: SharedState,
	}

	impl SourceCatalogPort for FakeSourceCatalog {
		fn load_catalog(&mut self, request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog> {
			let mut state = self.state.borrow_mut();
			state.log.push(format!("catalog:{}", request.label));
			Ok(SourceCatalog::new(
				ResourceGeneration::new(state.catalog_generation),
				vec![SourceUnit::new("source:main", state.source_name.clone())],
			))
		}
	}

	#[derive(Clone)]
	struct FakeCodeIndex {
		state: SharedState,
	}

	impl CodeIndexPort for FakeCodeIndex {
		fn build_index(&mut self, catalog: &SourceCatalog) -> WorkspaceResult<CodeIndex> {
			let mut state = self.state.borrow_mut();
			state
				.log
				.push(format!("index:catalog@{}", catalog.generation.value()));
			if let Some(failure) = &state.index_failure {
				return Err(failure.clone());
			}
			let source = catalog.sources[0].id.clone();
			Ok(CodeIndex::new(
				ResourceGeneration::new(20),
				catalog.generation,
				vec![SymbolRecord::new("symbol:main", source, "main", "function")],
			))
		}
	}

	#[derive(Clone)]
	struct FakeLinkage {
		state: SharedState,
	}

	impl LinkagePort for FakeLinkage {
		fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph> {
			self.state
				.borrow_mut()
				.log
				.push(format!("linkage:index@{}", index.generation.value()));
			Ok(LinkageGraph::new(
				ResourceGeneration::new(30),
				index.generation,
				3,
				1,
			))
		}
	}

	#[derive(Clone)]
	struct FakeChangeOverlay {
		state: SharedState,
	}

	impl ChangeOverlayPort for FakeChangeOverlay {
		fn build_change_overlay(
			&mut self,
			catalog: &SourceCatalog,
			index: &CodeIndex,
			linkage: &LinkageGraph,
		) -> WorkspaceResult<ChangeOverlay> {
			self.state.borrow_mut().log.push(format!(
				"changes:catalog@{}:index@{}:linkage@{}",
				catalog.generation.value(),
				index.generation.value(),
				linkage.generation.value()
			));
			Ok(ChangeOverlay::new(
				ResourceGeneration::new(40),
				catalog.generation,
				index.generation,
				vec![SymbolId::new("symbol:main")],
			))
		}
	}

	struct Fixture {
		state: SharedState,
	}

	impl Fixture {
		fn new() -> Self {
			Self {
				state: Rc::new(RefCell::new(FakeState {
					catalog_generation: 10,
					source_name: "src/main.rs".to_string(),
					..FakeState::default()
				})),
			}
		}

		fn session(
			&self,
		) -> WorkspaceSnapshotRefresh<
			FakeSourceCatalog,
			FakeCodeIndex,
			FakeLinkage,
			FakeChangeOverlay,
		> {
			WorkspaceSnapshotRefresh::new(
				FakeSourceCatalog {
					state: Rc::clone(&self.state),
				},
				FakeCodeIndex {
					state: Rc::clone(&self.state),
				},
				FakeLinkage {
					state: Rc::clone(&self.state),
				},
				FakeChangeOverlay {
					state: Rc::clone(&self.state),
				},
			)
		}

		fn log(&self) -> Vec<String> {
			self.state.borrow().log.clone()
		}

		fn set_index_failure(&self, message: &str) {
			self.state.borrow_mut().index_failure =
				Some(WorkspaceFailure::new(WorkspaceResource::CodeIndex, message));
		}

		fn set_catalog(&self, generation: u64, source_name: &str) {
			let mut state = self.state.borrow_mut();
			state.catalog_generation = generation;
			state.source_name = source_name.to_string();
		}
	}

	#[test]
	fn refresh_builds_resources_in_semantic_order() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		let transition = session.refresh(WorkspaceRequest::new("repo"));

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(1)
			}
		);
		assert_eq!(
			fixture.log(),
			vec![
				"catalog:repo",
				"index:catalog@10",
				"linkage:index@20",
				"changes:catalog@10:index@20:linkage@30",
			]
		);
	}

	#[test]
	fn load_catalog_publishes_file_tree_without_indexing() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		let transition = session.load_catalog(WorkspaceRequest::new("catalog"));
		let snapshot = session.snapshot().expect("catalog snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(1)
			}
		);
		assert_eq!(fixture.log(), vec!["catalog:catalog"]);
		assert_eq!(snapshot.index.sources.len(), 1);
		assert_eq!(snapshot.index.sources[0].rel_path, "src/main.rs");
		assert!(snapshot.index.symbols.is_empty());
		assert!(snapshot.index.references.is_empty());
		assert_eq!(snapshot.linkage.resolved_refs, 0);
	}

	#[test]
	fn load_index_publishes_symbols_without_linkage() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		let transition = session.load_index(WorkspaceRequest::new("index"));
		let snapshot = session.snapshot().expect("index snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(1)
			}
		);
		assert_eq!(fixture.log(), vec!["catalog:index", "index:catalog@10"]);
		assert_eq!(snapshot.index.symbols.len(), 1);
		assert_eq!(snapshot.linkage.resolved_refs, 0);
		assert!(snapshot.changes.changed_symbols.is_empty());
	}

	#[test]
	fn load_index_reuses_published_catalog_snapshot() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		session.load_catalog(WorkspaceRequest::new("catalog"));
		let transition = session.load_index(WorkspaceRequest::new("index").reuse_current_catalog());
		let snapshot = session.snapshot().expect("index snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(2)
			}
		);
		assert_eq!(fixture.log(), vec!["catalog:catalog", "index:catalog@10"]);
		assert_eq!(snapshot.catalog.generation, ResourceGeneration::new(10));
		assert_eq!(
			snapshot.index.catalog_generation,
			ResourceGeneration::new(10)
		);
		assert_eq!(snapshot.timings.source_catalog, Duration::ZERO);
	}

	#[test]
	fn load_index_refreshes_catalog_by_default() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		session.load_catalog(WorkspaceRequest::new("catalog"));
		fixture.set_catalog(11, "src/other.rs");
		let transition = session.load_index(WorkspaceRequest::new("index"));
		let snapshot = session.snapshot().expect("index snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(2)
			}
		);
		assert_eq!(
			fixture.log(),
			vec!["catalog:catalog", "catalog:index", "index:catalog@11"]
		);
		assert_eq!(snapshot.catalog.generation, ResourceGeneration::new(11));
		assert_eq!(
			snapshot.index.catalog_generation,
			ResourceGeneration::new(11)
		);
	}

	#[test]
	fn replacing_snapshot_advances_next_generation() {
		let fixture = Fixture::new();
		let mut producer = fixture.session();

		producer.load_catalog(WorkspaceRequest::new("catalog"));
		let seed = producer.snapshot_arc().expect("catalog snapshot");
		let mut consumer = fixture.session();
		consumer.replace_snapshot_arc(seed);
		let transition =
			consumer.load_index(WorkspaceRequest::new("index").reuse_current_catalog());

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(2)
			}
		);
	}

	#[test]
	fn resolve_linkage_reuses_published_index_snapshot() {
		let fixture = Fixture::new();
		let mut session = fixture.session();

		session.load_index(WorkspaceRequest::new("index"));
		let transition = session.resolve_linkage(WorkspaceRequest::new("linkage"));
		let snapshot = session.snapshot().expect("linked snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(2)
			}
		);
		assert_eq!(
			fixture.log(),
			vec![
				"catalog:index",
				"index:catalog@10",
				"linkage:index@20",
				"changes:catalog@10:index@20:linkage@30",
			]
		);
		assert_eq!(snapshot.catalog.generation, ResourceGeneration::new(10));
		assert_eq!(snapshot.index.generation, ResourceGeneration::new(20));
		assert_eq!(snapshot.linkage.resolved_refs, 3);
		assert_eq!(
			snapshot.changes.changed_symbols,
			vec![SymbolId::new("symbol:main")]
		);
	}

	#[test]
	fn failure_does_not_publish_partial_workspace_snapshot() {
		let fixture = Fixture::new();
		let mut session = fixture.session();
		session.refresh(WorkspaceRequest::new("repo"));
		fixture.set_index_failure("cannot index");

		let transition = session.refresh(WorkspaceRequest::new("repo"));

		assert_eq!(
			transition,
			WorkspaceTransition::Failed {
				failure: WorkspaceFailure::new(WorkspaceResource::CodeIndex, "cannot index"),
				preserved_generation: Some(ResourceGeneration::new(1)),
			}
		);
		assert_eq!(
			session.snapshot().map(|snapshot| snapshot.generation),
			Some(ResourceGeneration::new(1))
		);
		assert_eq!(
			session.last_failure(),
			Some(&WorkspaceFailure::new(
				WorkspaceResource::CodeIndex,
				"cannot index"
			))
		);
	}

	#[test]
	fn successful_refresh_swaps_the_complete_workspace_snapshot() {
		let fixture = Fixture::new();
		let mut session = fixture.session();
		session.refresh(WorkspaceRequest::new("repo"));
		fixture.set_catalog(11, "src/lib.rs");

		let transition = session.refresh(WorkspaceRequest::new("repo"));
		let snapshot = session.snapshot().expect("ready snapshot");

		assert_eq!(
			transition,
			WorkspaceTransition::Ready {
				generation: ResourceGeneration::new(2)
			}
		);
		assert_eq!(snapshot.generation, ResourceGeneration::new(2));
		assert_eq!(snapshot.catalog.generation, ResourceGeneration::new(11));
		assert_eq!(snapshot.catalog.sources[0].display_name, "src/lib.rs");
	}
}
