use super::contracts::{
	ChangeOverlayPort, CodeIndexPort, LinkagePort, RuleDiagnosticsPort, SourceCatalogPort,
};
use super::model::{
	ResourceGeneration, WorkspaceRequest, WorkspaceResult, WorkspaceSnapshot, WorkspaceTransition,
};

pub struct WorkspaceSession<Sources, Index, Linkage, Changes, Diagnostics> {
	source_catalog: Sources,
	code_index: Index,
	linkage: Linkage,
	change_overlay: Changes,
	rule_diagnostics: Diagnostics,
	next_generation: u64,
	snapshot: Option<WorkspaceSnapshot>,
	last_failure: Option<super::model::WorkspaceFailure>,
}

impl<Sources, Index, Linkage, Changes, Diagnostics>
	WorkspaceSession<Sources, Index, Linkage, Changes, Diagnostics>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
	Diagnostics: RuleDiagnosticsPort,
{
	pub fn new(
		source_catalog: Sources,
		code_index: Index,
		linkage: Linkage,
		change_overlay: Changes,
		rule_diagnostics: Diagnostics,
	) -> Self {
		Self {
			source_catalog,
			code_index,
			linkage,
			change_overlay,
			rule_diagnostics,
			next_generation: 1,
			snapshot: None,
			last_failure: None,
		}
	}

	pub fn refresh(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		match self.build_snapshot(request) {
			Ok(snapshot) => {
				let generation = snapshot.generation;
				self.snapshot = Some(snapshot);
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

	pub fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.snapshot.as_ref()
	}

	pub fn last_failure(&self) -> Option<&super::model::WorkspaceFailure> {
		self.last_failure.as_ref()
	}

	fn build_snapshot(&mut self, request: WorkspaceRequest) -> WorkspaceResult<WorkspaceSnapshot> {
		let catalog = self.source_catalog.load_catalog(&request)?;
		let index = self.code_index.build_index(&catalog)?;
		let linkage = self.linkage.resolve_linkage(&index)?;
		let changes = self
			.change_overlay
			.build_change_overlay(&catalog, &index, &linkage)?;
		let diagnostics = self
			.rule_diagnostics
			.collect_rule_diagnostics(&index, &linkage)?;
		let generation = self.allocate_generation();
		Ok(WorkspaceSnapshot {
			generation,
			catalog,
			index,
			linkage,
			changes,
			diagnostics,
		})
	}

	fn allocate_generation(&mut self) -> ResourceGeneration {
		let generation = ResourceGeneration::new(self.next_generation);
		self.next_generation += 1;
		generation
	}
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::rc::Rc;

	use super::*;
	use crate::workspace::session::{
		ChangeOverlay, CodeIndex, LinkageGraph, RuleDiagnostics, SourceCatalog, SourceUnit,
		SymbolId, SymbolRecord, WorkspaceFailure, WorkspaceResource,
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

	#[derive(Clone)]
	struct FakeRuleDiagnostics {
		state: SharedState,
	}

	impl RuleDiagnosticsPort for FakeRuleDiagnostics {
		fn collect_rule_diagnostics(
			&mut self,
			index: &CodeIndex,
			linkage: &LinkageGraph,
		) -> WorkspaceResult<RuleDiagnostics> {
			self.state.borrow_mut().log.push(format!(
				"diagnostics:index@{}:linkage@{}",
				index.generation.value(),
				linkage.generation.value()
			));
			Ok(RuleDiagnostics::new(
				ResourceGeneration::new(50),
				index.generation,
				0,
				2,
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
		) -> WorkspaceSession<
			FakeSourceCatalog,
			FakeCodeIndex,
			FakeLinkage,
			FakeChangeOverlay,
			FakeRuleDiagnostics,
		> {
			WorkspaceSession::new(
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
				FakeRuleDiagnostics {
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
				"diagnostics:index@20:linkage@30",
			]
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
