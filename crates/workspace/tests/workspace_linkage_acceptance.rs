use std::path::{Path, PathBuf};

use code_moniker_workspace::extract::JavaExtractionPipeline;
use code_moniker_workspace::snapshot::{ReferenceRecord, WorkspaceRequest, WorkspaceSnapshot};
use code_moniker_workspace::{LocalWorkspaceFacade, LocalWorkspaceOptions};

fn fixture_path(path: impl AsRef<Path>) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures")
		.join(path)
}

fn load_workspace(path: impl AsRef<Path>) -> WorkspaceSnapshot {
	load_workspace_with_options(LocalWorkspaceOptions::new(vec![fixture_path(path)], None))
}

fn load_workspace_with_options(options: LocalWorkspaceOptions) -> WorkspaceSnapshot {
	let mut workspace = LocalWorkspaceFacade::local(options);
	let transition = workspace.refresh(WorkspaceRequest::new("linkage-acceptance"));
	assert!(
		matches!(
			transition,
			code_moniker_workspace::snapshot::WorkspaceTransition::Ready { .. }
		),
		"workspace refresh failed: {transition:?}"
	);
	workspace
		.snapshot()
		.expect("ready workspace should expose a snapshot")
		.clone()
}

#[test]
fn rust_multiproject_links_public_cross_crate_symbols() {
	let snapshot = load_workspace("projects/rust/multiproject");

	assert_no_unresolved(&snapshot);
	assert_cross_crate_links(&snapshot);
	assert_local_rust_links(&snapshot);
}

#[test]
fn java_sdk_multiproject_classifies_platform_refs_as_external() {
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(vec![fixture_path("projects/java/multiprojet")], None)
			.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

	assert_eq!(snapshot.linkage.external_refs, 89);
	assert_eq!(
		snapshot.linkage.unresolved_refs,
		0,
		"unexpected unresolved references:\n{}",
		unresolved_report(&snapshot)
	);
	assert_external_reference(
		&snapshot,
		"method_call",
		"external_pkg:java/path:lang/path:System/method:println",
	);
	assert_external_reference(
		&snapshot,
		"method_call",
		"external_pkg:java/path:lang/path:String/method:trim",
	);
}

fn assert_no_unresolved(snapshot: &WorkspaceSnapshot) {
	assert_eq!(
		snapshot.linkage.unresolved_refs,
		0,
		"unexpected unresolved references:\n{}",
		unresolved_report(&snapshot)
	);
}

fn assert_cross_crate_links(snapshot: &WorkspaceSnapshot) {
	assert_linked_to(
		snapshot,
		"imports_symbol",
		"external_pkg:common_model/path:CustomerId",
		"dir:common-model/dir:src/module:lib/struct:CustomerId",
	);
	assert_linked_to(
		snapshot,
		"imports_symbol",
		"external_pkg:common_model/path:risk/path:RiskPolicy",
		"dir:common-model/dir:src/module:lib/module:risk/struct:RiskPolicy",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"external_pkg:common_model/path:CustomerId",
		"dir:common-model/dir:src/module:lib/struct:CustomerId",
	);
	assert_linked_to(
		snapshot,
		"implements",
		"external_pkg:common_model/path:Auditable",
		"dir:common-model/dir:src/module:lib/trait:Auditable",
	);
	assert_linked_to(
		snapshot,
		"calls",
		"external_pkg:common_model/path:normalize_customer",
		"dir:common-model/dir:src/module:lib/fn:normalize_customer(customer:CustomerId)",
	);
	assert_linked_to(
		snapshot,
		"calls",
		"external_pkg:common_model/path:risk/path:assess",
		"dir:common-model/dir:src/module:lib/module:risk/fn:assess(customer:&CustomerId)",
	);
}

