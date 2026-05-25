use super::model::{
	ChangeOverlay, CodeIndex, LinkageGraph, RuleDiagnostics, SourceCatalog, WorkspaceRequest,
	WorkspaceResult,
};

pub trait SourceCatalogPort {
	fn load_catalog(&mut self, request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog>;
}

pub trait CodeIndexPort {
	fn build_index(&mut self, catalog: &SourceCatalog) -> WorkspaceResult<CodeIndex>;
}

pub trait LinkagePort {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph>;
}

pub trait ChangeOverlayPort {
	fn build_change_overlay(
		&mut self,
		catalog: &SourceCatalog,
		index: &CodeIndex,
		linkage: &LinkageGraph,
	) -> WorkspaceResult<ChangeOverlay>;
}

pub trait RuleDiagnosticsPort {
	fn collect_rule_diagnostics(
		&mut self,
		index: &CodeIndex,
		linkage: &LinkageGraph,
	) -> WorkspaceResult<RuleDiagnostics>;
}