fn assert_local_rust_links(snapshot: &WorkspaceSnapshot) {
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:lib/path:errors/path:LocalError",
		"dir:order-service/dir:src/module:lib/module:errors/struct:LocalError",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:lib/module:types/path:WildcardType",
		"dir:order-service/dir:src/module:types/struct:WildcardType",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:types/path:WildcardType",
		"dir:order-service/dir:src/module:types/struct:WildcardType",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"external_pkg:common_model/path:Region/path:Eu",
		"dir:common-model/dir:src/module:lib/enum:Region/enum_constant:Eu",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:feature/path:Region",
		"dir:common-model/dir:src/module:lib/enum:Region",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"dir:order-service/dir:src/module:feature/path:Region/path:Eu",
		"dir:common-model/dir:src/module:lib/enum:Region/enum_constant:Eu",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:feature/path:Lang",
		"dir:common-model/dir:src/module:lib/enum:Lang",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"dir:order-service/dir:src/module:feature/path:Lang/path:Ts",
		"dir:common-model/dir:src/module:lib/enum:Lang/enum_constant:Ts",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"dir:order-service/dir:src/module:lib/path:constants/path:DEFAULT_REGION",
		"dir:order-service/dir:src/module:lib/module:constants/path:DEFAULT_REGION",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"external_pkg:common_model/path:CustomerId/path:tag",
		"dir:common-model/dir:src/module:lib/struct:CustomerId/method:tag(&CustomerId)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"dir:order-service/dir:src/module:lib/struct:LocalGraph/method:add_def",
		"dir:order-service/dir:src/module:lib/struct:LocalGraph/method:add_def",
	);
	assert_linked_to(
		snapshot,
		"imports_symbol",
		"dir:order-service/dir:src/dir:module_group/module:nested",
		"dir:order-service/dir:src/dir:module_group/dir:nested/module:mod",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:lib/fn:local_report_shape()/struct:Summary",
		"dir:order-service/dir:src/module:lib/fn:local_report_shape()/struct:Summary",
	);
	assert_linked_to(
		snapshot,
		"imports_module",
		"dir:order-service/dir:src/module:lib/module:types/path:ImportedState",
		"dir:order-service/dir:src/module:types/enum:ImportedState",
	);
}

fn assert_linked_to(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	reference_target: &str,
	symbol_identity: &str,
) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	let target_identities = linked_symbol_identities(snapshot, reference);
	assert!(
		target_identities
			.iter()
			.any(|identity| identity.contains(symbol_identity)),
		"reference `{}` was linked to [{}], expected target containing `{}`",
		reference.target_identity,
		target_identities.join(", "),
		symbol_identity
	);
}

fn assert_external_reference(snapshot: &WorkspaceSnapshot, kind: &str, reference_target: &str) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	let linked = snapshot
		.linkage
		.resolved
		.iter()
		.any(|edge| edge.reference.as_str() == reference.id.as_str());
	let unresolved = snapshot
		.linkage
		.unresolved
		.iter()
		.any(|item| item.reference.as_str() == reference.id.as_str());
	assert!(
		!linked && !unresolved,
		"reference `{}` should be classified external, linked={linked}, unresolved={unresolved}",
		reference.target_identity
	);
}

fn find_reference<'a>(
	snapshot: &'a WorkspaceSnapshot,
	kind: &str,
	target_identity: &str,
) -> Option<&'a ReferenceRecord> {
	snapshot.index.references.iter().find(|reference| {
		reference.kind == kind && reference.target_identity.contains(target_identity)
	})
}

fn linked_symbol_identities(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceRecord,
) -> Vec<String> {
	snapshot
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.reference.as_str() == reference.id.as_str())
		.filter_map(|edge| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id.as_str() == edge.target.as_str())
		})
		.map(|symbol| symbol.identity.clone())
		.collect()
}

fn unresolved_report(snapshot: &WorkspaceSnapshot) -> String {
	snapshot
		.linkage
		.unresolved
		.iter()
		.map(|unresolved| {
			let reference = snapshot
				.index
				.references
				.iter()
				.find(|reference| reference.id.as_str() == unresolved.reference.as_str());
			let meta = reference.map_or_else(
				|| "missing reference".to_string(),
				|reference| {
					format!(
						"kind={} confidence={:?} call={:?}/{:?}",
						reference.kind,
						reference.confidence,
						reference.call_name,
						reference.call_arity
					)
				},
			);
			format!("{} ({meta})", unresolved.target_identity)
		})
		.collect::<Vec<_>>()
		.join("\n")
}
