use std::path::{Path, PathBuf};

use code_moniker_workspace::audit::{AuditOptions, resolution_audit};
use code_moniker_workspace::snapshot::{
	DynamicReason, ExternalReferenceOrigin, ReferenceRecord, UnresolvedReason, WorkspaceRequest,
	WorkspaceSnapshot,
};
use code_moniker_workspace::{LocalWorkspaceOptions, LocalWorkspaceRegistry};

fn fixture_path(path: impl AsRef<Path>) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures")
		.join(path)
}

fn load_workspace(path: impl AsRef<Path>) -> WorkspaceSnapshot {
	load_workspace_with_options(LocalWorkspaceOptions::new(vec![fixture_path(path)], None))
}

fn load_workspace_with_options(options: LocalWorkspaceOptions) -> WorkspaceSnapshot {
	let mut workspace = LocalWorkspaceRegistry::local(options);
	let transition = workspace
		.commands()
		.refresh(WorkspaceRequest::new("linkage-acceptance"));
	assert!(
		matches!(
			transition,
			code_moniker_workspace::snapshot::WorkspaceTransition::Ready { .. }
		),
		"workspace refresh failed: {transition:?}"
	);
	let snapshot = workspace
		.queries()
		.snapshot()
		.expect("ready workspace should expose a snapshot")
		.clone();
	assert_external_origins_match_target_regime(&snapshot);
	snapshot
}

fn assert_external_origins_match_target_regime(snapshot: &WorkspaceSnapshot) {
	for reference in &snapshot.linkage.external {
		let sdk_target = reference.target_identity.contains("/sdk:");
		match reference.origin {
			ExternalReferenceOrigin::Sdk => assert!(
				sdk_target,
				"SDK provenance requires an sdk:<lang> target, got {}",
				reference.target_identity
			),
			ExternalReferenceOrigin::Dependency
			| ExternalReferenceOrigin::Injected
			| ExternalReferenceOrigin::UnknownExternal => assert!(
				!sdk_target,
				"target {} must use SDK provenance instead of {:?}",
				reference.target_identity, reference.origin
			),
		}
	}
}

#[test]
fn rust_multiproject_links_public_cross_crate_symbols() {
	let snapshot = load_workspace("projects/rust/multiproject");

	assert_no_unresolved(&snapshot);
	assert_cross_crate_links(&snapshot);
	assert_local_rust_links(&snapshot);
}

#[test]
fn rust_binary_links_its_same_package_library_crate() {
	let snapshot = load_workspace("projects/rust/bin-lib-package");

	assert_linked_once_to(
		&snapshot,
		"calls",
		"external_pkg:bin_lib_runtime/path:run",
		"module:runtime/fn:run()",
	);
	let invalid = find_reference(&snapshot, "calls", "external_pkg:bin_lib_package/path:run")
		.expect("invalid package-name crate root should still be observable");
	assert!(
		linked_symbol_identities(&snapshot, invalid).is_empty(),
		"[lib].name replaces the default package-derived Rust crate root"
	);
}

#[test]
fn rust_bin_lib_linkage_is_anchored_to_the_manifest_not_a_src_segment() {
	let snapshot = load_workspace("projects/rust/nested-bin-lib");

	assert_linked_once_to(
		&snapshot,
		"calls",
		"external_pkg:nested_runtime/path:run",
		"module:runtime/fn:run()",
	);
}

#[test]
fn rust_type_references_ignore_value_namespace_homonyms() {
	let snapshot = load_workspace("projects/rust/type-value-homonym");

	assert_linked_once_to(
		&snapshot,
		"uses_type",
		"module:config/path:Config",
		"module:config/struct:Config",
	);
}

#[test]
fn global_name_matches_do_not_cross_language_boundaries() {
	let snapshot = load_workspace("projects/mixed-language");
	let source_identity = "module:caller/struct:Caller/method:call_language_homonyms()";
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));

	for call_name in ["tsOnly", "javaOnly", "goOnly"] {
		let reference = snapshot
			.index
			.references
			.iter()
			.find(|reference| {
				reference.kind == "method_call"
					&& reference.source_symbol == source.id
					&& reference.call_name.as_deref() == Some(call_name)
			})
			.unwrap_or_else(|| panic!("missing `{call_name}` call from `{source_identity}`"));
		assert!(
			linked_symbol_identities(&snapshot, reference).is_empty(),
			"`{call_name}` must not resolve to a definition from another language"
		);
		let unresolved = snapshot
			.linkage
			.unresolved
			.iter()
			.find(|item| item.reference == reference.id)
			.unwrap_or_else(|| panic!("`{call_name}` should remain explicitly unresolved"));
		assert_eq!(unresolved.reason, UnresolvedReason::NoCandidate);
	}
}

#[test]
fn csharp_sdk_links_unique_methods_and_classifies_open_receivers() {
	let snapshot = load_workspace("projects/cs/resolution");

	assert_call_resolves_only_to(
		&snapshot,
		"module:Program/class:Program/method:Run(worker:Worker,runtime:object)",
		"method_call",
		"Format",
		1,
		"module:Worker/class:Worker/method:Format(value:string)",
	);
	assert_dynamic_reason(
		&snapshot,
		"module:Program/class:Program/method:Run(worker:Worker,runtime:object)",
		"method_call",
		Some("MissingRuntimeMember"),
		DynamicReason::InsufficientLocalFacts,
	);
}

#[test]
fn c_sdk_links_program_wide_functions_and_local_headers() {
	let snapshot = load_workspace("projects/c/resolution");

	assert_call_resolves_only_to(
		&snapshot,
		"module:main/func:run()",
		"calls",
		"add",
		2,
		"module:math/func:add(left:int,right:int)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:main/func:run()",
		"calls",
		"open",
		1,
		"module:math/func:open(flags:int)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:main/func:run()",
		"calls",
		"DOUBLE",
		1,
		"module:math.h/macro:DOUBLE(value)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:main/func:run()",
		"calls",
		"twice",
		1,
		"module:math.h/func:twice(value:int)",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:main/func:run()",
		"module:main/var:MATH_VERSION",
		"module:math.h/const:MATH_VERSION",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:fragment/func:included_value()",
		"type:MathRecord/field:value",
		"struct:MathRecord/field:value",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:fragment/func:included_value()",
		"type:MathBufferPtr/field:len",
		"struct:MathBuffer/field:len",
	);
	assert_c_preprocessor_linkage(&snapshot);
	assert_call_resolves_only_to(
		&snapshot,
		"module:fragment/func:included_value()",
		"calls",
		"DOUBLE",
		1,
		"module:math.h/macro:DOUBLE(value)",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:fragment/func:included_value()",
		"module:fragment/var:MATH_VERSION",
		"module:math.h/const:MATH_VERSION",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"imports_module",
		"module:main",
		"module:math.h",
		"module:math.h",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"imports_module",
		"module:main",
		"dir:project/module:config.h",
		"dir:project/module:config.h",
	);
	assert_external_reference(
		&snapshot,
		"imports_module",
		"external_pkg:vendor/path:missing",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:main/func:run()",
		"sdk:c/path:libc/func:assert",
	);
	assert_linked_to(
		&snapshot,
		"uses_type",
		"type:API_IMPORT",
		"module:math.h/const:API_IMPORT",
	);
	assert_named_call_unresolved(&snapshot, "module:main/func:run()", "hidden", 1);
	assert_call_resolves_only_to(
		&snapshot,
		"module:main/func:run()",
		"calls",
		"handler",
		0,
		"module:main/func:run()/local:handler",
	);
}

fn assert_c_preprocessor_linkage(snapshot: &WorkspaceSnapshot) {
	let injected = find_reference(snapshot, "reads", "module:fragment/var:injected_value")
		.expect("macro-introduced local read");
	assert!(snapshot.linkage.dynamic.iter().any(|dynamic| {
		dynamic.reference == injected.id && dynamic.reason == DynamicReason::PreprocessorExpansion
	}));
	assert_linked_once_from_symbol(
		snapshot,
		"reads",
		"module:fragment/func:included_value()",
		"module:fragment/var:MATH_MODE_FAST",
		"enum:MathMode/enum_constant:MATH_MODE_FAST",
	);
	for (target, label) in [
		("module:fragment/var:FAST", "token-pasted"),
		("module:fragment/var:value", "structural"),
	] {
		let reference = find_reference(snapshot, "reads", target)
			.unwrap_or_else(|| panic!("missing {label} macro argument reference"));
		assert!(snapshot.linkage.dynamic.iter().any(|dynamic| {
			dynamic.reference == reference.id
				&& dynamic.reason == DynamicReason::PreprocessorExpansion
		}));
	}
	let type_macro = find_reference(snapshot, "uses_type", "type:TYPE_MACRO")
		.expect("macro invocation used as a generated C type");
	assert!(snapshot.linkage.dynamic.iter().any(|dynamic| {
		dynamic.reference == type_macro.id && dynamic.reason == DynamicReason::PreprocessorExpansion
	}));
	assert_named_call_unresolved(
		snapshot,
		"module:fragment/func:included_value()",
		"DOUBLE",
		2,
	);
	assert_dynamic_reason(
		snapshot,
		"module:fragment/func:included_value()",
		"calls",
		Some("VARIADIC"),
		DynamicReason::PreprocessorExpansion,
	);
	for (name, context) in [
		("ordinary_typo", "outside macro arguments"),
		("mixed_typo", "inside a non-structural macro argument"),
	] {
		let target = format!("module:fragment/var:{name}");
		let reference = find_reference(snapshot, "reads", &target)
			.unwrap_or_else(|| panic!("missing unresolved read {context}"));
		assert!(
			snapshot
				.linkage
				.unresolved
				.iter()
				.any(|unresolved| unresolved.reference == reference.id),
			"reads {context} must remain honestly unresolved"
		);
		assert!(
			snapshot
				.linkage
				.dynamic
				.iter()
				.all(|dynamic| dynamic.reference != reference.id),
			"reads {context} must not become preprocessor dynamics"
		);
	}
}

#[test]
fn c_pgxs_build_provenance_classifies_unindexed_postgresql_references() {
	let snapshot = load_workspace("projects/c/pgxs");

	assert_external_reference(
		&snapshot,
		"imports_module",
		"external_pkg:postgresql/path:postgres",
	);
	let local_generated = find_reference(&snapshot, "imports_module", "module:local_generated.h")
		.expect("missing local generated include reference");
	assert!(
		snapshot
			.linkage
			.unresolved
			.iter()
			.any(|unresolved| unresolved.reference == local_generated.id),
		"unknown quoted includes must remain unresolved even in PGXS projects"
	);
	assert!(
		!reference_is_external(&snapshot, local_generated),
		"PGXS must not claim arbitrary local quoted includes"
	);
	assert_dynamic_reason(
		&snapshot,
		"module:extension/func:run()",
		"calls",
		Some("RequestAddinShmemSpace"),
		DynamicReason::ExternalDependencyUnindexed,
	);
	assert_dynamic_reason(
		&snapshot,
		"module:extension/func:run()",
		"uses_type",
		None,
		DynamicReason::ExternalDependencyUnindexed,
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:extension/func:run()",
		"calls",
		"local_helper",
		0,
		"module:extension/func:local_helper()",
	);
}

#[test]
fn sql_sdk_links_schema_qualified_overloads_and_classifies_open_calls() {
	let snapshot = load_workspace("projects/sql/resolution");

	assert_linked_once_from_symbol(
		&snapshot,
		"uses_type",
		"function:accept_state(value:public.order_state)",
		"schema:public/type:order_state",
		"schema:public/type:order_state",
	);

	assert_call_resolves_only_to(
		&snapshot,
		"module:usage",
		"calls",
		"finish",
		0,
		"module:definitions/schema:public/function:finish()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:usage",
		"calls",
		"pick",
		2,
		"module:definitions/schema:public/function:pick(left_value:int4,right_value:int4)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:usage",
		"calls",
		"refresh",
		0,
		"module:definitions/schema:public/procedure:refresh()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:usage",
		"calls",
		"lowercase",
		0,
		"module:definitions/schema:public/function:lowercase()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:usage",
		"calls",
		"MixedCase",
		0,
		"module:definitions/schema:public/function:MixedCase()",
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:usage",
		"calls",
		"pick",
		1,
		&[
			"module:definitions/schema:public/function:pick(value:int4)",
			"module:definitions/schema:private/function:pick(value:int4)",
		],
	);
	assert_call_resolves_only_to(
		&snapshot,
		"function:call_choose_search_path(value:int4)",
		"calls",
		"choose",
		1,
		"schema:public/function:choose(value:int4)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"function:call_choose_int(value:int4)",
		"calls",
		"choose",
		1,
		"schema:public/function:choose(value:int4)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"function:call_choose_text(value:text)",
		"calls",
		"choose",
		1,
		"schema:public/function:choose(value:text)",
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"function:call_choose_unknown()",
		"calls",
		"choose",
		1,
		&[
			"schema:public/function:choose(value:int4)",
			"schema:public/function:choose(value:text)",
		],
	);
	assert_dynamic_reason(
		&snapshot,
		"module:usage",
		"calls",
		Some("mixedcase"),
		DynamicReason::ExternalDependencyUnindexed,
	);
	assert_dynamic_reason(
		&snapshot,
		"module:usage",
		"calls",
		Some("missing_runtime_function"),
		DynamicReason::ExternalDependencyUnindexed,
	);
}

#[test]
fn sql_relational_edges_link_across_source_documents() {
	let snapshot = load_workspace("projects/sql/resolution");

	assert_linked_once_from_symbol(
		&snapshot,
		"writes",
		"module:relation_usage",
		"schema:public/table:orders",
		"module:tables/schema:public/table:orders",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:relation_usage",
		"schema:public/table:open_orders",
		"module:views/schema:public/view:open_orders",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reads",
		"module:views/schema:public/view:open_orders",
		"schema:public/table:orders",
		"module:tables/schema:public/table:orders",
	);
	assert_linked_once_to(
		&snapshot,
		"references",
		"module:tables/schema:public/table:customers",
		"module:customers/schema:public/table:customers",
	);
	assert_linked_once_to(
		&snapshot,
		"references",
		"module:tables/schema:public/table:customers/column:id",
		"module:customers/schema:public/table:customers/column:id",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"references",
		"module:triggers/schema:public/table:orders/trigger:audit_orders",
		"module:triggers/schema:public/table:orders",
		"module:tables/schema:public/table:orders",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:triggers/schema:public/table:orders/trigger:audit_orders",
		"calls",
		"audit_order",
		0,
		"module:trigger_functions/schema:public/function:audit_order()",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"references",
		"module:triggers/schema:public/view:open_orders/trigger:route_open_orders",
		"module:triggers/schema:public/view:open_orders",
		"module:views/schema:public/view:open_orders",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:triggers/schema:public/view:open_orders/trigger:route_open_orders",
		"calls",
		"route_open_order",
		0,
		"module:trigger_functions/schema:public/function:route_open_order()",
	);
}

#[test]
fn rust_multiproject_canonicalizes_mod_rs_modules() {
	let snapshot = load_workspace("projects/rust/multiproject");

	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"dir:order-service/dir:src/module:module_group/module:nested",
		"dir:order-service/dir:src/module:module_group/module:nested",
	);
	assert_linked_once_from_symbol(
		&snapshot,
		"reexports",
		"dir:order-service/dir:src/module:lib",
		"dir:order-service/dir:src/module:module_group",
		"dir:order-service/dir:src/module:module_group",
	);
	assert_no_reference_containing(
		&snapshot,
		"reexports",
		"dir:order-service/dir:src/module:lib/module:module_group",
	);
	assert_no_symbol_containing(
		&snapshot,
		"dir:order-service/dir:src/dir:module_group/dir:nested/module:mod",
	);
	assert_symbol_count_containing(
		&snapshot,
		"dir:order-service/dir:src/module:module_group/module:nested",
		1,
	);
}

#[test]
fn rust_cross_crate_import_resolves_public_mod_rs_reexport() {
	let snapshot = load_workspace("projects/rust/reexport-mod-cross-crate");

	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"external_pkg:public_api/path:api/path:SharedModel",
		"dir:public-api/dir:src/module:api/module:model/struct:SharedModel",
	);
	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"external_pkg:public_api/path:api/path:AliasedModel",
		"dir:public-api/dir:src/module:api/module:model/struct:SharedModel",
	);
	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"external_pkg:public_api/path:api/path:DeepModel",
		"dir:public-api/dir:src/module:api/module:model/struct:DeepModel",
	);
	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"external_pkg:public_api/path:model_facade/path:NestedModel",
		"dir:inner-model/dir:src/module:lib/module:models/struct:NestedModel",
	);
	assert_linked_once_to(
		&snapshot,
		"imports_symbol",
		"external_pkg:public_api/path:facade/path:ExternalModel",
		"dir:inner-model/dir:src/module:lib/struct:ExternalModel",
	);
}

#[test]
fn rust_qualified_calls_do_not_match_unrelated_same_arity_callables() {
	let snapshot = load_workspace("projects/rust/qualified-call-collision");

	assert_call_resolves_only_to(
		&snapshot,
		"fn:uses_qualified_path_matches",
		"calls",
		"matches",
		2,
		"module:check/module:path/fn:matches(pattern:&Pattern,m:&Moniker)",
	);
}

#[test]
fn rust_sdk_method_calls_do_not_match_workspace_receiver_homonyms() {
	let snapshot = load_workspace("projects/rust/qualified-call-collision");
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| {
			symbol
				.identity
				.contains("fn:clone_sdk_path(path:&std::path::PathBuf)")
		})
		.expect("clone_sdk_path symbol");
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.source_symbol == source.id
				&& reference.kind == "method_call"
				&& reference.call_name.as_deref() == Some("clone")
		})
		.expect("PathBuf clone reference");
	assert!(
		linked_symbol_identities(&snapshot, reference).is_empty(),
		"`PathBuf::clone` must not resolve to `CloneCollision::clone`"
	);
	assert!(
		snapshot.linkage.external.iter().any(|external| {
			external.reference == reference.id && external.origin == ExternalReferenceOrigin::Sdk
		}),
		"`PathBuf::clone` should retain SDK provenance"
	);
	assert_call_resolves_only_to(
		&snapshot,
		"fn:clone_local(value:&CloneCollision)",
		"method_call",
		"clone",
		0,
		"struct:CloneCollision/method:clone()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"fn:clone_cross_file(value:&CrossFileClone)",
		"method_call",
		"clone",
		0,
		"module:cross_file/struct:CrossFileClone/method:clone()",
	);
	let derived_source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| {
			symbol
				.identity
				.contains("fn:clone_cross_file_derived(value:&DerivedClone)")
		})
		.expect("clone_cross_file_derived symbol");
	let derived_reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.source_symbol == derived_source.id
				&& reference.kind == "method_call"
				&& reference.call_name.as_deref() == Some("clone")
		})
		.expect("derived Clone reference");
	assert!(
		linked_symbol_identities(&snapshot, derived_reference).is_empty(),
		"derived cross-file Clone must not resolve to a workspace homonym"
	);
	assert!(
		snapshot.linkage.external.iter().any(|external| {
			external.reference == derived_reference.id
				&& external.origin == ExternalReferenceOrigin::Sdk
				&& external.target_identity.contains("sdk:rs")
		}),
		"derived cross-file Clone should fall back to the Rust SDK"
	);
}

#[test]
fn rust_facade_wildcard_reexports_forward_crate_references() {
	let snapshot = load_workspace("projects/rust/facade-crate");

	assert_call_resolves_only_to(
		&snapshot,
		"module:use_facade/test:spins()",
		"calls",
		"spin_widget",
		0,
		"dir:inner/dir:src/module:lib/fn:spin_widget()",
	);
}

#[test]
fn rust_facade_reexport_does_not_rival_the_canonical_definition() {
	let snapshot = load_workspace("projects/rust/facade-alias");

	assert_linked_once_from_symbol(
		&snapshot,
		"uses_type",
		"module:consumer/fn:build(_run:&CheckRun)",
		"CheckRun",
		"module:command/struct:CheckRun",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:consumer/fn:build(_run:&CheckRun)",
		"calls",
		"execute",
		0,
		"module:command/fn:execute()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:consumer/module:nested/fn:execute_from_parent()",
		"calls",
		"execute",
		0,
		"module:command/fn:execute()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:consumer/module:nested/fn:execute_from_wildcard()",
		"calls",
		"execute",
		0,
		"module:command/fn:execute()",
	);
}

#[test]
fn rust_qualified_calls_resolve_through_module_reexports() {
	let snapshot = load_workspace("projects/rust/reexport-qualified");

	assert_call_resolves_only_to(
		&snapshot,
		"module:app/fn:run()",
		"calls",
		"version",
		0,
		"module:store/module:engine/fn:version()",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:app/fn:run()",
		"external_pkg:unknown_vendored",
	);
}

#[test]
fn java_sdk_multiproject_links_spring_and_platform_refs() {
	let snapshot = load_workspace("projects/java/multiprojet");

	assert_no_unresolved(&snapshot);
	assert_java_platform_refs(&snapshot);
	assert_java_spring_refs(&snapshot);
	assert_java_generic_refs(&snapshot);
	assert_java_nested_type_refs(&snapshot);
	assert_java_external_fluent_refs(&snapshot);
	assert_java_switch_refs(&snapshot);
	assert_java_lombok_refs(&snapshot);
	assert_java_cross_project_interface_implementations(&snapshot);
}

#[test]
fn java_lombok_boundaries_do_not_invent_accessors() {
	let snapshot = load_workspace("projects/java/lombok-boundaries");

	assert_call_unresolved(
		&snapshot,
		"package:com/package:acme/package:lombokboundary/module:LombokDataBoundary/class:LombokDataBoundary/method:exercise()",
		"setCode",
		1,
	);
	assert_call_unresolved(
		&snapshot,
		"package:com/package:acme/package:lombokboundary/module:LombokDataBoundary/class:LombokDataBoundary/method:exercise()",
		"isReviewed",
		0,
	);
	assert_call_unresolved(
		&snapshot,
		"package:com/package:acme/package:lombokboundary/module:LombokValueBoundary/class:LombokValueBoundary/method:exercise()",
		"withCode",
		1,
	);
	assert_unresolved_reasons_recorded(&snapshot);
}

#[test]
fn java_declared_source_group_connects_manifest_less_modules() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:MainCaller/class:MainCaller/method:readLabel(SharedRecord)",
		"getLabel",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:getLabel()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:TestCaller/class:TestCaller/method:readLabel(SharedRecord)",
		"getLabel",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:getLabel()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:MainCaller/class:MainCaller/method:readDescription(SharedRecord)",
		"describe",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:describe()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:TestCaller/class:TestCaller/method:readDescription(SharedRecord)",
		"describe",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:describe()",
	);
}

#[test]
fn java_workspace_wildcard_imports_resolve_only_exported_members() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_linked_once_to(
		&snapshot,
		"uses_type",
		"package:caller/module:Widget/path:Widget",
		"package:exports/module:Widget/class:Widget",
	);
	assert_linked_once_to(
		&snapshot,
		"instantiates",
		"package:caller/module:Widget/path:Widget",
		"package:exports/module:Widget/class:Widget",
	);
	assert_linked_once_to(
		&snapshot,
		"calls",
		"module:WildcardCaller/class:WildcardCaller/method:decorate()",
		"package:exports/module:Tools/class:Tools/method:decorate()",
	);
	assert_named_call_unresolved(
		&snapshot,
		"package:caller/module:WildcardCaller/class:WildcardCaller/method:invalidStaticImport()",
		"instanceOnly",
		0,
	);
}

#[test]
fn java_declared_source_groups_block_cross_group_calls() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_blocked(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:outsider/module:OutsiderCaller/class:OutsiderCaller/method:readLabel(SharedRecord)",
		"getLabel",
		0,
	);
	assert_call_blocked(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:outsider/module:OutsiderCaller/class:OutsiderCaller/method:readDescription(SharedRecord)",
		"describe",
		0,
	);
}

#[test]
fn java_declared_source_group_maps_non_standard_main_and_test_srcsets() {
	let snapshot = load_workspace("projects/java/custom-source-group");

	assert_call_resolves_only_to(
		&snapshot,
		"srcset:main/lang:java/package:com/package:acme/package:custom/module:MainCaller/class:MainCaller/method:read(Clock)",
		"method_call",
		"now",
		0,
		"srcset:main/lang:java/package:com/package:acme/package:custom/module:Clock/class:Clock/method:now()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"srcset:test/lang:java/package:com/package:acme/package:custom/module:TestCaller/class:TestCaller/method:read(Clock)",
		"method_call",
		"now",
		0,
		"srcset:test/lang:java/package:com/package:acme/package:custom/module:Clock/class:Clock/method:now()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"srcset:test/lang:java/package:com/package:acme/package:custom/module:TestCaller/class:TestCaller/method:readProduction(ProductionOnly)",
		"method_call",
		"name",
		0,
		"srcset:main/lang:java/package:com/package:acme/package:custom/module:ProductionOnly/class:ProductionOnly/method:name()",
	);
}

#[test]
fn java_same_package_homonyms_prefer_the_source_srcset() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_resolves_only_to(
		&snapshot,
		"module:ClockUser/class:ClockUser/method:read(Clock)",
		"method_call",
		"now",
		0,
		"srcset:test/lang:java/package:com/package:acme/package:nomanifest/module:Clock/class:Clock/method:now()",
	);
}

#[test]
fn java_fluent_chains_resolve_through_return_types() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:ChainCaller/class:ChainCaller/method:chainThrough()",
		"make",
		0,
		"package:com/package:acme/package:nomanifest/module:RecordFactory/class:RecordFactory/method:make()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:ChainCaller/class:ChainCaller/method:chainThrough()",
		"describe",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:describe()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:ChainCaller/class:ChainCaller/method:ackViaChain(ChannelFactory)",
		"ack",
		0,
		"package:com/package:acme/package:nomanifest/module:Acknowledger/interface:Acknowledger/method:ack()",
	);
}

#[test]
fn java_method_calls_resolve_through_type_hierarchy() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:ChannelCaller/class:ChannelCaller/method:ackThrough(Channel)",
		"ack",
		0,
		"package:com/package:acme/package:nomanifest/module:Acknowledger/interface:Acknowledger/method:ack()",
	);
}

#[test]
fn java_inherited_fields_type_receivers_across_files() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/module:HolderChild/class:HolderChild/method:useRecord()",
		"describe",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/method:describe()",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"method_call",
		"class:HolderChild/method:useHelper()",
		"module:Helper",
	);
	assert_call_unresolved(
		&snapshot,
		"package:com/package:acme/package:nomanifest/module:LoggedChild/class:LoggedChild/method:run()",
		"info",
		1,
	);
}

#[test]
fn java_foreign_package_imports_classify_external_without_manifest() {
	let snapshot = load_workspace("projects/java/no-manifest-declared");

	assert_external_reference(
		&snapshot,
		"method_call",
		"package:com/package:thirdparty/package:util/module:Helper/path:Helper/method:help()",
	);
	assert_call_unresolved(
		&snapshot,
		"package:com/package:acme/package:nomanifest/module:ThirdPartyUser/class:ThirdPartyUser/method:describeMissing()",
		"getLabel",
		0,
	);
}

#[test]
fn java_declared_source_group_overrides_manifest_block() {
	let snapshot = load_workspace("projects/java/manifest-declared-override");

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:override/package:caller/module:PlainCaller/class:PlainCaller/method:readLabel(PlainRecord)",
		"label",
		0,
		"package:com/package:acme/package:override/module:PlainRecord/class:PlainRecord/method:label()",
	);
}

fn assert_unresolved_reasons_recorded(snapshot: &WorkspaceSnapshot) {
	assert!(
		snapshot.linkage.unresolved_refs > 0,
		"fixture should keep truly unresolved references"
	);
	assert!(
		snapshot
			.linkage
			.unresolved
			.iter()
			.all(|unresolved| unresolved.reason != UnresolvedReason::ManifestBlocked),
		"truly unresolved references must carry a non-manifest reason"
	);
	assert!(
		snapshot
			.linkage
			.unresolved
			.iter()
			.any(|unresolved| unresolved.reason == UnresolvedReason::NoCandidate),
		"lombok accessor calls should be recorded as no_candidate, got: {:?}",
		snapshot
			.linkage
			.unresolved
			.iter()
			.map(|unresolved| unresolved.reason)
			.collect::<Vec<_>>()
	);
	assert!(
		snapshot
			.linkage
			.manifest_blocked
			.iter()
			.all(|blocked| blocked.reason == UnresolvedReason::ManifestBlocked),
		"manifest-blocked references must carry the manifest_blocked reason"
	);
}

#[test]
fn ts_manifest_declared_zustand_store_api_methods_are_external() {
	let snapshot = load_workspace("projects/ts/zustand-manifest");

	assert_external_reference(&snapshot, "calls", "external_pkg:zustand/function:create");
	assert_external_origin(
		&snapshot,
		"calls",
		"external_pkg:zustand/function:create",
		ExternalReferenceOrigin::Dependency,
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:from_barrel",
		"external_pkg:zustand/function:create",
	);
	assert_external_method_call_target(
		&snapshot,
		"getState",
		"external_pkg:zustand/function:create/method:getState",
	);
	assert_external_method_call_target(
		&snapshot,
		"setState",
		"external_pkg:zustand/function:create/method:setState",
	);
	assert_external_method_call_origin(&snapshot, "getState", ExternalReferenceOrigin::Dependency);
	assert_external_method_call_origin(&snapshot, "setState", ExternalReferenceOrigin::Dependency);
}

#[test]
fn ts_no_manifest_receiver_chain_preserves_unknown_external_origin() {
	let snapshot = load_workspace("projects/ts/no-manifest");

	assert_external_origin(
		&snapshot,
		"calls",
		"external_pkg:zustand/function:create",
		ExternalReferenceOrigin::UnknownExternal,
	);
	assert_external_method_call_target(
		&snapshot,
		"getState",
		"external_pkg:zustand/function:create/method:getState",
	);
	assert_external_method_call_origin(
		&snapshot,
		"getState",
		ExternalReferenceOrigin::UnknownExternal,
	);
}

#[test]
fn ts_external_receiver_origin_is_scoped_to_the_call_site_manifest() {
	let snapshot = load_workspace("projects/ts/mixed-manifest");

	assert_external_method_call_origin_from_symbol(
		&snapshot,
		"dir:a/dir:src/module:app/function:fromA()",
		"getState",
		ExternalReferenceOrigin::Dependency,
	);
	assert_external_method_call_origin_from_symbol(
		&snapshot,
		"dir:b/dir:src/module:app/function:fromB()",
		"getState",
		ExternalReferenceOrigin::UnknownExternal,
	);
}

#[test]
fn ts_barrel_reexport_forwards_type_references() {
	let snapshot = load_workspace("projects/ts/barrel");

	assert_linked_once_from_symbol(
		&snapshot,
		"uses_type",
		"module:consumer/function:update(review:ChangeReviewResult)",
		"ChangeReviewResult",
		"module:generated/interface:ChangeReviewResult",
	);
}

#[test]
fn ts_namespace_import_calls_resolve_to_module_functions() {
	let snapshot = load_workspace("projects/ts/namespace-import");

	assert_named_call_linked_to(
		&snapshot,
		"module:index/function:kinds",
		"arrayToEnum",
		"module:util/function:arrayToEnum",
	);
	assert_named_call_linked_to(
		&snapshot,
		"module:index/function:first",
		"pickFirst",
		"module:bag/function:pickFirst",
	);
}

#[test]
fn ts_types_package_declares_the_runtime_dependency() {
	let snapshot = load_workspace("projects/ts/types-manifest");

	assert_external_origin(
		&snapshot,
		"reads",
		"external_pkg:vscode",
		ExternalReferenceOrigin::Dependency,
	);
}

#[test]
fn ts_manifest_undeclared_package_imports_are_not_external() {
	let snapshot = load_workspace("projects/ts/undeclared-manifest");

	assert_not_external_reference(
		&snapshot,
		"imports_symbol",
		"external_pkg:zustand/path:create",
	);
	assert_not_external_reference(&snapshot, "calls", "external_pkg:zustand/function:create");
	assert_not_external_method_call(&snapshot, "getState");
}

#[test]
fn go_same_package_cross_file_calls_resolve() {
	let snapshot = load_workspace("projects/go/module-workspace");

	assert_call_resolves_only_to(
		&snapshot,
		"module:app/func:Build()",
		"calls",
		"NewRouter",
		0,
		"module:router/func:NewRouter()",
	);
}

#[test]
fn python_project_module_shadows_stdlib_module() {
	let snapshot = load_workspace("projects/python/stdlib-shadow");

	assert_call_resolves_only_to(
		&snapshot,
		"module:app/function:run()",
		"calls",
		"dumps",
		1,
		"module:json/function:dumps(value)",
	);
	assert_external_reference(&snapshot, "imports_module", "sdk:python/path:sys");
}

#[test]
fn go_short_var_receivers_link_methods_across_files() {
	let snapshot = load_workspace("projects/go/module-workspace");

	assert_call_linked_to(
		&snapshot,
		"module:app/func:Build()",
		"HandleFunc",
		1,
		"module:router/struct:Router/method:HandleFunc(path:string)",
	);
	assert_call_linked_to(
		&snapshot,
		"module:app/func:Build()",
		"Path",
		0,
		"module:route/struct:Route/method:Path()",
	);
}

#[test]
fn go_stdlib_imports_classify_external_with_manifest() {
	let snapshot = load_workspace("projects/go/module-workspace");

	assert_external_reference(&snapshot, "imports_module", "sdk:go/path:fmt");
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:app/func:Build()",
		"sdk:go/path:strings/func:ToUpper",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:app/func:Build()",
		"sdk:go/path:fmt/func:Println",
	);
}

#[test]
fn python_module_constants_and_builtins_resolve() {
	let snapshot = load_workspace("projects/python/analytics-service");

	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"package:analytics_service/module:constants/path:DEFAULT_LIMIT",
		"package:analytics_service/module:constants/path:DEFAULT_LIMIT",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:limits/function:effective_limit",
		"sdk:python/path:builtins/path:print",
	);
}

#[test]
fn python_project_links_imported_types_constructors_and_methods() {
	let snapshot = load_workspace("projects/python/analytics-service");

	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"package:analytics_service/module:models/path:Customer",
		"package:analytics_service/module:models/class:Customer",
	);
	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"package:analytics_service/module:policies/path:RiskPolicy",
		"package:analytics_service/module:policies/class:RiskPolicy",
	);
	assert_linked_to(
		&snapshot,
		"uses_type",
		"package:analytics_service/module:models/path:Customer",
		"package:analytics_service/module:models/class:Customer",
	);
	assert_linked_to(
		&snapshot,
		"uses_type",
		"package:analytics_service/module:models/path:RiskScore",
		"package:analytics_service/module:models/class:RiskScore",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:analytics_service/module:service/function:build_default_service()",
		"calls",
		"RiskPolicy",
		0,
		"package:analytics_service/module:policies/class:RiskPolicy",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:analytics_service/module:service/class:AnalyticsService/method:score(customer:Customer,features:dict[str,int])",
		"method_call",
		"evaluate",
		2,
		"package:analytics_service/module:policies/class:RiskPolicy/method:evaluate(customer:Customer,features:dict[str,int])",
	);
}

#[test]
fn python_self_method_calls_resolve_through_inheritance() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:OrderRepository/method:load_orders()",
		"open_session",
		0,
		"package:orders_service/module:storage/class:BaseRepository/method:open_session()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:OrderRepository/method:backend_label()",
		"describe_backend",
		0,
		"package:orders_service/module:storage/class:BaseRepository/method:describe_backend()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:ArchivedOrderRepository/method:load_archived()",
		"load_orders",
		0,
		"package:orders_service/module:repository/class:OrderRepository/method:load_orders()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:ArchivedOrderRepository/method:load_archived()",
		"open_session",
		0,
		"package:orders_service/module:storage/class:BaseRepository/method:open_session()",
	);
}

#[test]
fn python_self_method_calls_resolve_through_multiple_bases() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:AuditedOrderRepository/method:load_audited()",
		"trace",
		1,
		"package:orders_service/module:storage/class:TracingMixin/method:trace(message:str)",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:repository/class:AuditedOrderRepository/method:load_audited()",
		"open_session",
		0,
		"package:orders_service/module:storage/class:BaseRepository/method:open_session()",
	);
}

#[test]
fn python_package_init_reexports_forward_symbols_and_inheritance() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"package:orders_service/module:catalog/path:CatalogEntry",
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry",
	);
	assert_linked_to(
		&snapshot,
		"extends",
		"package:orders_service/module:catalog/path:CatalogEntry",
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:listing/class:PricedEntry/method:price_key()",
		"entry_key",
		0,
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry/method:entry_key()",
	);
}

#[test]
fn python_module_level_values_type_their_imported_method_calls() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:setup/function:register_defaults()",
		"calls",
		"register",
		1,
		"package:orders_service/module:registry/class:RepositoryRegistry/method:register(name:str)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:setup/function:register_defaults()",
		"calls",
		"count",
		0,
		"package:orders_service/module:registry/class:RepositoryRegistry/method:count()",
	);
}

#[test]
fn python_method_calls_on_module_values_resolve_through_their_class() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:serve/function:warm_service()",
		"method_call",
		"count",
		0,
		"package:orders_service/module:registry/class:RepositoryRegistry/method:count()",
	);
	assert_call_linked_to(
		&snapshot,
		"package:tools/module:serve",
		"register",
		1,
		"package:orders_service/module:registry/class:RepositoryRegistry/method:register(name:str)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:serve/function:seed_registry(target:RepositoryRegistry)",
		"method_call",
		"count",
		0,
		"package:orders_service/module:registry/class:RepositoryRegistry/method:count()",
	);
	assert_linked_to(
		&snapshot,
		"annotates",
		"path:service/function:guard",
		"package:orders_service/module:registry/class:RepositoryRegistry/method:guard(func)",
	);
}

#[test]
fn python_unknown_receivers_do_not_link_to_unrelated_homonyms() {
	let snapshot = load_workspace("projects/python/orders-service");
	let source_identity = "package:orders_service/module:listing/function:normalize_unknown(value)";
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some("normalize")
		})
		.expect("missing normalize method call");

	assert_eq!(reference.confidence.as_deref(), Some("unresolved"));
	assert!(linked_symbol_identities(&snapshot, reference).is_empty());
	let dynamic = snapshot
		.linkage
		.dynamic
		.iter()
		.find(|dynamic| dynamic.reference == reference.id)
		.expect("unknown receiver should be explicitly classified");
	assert!(matches!(
		dynamic.reason,
		DynamicReason::DuckTypedCandidateSet | DynamicReason::InsufficientLocalFacts
	));
	let audit = resolution_audit(&snapshot, "module:listing", AuditOptions::default());
	assert!(audit.clusters.iter().any(|cluster| {
		matches!(
			cluster.pattern.reason.as_str(),
			"duck_typed_candidate_set" | "insufficient_local_facts"
		) && cluster.pattern.confidence == "unresolved"
			&& cluster.pattern.kind == "method_call"
	}));
	assert!(audit.clusters.iter().any(|cluster| {
		cluster.pattern.reason == "no_candidate"
			&& cluster.pattern.confidence == "name_match"
			&& cluster.pattern.kind == "calls"
	}));
}

#[test]
fn python_annotated_factory_returns_resolve_direct_and_assigned_chains() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"package:orders_service/module:listing/function:first_entry_key_direct()",
		"method_call",
		"entry_key",
		0,
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry/method:entry_key()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:orders_service/module:listing/function:first_entry_key_assigned()",
		"method_call",
		"entry_key",
		0,
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry/method:entry_key()",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"method_call",
		"package:orders_service/module:listing/function:first_entry_key_normalized()",
		"sdk:python/path:builtins/path:str/method:strip",
	);
}

#[test]
fn python_unknown_reads_do_not_link_to_unrelated_homonyms() {
	let snapshot = load_workspace("projects/python/orders-service");
	let source_identity = "package:orders_service/module:listing/function:read_unknown()";
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == "reads"
				&& reference.source_symbol == source.id
				&& reference
					.target_identity
					.contains("function:workspace_only_flag")
		})
		.expect("missing workspace_only_flag read");

	assert_eq!(reference.confidence.as_deref(), Some("unresolved"));
	assert!(linked_symbol_identities(&snapshot, reference).is_empty());
	assert!(snapshot.linkage.dynamic.iter().any(|dynamic| {
		dynamic.reference == reference.id && dynamic.reason == DynamicReason::InsufficientLocalFacts
	}));
}

#[test]
fn python_same_module_callable_reads_keep_their_exact_binding() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_linked_once_to(
		&snapshot,
		"reads",
		"module:listing/function:local_callback",
		"module:listing/function:local_callback()",
	);
}

#[test]
fn python_from_imports_of_submodules_link_to_their_modules() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"module:orders_service/path:catalog",
		"package:orders_service/package:catalog/module:__init__",
	);
	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"package:orders_service/module:catalog/path:entries",
		"package:orders_service/package:catalog/module:entries",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:browse/function:browse_entry(sku:str)",
		"calls",
		"CatalogEntry",
		1,
		"package:orders_service/package:catalog/module:entries/class:CatalogEntry",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:browse/function:browse_all()",
		"calls",
		"make_entry",
		1,
		"package:orders_service/package:catalog/module:entries/function:make_entry(sku:str)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:browse/function:browse_all()",
		"calls",
		"default_entry",
		0,
		"package:orders_service/package:catalog/module:__init__/function:default_entry()",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"package:tools/module:browse/function:browse_fallback()",
		"calls",
		"make_default_entry",
		0,
		"package:orders_service/package:catalog/module:entries/function:make_default_entry(sku:str)",
	);
}

#[test]
fn python_self_reads_bind_to_the_method_param() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_linked_to(
		&snapshot,
		"reads",
		"class:BaseRepository/method:open_session()/local:self",
		"class:BaseRepository/method:open_session()/param:self",
	);
	assert_linked_to(
		&snapshot,
		"reads",
		"class:BaseRepository/method:dsn_scheme()/local:self",
		"class:BaseRepository/method:dsn_scheme()/param:self",
	);
}

#[test]
fn python_bare_builtin_reads_classify_external() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_external_reference(&snapshot, "reads", "sdk:python/path:builtins/path:KeyError");
	assert_external_reference(
		&snapshot,
		"reads",
		"sdk:python/path:builtins/path:TimeoutError",
	);
}

#[test]
fn python_top_level_package_reexports_reach_sibling_consumers() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_linked_to(
		&snapshot,
		"imports_symbol",
		"module:orders_service/path:BaseRepository",
		"package:orders_service/module:storage/class:BaseRepository",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:open_report_session()",
		"calls",
		"BaseRepository",
		1,
		"package:orders_service/module:storage/class:BaseRepository",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:audit/function:audit_session()",
		"calls",
		"BaseRepository",
		1,
		"package:orders_service/module:storage/class:BaseRepository",
	);
}

#[test]
fn python_wildcard_reexports_follow_static_all_across_multiple_modules() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:open_exported_client()",
		"calls",
		"ExportedClient",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:audit/function:audit_exported_client()",
		"calls",
		"ExportedClient",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:create_exported_client()",
		"calls",
		"create",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient/method:create(name:str)",
	);
}

#[test]
fn python_regular_module_facade_preserves_external_provenance() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:report/function:open_exported_path()",
		"sdk:python/path:pathlib/function:Path",
	);
}

#[test]
fn python_conditional_imports_are_dynamic_with_bounded_candidates() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:open_conditional_client()",
		"ConditionalClient",
		1,
		&[
			"package:orders_service/module:conditional_a/class:ConditionalClient",
			"package:orders_service/module:conditional_b/class:ConditionalClient",
		],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:open_single_conditional_client()",
		"ConditionalSingleClient",
		1,
		&["package:orders_service/module:wildcard_impl/class:ExportedClient"],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:call_function_conditional_client(",
		"FunctionConditionalClient",
		1,
		&["package:orders_service/module:wildcard_impl/class:ExportedClient"],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:call_multi_function_conditional_client(",
		"RuntimeClient",
		1,
		&[
			"package:orders_service/module:conditional_a/class:ConditionalClient",
			"package:orders_service/module:conditional_b/class:ConditionalClient",
		],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:conditional/function:build_conditional_client()",
		"ConditionalClient",
		1,
		&[
			"package:orders_service/module:conditional_a/class:ConditionalClient",
			"package:orders_service/module:conditional_b/class:ConditionalClient",
		],
	);
}

#[test]
fn python_function_imports_do_not_leak_into_sibling_scopes() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:configure_scoped_client()",
		"calls",
		"ScopedClient",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
	assert_named_call_unresolved(
		&snapshot,
		"module:report/function:call_leaked_scoped_client()",
		"ScopedClient",
		1,
	);
}

#[test]
fn python_external_wildcards_do_not_leave_colliding_local_bindings_unique() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:call_shadowed_client()",
		"ShadowedClient",
		1,
		&["package:orders_service/module:wildcard_impl/class:ExportedClient"],
	);
}

#[test]
fn python_conditional_all_keeps_wildcard_consumers_dynamic() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:call_conditional_export()",
		"ConditionalExport",
		0,
		&[],
	);
}

#[test]
fn python_explicit_reexports_reach_a_fixpoint() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:call_deep_explicit_client()",
		"calls",
		"DeepExplicitClient",
		0,
		"package:orders_service/module:explicit_a/class:DeepExplicitClient",
	);
}

#[test]
fn python_local_type_sets_preserve_union_candidates() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:type_sets/function:render_union(",
		"method_call",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaRenderer/method:render()",
			"module:type_sets/class:BetaRenderer/method:render()",
		],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:type_sets/function:render_optional(",
		"render",
		0,
		&["module:type_sets/class:AlphaRenderer/method:render()"],
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:type_sets/function:render_reassigned(",
		"method_call",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaRenderer/method:render()",
			"module:type_sets/class:BetaRenderer/method:render()",
		],
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:type_sets/function:render_chained(",
		"method_call",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaRenderer/method:render()",
			"module:type_sets/class:BetaRenderer/method:render()",
		],
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:type_sets/function:render_constructed()",
		"method_call",
		"render",
		0,
		"module:type_sets/class:AlphaRenderer/method:render()",
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:type_sets/function:render_loop()",
		"method_call",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaRenderer/method:render()",
			"module:type_sets/class:BetaRenderer/method:render()",
		],
	);
	assert_call_is_candidate_with_targets(
		&snapshot,
		"module:type_sets/function:render_heterogeneous_tuple(",
		"method_call",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaRenderer/method:render()",
			"module:type_sets/class:BetaRenderer/method:render()",
		],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:type_sets/function:render_exception(",
		"render",
		0,
		&[
			"module:type_sets/class:AlphaError/method:render()",
			"module:type_sets/class:BetaError/method:render()",
		],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:type_sets/function:render_protocol(",
		"begins",
		0,
		&["module:type_sets/class:FullProtocol/method:begins()"],
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:type_sets/function:render_protocol(",
		"finishes",
		0,
		&["module:type_sets/class:FullProtocol/method:finishes()"],
	);
	assert_dynamic_reason(
		&snapshot,
		"module:type_sets/function:render_open(",
		"method_call",
		Some("runtime_only"),
		DynamicReason::InsufficientLocalFacts,
	);
	assert_dynamic_reason(
		&snapshot,
		"module:type_sets/function:read_open()",
		"reads",
		None,
		DynamicReason::InsufficientLocalFacts,
	);
}

#[test]
fn python_all_state_controls_wildcards_without_false_unique_edges() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_named_call_unresolved(
		&snapshot,
		"module:report/function:call_hidden_client()",
		"HiddenClient",
		0,
	);
	assert_call_is_dynamic_with_targets(
		&snapshot,
		"module:report/function:call_dynamic_client()",
		"DynamicClient",
		0,
		&[],
	);
}

#[test]
fn python_regular_module_imports_participate_in_facade_bindings() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:call_exported_module_client()",
		"calls",
		"ExportedClient",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"calls",
		"module:report/function:call_exported_external_module()",
		"sdk:python/path:pathlib/function:Path",
	);
}

#[test]
fn python_local_import_aliases_follow_the_exported_binding_name() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:open_aliased_client()",
		"calls",
		"ClientAlias",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
}

#[test]
fn python_nested_package_init_exports_match_module_path_access() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:open_nested_facade_client()",
		"calls",
		"ExportedClient",
		1,
		"package:orders_service/module:wildcard_impl/class:ExportedClient",
	);
}

#[test]
fn python_nested_package_wildcards_forward_static_exports() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:open_nested_wildcard_field()",
		"calls",
		"NestedField",
		1,
		"package:orders_service/package:nested/package:fields/module:__init__/class:NestedField",
	);
}

#[test]
fn python_package_init_definitions_are_explicitly_importable() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:call_package_helper()",
		"calls",
		"package_helper",
		1,
		"package:orders_service/package:nested/package:facade/module:__init__/function:package_helper(value:str)",
	);
	assert_call_resolves_only_to(
		&snapshot,
		"module:report/function:call_private_package_helper()",
		"calls",
		"_private_helper",
		1,
		"package:orders_service/package:nested/package:facade/module:__init__/function:_private_helper(value:str)",
	);
}

#[test]
fn python_self_method_calls_reach_external_stdlib_bases() {
	let snapshot = load_workspace("projects/python/orders-service");

	assert_external_reference_from_symbol(
		&snapshot,
		"method_call",
		"class:RepositoryChecks/method:test_open_session()",
		"unittest",
	);
	assert_call_linked_to(
		&snapshot,
		"package:orders_service/module:checks/class:LayeredChecks/method:test_layered_label()",
		"check_label",
		0,
		"package:orders_service/module:base_check/class:BaseCheck/method:check_label()",
	);
	assert_external_reference_from_symbol(
		&snapshot,
		"method_call",
		"class:LayeredChecks/method:test_layered_label()",
		"unittest",
	);
}

fn assert_java_platform_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:lang/path:System/path:out/method:println",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:lang/path:System/path:err/method:println",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:lang/path:String/method:trim",
	);
	assert_external_reference(
		snapshot,
		"imports_symbol",
		"package:com/package:google/package:common/package:truth/package:Truth/module:assertThat/path:assertThat",
	);
	assert_external_reference(
		snapshot,
		"imports_symbol",
		"package:org/package:junit/module:Test/path:Test",
	);
	assert_external_reference(
		snapshot,
		"annotates",
		"package:org/package:junit/module:Test/path:Test",
	);
	assert_external_reference(
		snapshot,
		"annotates",
		"sdk:java/path:java/path:lang/path:Deprecated",
	);
	assert_external_reference(
		snapshot,
		"annotates",
		"sdk:java/path:java/path:lang/path:SuppressWarnings",
	);
	assert_external_reference(
		snapshot,
		"calls",
		"package:com/package:google/package:common/package:truth/module:Truth/method:assertThat",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"package:com/package:google/package:common/package:truth/module:Truth/method:assertThat(_)/method:isEqualTo",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"package:com/package:google/package:common/package:truth/module:Truth/method:assertThat(_)/method:isTrue",
	);
}

fn assert_java_external_fluent_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_reference(
		snapshot,
		"method_call",
		"package:com/package:google/package:common/package:truth/module:Truth/method:assertThat(_)/method:hasMessageThat",
	);
	assert_external_call(
		snapshot,
		"package:com/package:acme/package:order/module:OrderArchitectureTest/class:OrderArchitectureTest/method:routesPremiumCustomerThroughPriorityLane()",
		"startsWith",
		1,
	);
}

fn assert_java_spring_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_reference(
		snapshot,
		"annotates",
		"package:org/package:springframework/package:stereotype/module:Service/path:Service",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"package:org/package:springframework/package:http/module:ResponseEntity/path:ResponseEntity/method:ok",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"package:org/package:springframework/package:boot/module:SpringApplication/path:SpringApplication/method:run",
	);
	assert_reference_from_symbol(
		snapshot,
		"annotates",
		"package:com/package:acme/package:springedge/package:api/module:CustomerController/class:CustomerController/method:getCustomer(String)/param:customerId",
		"package:org/package:springframework/package:web/package:bind/package:annotation/module:PathVariable/path:PathVariable",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:springedge/package:app/module:SpringCustomerService/path:SpringCustomerService/method:loadProfile",
		"package:com/package:acme/package:springedge/package:app/module:SpringCustomerService/class:SpringCustomerService/method:loadProfile(String)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:springedge/package:app/module:CustomerProfileDto/path:CustomerProfileDto/method:from",
		"package:com/package:acme/package:springedge/package:app/module:CustomerProfileDto/record:CustomerProfileDto/method:from(CustomerProfile)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:common/package:customer/module:RiskPolicy/path:RiskPolicy/method:isPriority",
		"package:com/package:acme/package:common/package:customer/module:RiskPolicy/class:RiskPolicy/method:isPriority(CustomerProfile)",
	);
}

fn assert_java_generic_refs(snapshot: &WorkspaceSnapshot) {
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:TypedOrderBox/path:TypedOrderBox/method:value",
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:value()",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:TypedOrderBox/path:TypedOrderBox/method:castValue",
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:castValue()",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:TypedOrderBox/path:TypedOrderBox/method:echo",
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:echo(E)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:TypedOrderBox/path:TypedOrderBox/method:identity",
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:identity(S)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:TypedOrderBox/path:TypedOrderBox/method:creator",
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:creator(TypedOrderBox<O>)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:GenericCreator/path:GenericCreator/method:create",
		"package:com/package:acme/package:order/module:GenericCreator/interface:GenericCreator/method:create(U)",
	);
	assert_call_linked_to(
		snapshot,
		"package:com/package:acme/package:order/module:OrderApplication/class:OrderApplication/method:routeOrder(String)",
		"create",
		1,
		"package:com/package:acme/package:order/module:GenericCreator/interface:GenericCreator/method:create(U)",
	);
	assert_call_linked_to(
		snapshot,
		"package:com/package:acme/package:order/module:OrderApplication/class:OrderApplication/method:routeOrder(String)",
		"value",
		0,
		"package:com/package:acme/package:order/module:TypedOrderBox/class:TypedOrderBox/method:value()",
	);
	assert_no_reference_containing(snapshot, "uses_type", "module:T/path:T");
	assert_no_reference_containing(snapshot, "uses_type", "module:E/path:E");
	assert_no_reference_containing(snapshot, "uses_type", "module:S/path:S");
	assert_no_reference_containing(snapshot, "uses_type", "module:O/path:O");
	assert_no_reference_containing(snapshot, "uses_type", "module:I/path:I");
	assert_no_reference_containing(snapshot, "uses_type", "module:U/path:U");
}

fn assert_java_nested_type_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_reference(
		snapshot,
		"uses_type",
		"sdk:java/path:java/path:util/path:Map/path:Entry",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:util/path:Map/method:entry",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:util/path:Map/path:Entry/method:getKey",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:util/path:Map/path:Entry/method:getValue",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"sdk:java/path:java/path:lang/path:Class/method:getSimpleName",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"package:com/package:acme/package:order/module:OrderContainer/path:OrderContainer/path:OrderToken",
		"package:com/package:acme/package:order/module:OrderContainer/class:OrderContainer/class:OrderToken",
	);
	assert_linked_to(
		snapshot,
		"instantiates",
		"package:com/package:acme/package:order/module:OrderContainer/class:OrderContainer/path:OrderToken",
		"package:com/package:acme/package:order/module:OrderContainer/class:OrderContainer/class:OrderToken",
	);
}

fn assert_java_switch_refs(snapshot: &WorkspaceSnapshot) {
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:OrderApplication/class:OrderApplication/method:selectLane",
		"package:com/package:acme/package:order/module:OrderApplication/class:OrderApplication/method:selectLane(CustomerProfile)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:OrderLane/path:OrderLane/method:route",
		"package:com/package:acme/package:order/module:OrderLane/enum:OrderLane/method:route()",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:common/package:customer/module:RiskPolicy/path:RiskPolicy/method:score",
		"package:com/package:acme/package:common/package:customer/module:RiskPolicy/class:RiskPolicy/method:score(CustomerProfile)",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:common/package:customer/module:CustomerProfile/path:CustomerProfile/method:segment",
		"package:com/package:acme/package:common/package:customer/module:CustomerProfile/record:CustomerProfile/method:segment()",
	);
	assert_linked_to(
		snapshot,
		"method_call",
		"package:com/package:acme/package:order/module:OrderLane/path:OrderLane/method:requiresReview",
		"package:com/package:acme/package:order/module:OrderLane/enum:OrderLane/method:requiresReview()",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"package:com/package:acme/package:order/module:OrderLane/path:OrderLane/path:PRIORITY",
		"package:com/package:acme/package:order/module:OrderLane/enum:OrderLane/enum_constant:PRIORITY",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"package:com/package:acme/package:order/module:OrderLane/path:OrderLane/path:STANDARD",
		"package:com/package:acme/package:order/module:OrderLane/enum:OrderLane/enum_constant:STANDARD",
	);
	assert_linked_to(
		snapshot,
		"reads",
		"package:com/package:acme/package:order/module:OrderLane/path:OrderLane/path:REVIEW",
		"package:com/package:acme/package:order/module:OrderLane/enum:OrderLane/enum_constant:REVIEW",
	);
}

fn assert_java_lombok_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_call_target(
		snapshot,
		"package:com/package:acme/package:order/module:LombokOrderLifecycle/class:LombokOrderLifecycle/method:activatePriorityOrder()",
		"info",
		2,
		"external_pkg:org/path:slf4j/path:Logger/method:info",
	);
	for (call, arity, method) in [
		("setStatus", 1, "method:setStatus(_)"),
		("setPriority", 1, "method:setPriority(_)"),
		("getReviewed", 0, "method:getReviewed()"),
		("getImmutableCode", 0, "method:getImmutableCode()"),
		("getStatus", 0, "method:getStatus()"),
		("isPriority", 0, "method:isPriority()"),
	] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderLifecycle/class:LombokOrderLifecycle/method:activatePriorityOrder()",
			call,
			arity,
			&format!(
				"package:com/package:acme/package:order/module:LombokOrderState/class:LombokOrderState/{method}"
			),
		);
	}
	assert_linked_to(
		snapshot,
		"calls",
		"package:com/package:acme/package:order/module:LombokFieldAccessors/class:LombokFieldAccessors/method:getFieldOnly()",
		"package:com/package:acme/package:order/module:LombokFieldAccessors/class:LombokFieldAccessors/method:getFieldOnly()",
	);
	for call in ["builder", "build"] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderBuilderUsage/class:LombokOrderBuilderUsage/method:assemble()",
			call,
			0,
			&format!(
				"package:com/package:acme/package:order/module:LombokBuildableOrder/class:LombokBuildableOrder/method:{call}()"
			),
		);
	}
	for call in ["reference", "status"] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderBuilderUsage/class:LombokOrderBuilderUsage/method:assemble()",
			call,
			1,
			&format!(
				"package:com/package:acme/package:order/module:LombokBuildableOrder/class:LombokBuildableOrder/method:{call}(_)"
			),
		);
	}
}

fn assert_java_cross_project_interface_implementations(snapshot: &WorkspaceSnapshot) {
	assert_linked_to(
		snapshot,
		"implements",
		"package:com/package:acme/package:common/package:customer/module:CustomerResolver/path:CustomerResolver",
		"package:com/package:acme/package:common/package:customer/module:CustomerResolver/interface:CustomerResolver",
	);
	assert_reference_from_symbol(
		snapshot,
		"implements",
		"package:com/package:acme/package:springedge/package:app/module:SpringCustomerRepository/class:SpringCustomerRepository",
		"package:com/package:acme/package:common/package:customer/module:CustomerResolver/path:CustomerResolver",
	);
}

fn assert_no_unresolved(snapshot: &WorkspaceSnapshot) {
	assert_eq!(
		snapshot.linkage.unresolved_refs,
		0,
		"unexpected unresolved references:\n{}",
		unresolved_report(snapshot)
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
		"dir:order-service/dir:src/module:lib/module:errors/path:LocalError",
		"dir:order-service/dir:src/module:lib/module:errors/struct:LocalError",
	);
	assert_linked_to(
		snapshot,
		"uses_type",
		"dir:order-service/dir:src/module:types/path:WildcardType",
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
		"dir:order-service/dir:src/module:lib/module:constants/path:DEFAULT_REGION",
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
	assert_linked_once_to(
		snapshot,
		"imports_symbol",
		"dir:order-service/dir:src/module:module_group/module:nested",
		"dir:order-service/dir:src/module:module_group/module:nested",
	);
	assert_linked_once_from_symbol(
		snapshot,
		"reexports",
		"dir:order-service/dir:src/module:lib",
		"dir:order-service/dir:src/module:module_group",
		"dir:order-service/dir:src/module:module_group",
	);
	assert_no_reference_containing(
		snapshot,
		"reexports",
		"dir:order-service/dir:src/module:lib/module:module_group",
	);
	assert_no_symbol_containing(
		snapshot,
		"dir:order-service/dir:src/dir:module_group/dir:nested/module:mod",
	);
	assert_symbol_count_containing(
		snapshot,
		"dir:order-service/dir:src/module:module_group/module:nested",
		1,
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
		"dir:order-service/dir:src/module:types/path:ImportedState",
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

fn assert_linked_once_to(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	reference_target: &str,
	symbol_identity: &str,
) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	let target_identities = linked_symbol_identities(snapshot, reference);
	assert_eq!(
		target_identities.len(),
		1,
		"reference `{}` should resolve to exactly one target, got [{}]",
		reference.target_identity,
		target_identities.join(", "),
	);
	assert!(
		target_identities[0].contains(symbol_identity),
		"reference `{}` was linked to `{}`, expected target containing `{}`",
		reference.target_identity,
		target_identities[0],
		symbol_identity
	);
}

fn assert_linked_once_from_symbol(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	source_identity: &str,
	reference_target: &str,
	symbol_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& reference.target_identity.contains(reference_target)
		})
		.unwrap_or_else(|| {
			panic!(
				"missing {kind} reference from `{}` to target containing `{reference_target}`",
				source.identity
			)
		});
	let target_identities = linked_symbol_identities(snapshot, reference);
	assert_eq!(
		target_identities.len(),
		1,
		"reference `{}` should resolve to exactly one target, got [{}]",
		reference.target_identity,
		target_identities.join(", "),
	);
	assert!(
		target_identities[0].contains(symbol_identity),
		"reference `{}` was linked to `{}`, expected target containing `{}`",
		reference.target_identity,
		target_identities[0],
		symbol_identity
	);
}

fn assert_named_call_linked_to(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	symbol_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
		})
		.collect::<Vec<_>>();
	assert!(
		references
			.iter()
			.any(|reference| linked_symbol_identities(snapshot, reference)
				.iter()
				.any(|identity| identity.contains(symbol_identity))),
		"no `{call_name}` call from `{}` was linked to `{symbol_identity}`; matching refs: [{}]",
		source.identity,
		references
			.iter()
			.map(|reference| format!(
				"target={} linked=[{}]",
				reference.target_identity,
				linked_symbol_identities(snapshot, reference).join(", ")
			))
			.collect::<Vec<_>>()
			.join("; ")
	);
}

fn assert_call_linked_to(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
	symbol_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert!(
		references
			.iter()
			.any(|reference| linked_symbol_identities(snapshot, reference)
				.iter()
				.any(|identity| identity.contains(symbol_identity))),
		"no `{call_name}`/{call_arity} call from `{}` was linked to `{symbol_identity}`; matching refs: [{}]",
		source.identity,
		references
			.iter()
			.map(|reference| format!(
				"target={} linked=[{}]",
				reference.target_identity,
				linked_symbol_identities(snapshot, reference).join(", ")
			))
			.collect::<Vec<_>>()
			.join("; ")
	);
}

fn assert_call_resolves_only_to(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	kind: &str,
	call_name: &str,
	call_arity: usize,
	symbol_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert_eq!(
		references.len(),
		1,
		"expected exactly one `{call_name}`/{call_arity} {kind} reference from `{}`, got {}",
		source.identity,
		references.len()
	);
	let target_identities = linked_symbol_identities(snapshot, references[0]);
	assert_eq!(
		target_identities.len(),
		1,
		"reference `{}` should resolve to exactly one target, got [{}]",
		references[0].target_identity,
		target_identities.join(", "),
	);
	assert!(
		target_identities[0].contains(symbol_identity),
		"reference `{}` was linked to `{}`, expected target containing `{}`",
		references[0].target_identity,
		target_identities[0],
		symbol_identity
	);
}

fn assert_call_is_dynamic_with_targets(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
	expected_targets: &[&str],
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			matches!(reference.kind.as_str(), "calls" | "method_call")
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.unwrap_or_else(|| panic!("missing `{call_name}`/{call_arity} call"));
	let dynamic = snapshot
		.linkage
		.dynamic
		.iter()
		.find(|dynamic| dynamic.reference == reference.id)
		.unwrap_or_else(|| {
			panic!(
				"call `{}` is not classified dynamic",
				reference.target_identity
			)
		});
	let identities = dynamic
		.candidates
		.iter()
		.filter_map(|target| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id == *target)
		})
		.map(|symbol| symbol.identity.as_ref())
		.collect::<Vec<_>>();
	assert_eq!(identities.len(), expected_targets.len(), "{identities:?}");
	for expected in expected_targets {
		assert!(
			identities
				.iter()
				.any(|identity| identity.contains(expected)),
			"candidate targets {identities:?} do not contain `{expected}`"
		);
	}
	assert!(
		snapshot
			.linkage
			.resolved
			.iter()
			.all(|edge| edge.reference != reference.id),
		"dynamic call must remain outside the unique graph"
	);
}

fn assert_call_is_candidate_with_targets(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	kind: &str,
	call_name: &str,
	call_arity: usize,
	expected_targets: &[&str],
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.unwrap_or_else(|| panic!("missing `{call_name}`/{call_arity} {kind} reference"));
	let candidate = snapshot
		.linkage
		.candidates
		.iter()
		.find(|candidate| candidate.reference == reference.id)
		.unwrap_or_else(|| {
			panic!(
				"{kind} reference `{}` is not a candidate (resolved={}, dynamic={}, unresolved={})",
				reference.target_identity,
				snapshot
					.linkage
					.resolved
					.iter()
					.any(|edge| edge.reference == reference.id),
				snapshot
					.linkage
					.dynamic
					.iter()
					.any(|entry| entry.reference == reference.id),
				snapshot
					.linkage
					.unresolved
					.iter()
					.any(|entry| entry.reference == reference.id),
			)
		});
	let identities = candidate
		.targets
		.iter()
		.filter_map(|target| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id == *target)
		})
		.map(|symbol| symbol.identity.as_ref())
		.collect::<Vec<_>>();
	assert_eq!(identities.len(), expected_targets.len(), "{identities:?}");
	for expected in expected_targets {
		assert!(
			identities
				.iter()
				.any(|identity| identity.contains(expected)),
			"candidate targets {identities:?} do not contain `{expected}`"
		);
	}
	assert!(
		snapshot
			.linkage
			.resolved
			.iter()
			.all(|edge| edge.reference != reference.id),
		"candidate call must remain outside the unique graph"
	);
}

fn assert_dynamic_reason(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	kind: &str,
	call_name: Option<&str>,
	expected: DynamicReason,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& call_name.is_none_or(|name| reference.call_name.as_deref() == Some(name))
		})
		.unwrap_or_else(|| panic!("missing `{kind}` reference from `{source_identity}`"));
	let dynamic = snapshot
		.linkage
		.dynamic
		.iter()
		.find(|dynamic| dynamic.reference == reference.id)
		.unwrap_or_else(|| panic!("reference `{}` is not dynamic", reference.target_identity));
	assert_eq!(dynamic.reason, expected);
}

fn assert_named_call_unresolved(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == "calls"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.unwrap_or_else(|| panic!("missing `{call_name}`/{call_arity} call"));
	assert!(
		snapshot
			.linkage
			.unresolved
			.iter()
			.any(|item| item.reference == reference.id),
		"call `{}` should remain unresolved",
		reference.target_identity
	);
	assert!(
		snapshot
			.linkage
			.resolved
			.iter()
			.all(|edge| edge.reference != reference.id),
		"unresolved call must remain outside the unique graph"
	);
}

fn assert_external_call(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert!(
		references
			.iter()
			.any(|reference| reference_is_external(snapshot, reference)),
		"no `{call_name}`/{call_arity} call from `{}` was classified external",
		source.identity
	);
}

fn assert_external_call_target(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
	target_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			external_target_identities(snapshot, reference)
				.iter()
				.any(|identity| identity.contains(target_identity))
		}),
		"no `{call_name}`/{call_arity} call from `{}` was external with target `{target_identity}`",
		source.identity
	);
}

fn assert_external_method_call_target(
	snapshot: &WorkspaceSnapshot,
	call_name: &str,
	target_identity: &str,
) {
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call" && reference.call_name.as_deref() == Some(call_name)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			external_target_identities(snapshot, reference)
				.iter()
				.any(|identity| identity.contains(target_identity))
		}),
		"no `{call_name}` method_call was external with target `{target_identity}`",
	);
}

fn assert_external_method_call_origin(
	snapshot: &WorkspaceSnapshot,
	call_name: &str,
	expected: ExternalReferenceOrigin,
) {
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call" && reference.call_name.as_deref() == Some(call_name)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			snapshot
				.linkage
				.external
				.iter()
				.any(|external| external.reference == reference.id && external.origin == expected)
		}),
		"no `{call_name}` method_call had external origin {expected:?}",
	);
}

fn assert_external_method_call_origin_from_symbol(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	expected: ExternalReferenceOrigin,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			snapshot
				.linkage
				.external
				.iter()
				.any(|external| external.reference == reference.id && external.origin == expected)
		}),
		"no `{call_name}` method_call from `{}` had external origin {expected:?}; observed: {:?}",
		source.identity,
		references
			.iter()
			.map(|reference| {
				(
					reference.target_identity.as_ref(),
					snapshot
						.linkage
						.external
						.iter()
						.filter(|external| external.reference == reference.id)
						.map(|external| (external.target_identity.as_ref(), external.origin))
						.collect::<Vec<_>>(),
				)
			})
			.collect::<Vec<_>>(),
	);
}

fn assert_not_external_method_call(snapshot: &WorkspaceSnapshot, call_name: &str) {
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call" && reference.call_name.as_deref() == Some(call_name)
		})
		.collect::<Vec<_>>();
	assert!(!references.is_empty(), "missing `{call_name}` method_call");
	assert!(
		references.iter().all(|reference| {
			snapshot
				.linkage
				.external
				.iter()
				.all(|external| external.reference != reference.id)
		}),
		"`{call_name}` must not bypass manifest policy through semantic propagation",
	);
}

fn assert_call_unresolved(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			snapshot
				.linkage
				.unresolved
				.iter()
				.any(|item| item.reference == reference.id)
		}),
		"`{call_name}`/{call_arity} from `{}` should remain unresolved",
		source.identity
	);
	assert!(
		references
			.iter()
			.all(|reference| linked_symbol_identities(snapshot, reference).is_empty()),
		"`{call_name}`/{call_arity} from `{}` should not be linked",
		source.identity
	);
}

fn assert_call_blocked(
	snapshot: &WorkspaceSnapshot,
	source_identity: &str,
	call_name: &str,
	call_arity: usize,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let references = snapshot
		.index
		.references
		.iter()
		.filter(|reference| {
			reference.kind == "method_call"
				&& reference.source_symbol == source.id
				&& reference.call_name.as_deref() == Some(call_name)
				&& reference.call_arity == Some(call_arity)
		})
		.collect::<Vec<_>>();
	assert!(
		references.iter().any(|reference| {
			snapshot
				.linkage
				.manifest_blocked
				.iter()
				.any(|item| item.reference == reference.id)
		}),
		"`{call_name}`/{call_arity} from `{}` should be blocked by declared source groups",
		source.identity
	);
	assert!(
		references
			.iter()
			.all(|reference| linked_symbol_identities(snapshot, reference).is_empty()),
		"`{call_name}`/{call_arity} from `{}` should not be linked",
		source.identity
	);
}

fn assert_external_reference(snapshot: &WorkspaceSnapshot, kind: &str, reference_target: &str) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	assert!(
		reference_is_external(snapshot, reference),
		"reference `{}` should be classified external",
		reference.target_identity
	);
}

fn assert_external_origin(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	reference_target: &str,
	expected: ExternalReferenceOrigin,
) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	let origin = snapshot
		.linkage
		.external
		.iter()
		.find(|external| external.reference == reference.id)
		.map(|external| external.origin);
	assert_eq!(
		origin,
		Some(expected),
		"reference `{}` has the wrong external origin",
		reference.target_identity
	);
}

fn assert_external_reference_from_symbol(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	source_identity: &str,
	reference_target: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.ends_with(source_identity))
		.or_else(|| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.identity.contains(source_identity))
		})
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& external_target_identities(snapshot, reference)
					.iter()
					.any(|identity| identity.contains(reference_target))
		})
		.unwrap_or_else(|| {
			let observed = snapshot
				.index
				.references
				.iter()
				.filter(|reference| reference.kind == kind && reference.source_symbol == source.id)
				.map(|reference| {
					(
						reference.target_identity.as_ref(),
						external_target_identities(snapshot, reference),
					)
				})
				.collect::<Vec<_>>();
			panic!(
				"missing external {kind} reference from `{}` to target containing `{reference_target}`; observed {observed:?}",
				source.identity,
			)
		});
	assert!(
		reference_is_external(snapshot, reference),
		"reference `{}` from `{}` should be classified external",
		reference.target_identity,
		source.identity
	);
}

fn assert_not_external_reference(snapshot: &WorkspaceSnapshot, kind: &str, reference_target: &str) {
	let reference = find_reference(snapshot, kind, reference_target)
		.unwrap_or_else(|| panic!("missing {kind} reference matching `{reference_target}`"));
	assert!(
		!reference_is_external(snapshot, reference),
		"reference `{}` should not be classified external",
		reference.target_identity
	);
}

fn reference_is_external(snapshot: &WorkspaceSnapshot, reference: &ReferenceRecord) -> bool {
	snapshot
		.linkage
		.external
		.iter()
		.any(|item| item.reference == reference.id)
}

fn external_target_identities(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceRecord,
) -> Vec<String> {
	snapshot
		.linkage
		.external
		.iter()
		.filter(|item| item.reference == reference.id)
		.map(|item| item.target_identity.to_string())
		.collect()
}

fn assert_reference_from_symbol(
	snapshot: &WorkspaceSnapshot,
	kind: &str,
	source_identity: &str,
	target_identity: &str,
) {
	let source = snapshot
		.index
		.symbols
		.iter()
		.find(|symbol| symbol.identity.contains(source_identity))
		.unwrap_or_else(|| panic!("missing source symbol containing `{source_identity}`"));
	let reference = snapshot
		.index
		.references
		.iter()
		.find(|reference| {
			reference.kind == kind
				&& reference.source_symbol == source.id
				&& reference.target_identity.contains(target_identity)
		})
		.unwrap_or_else(|| {
			panic!(
				"missing {kind} reference from `{}` to target containing `{target_identity}`",
				source.identity
			)
		});
	assert!(
		snapshot
			.linkage
			.unresolved
			.iter()
			.all(|item| item.reference != reference.id),
		"reference `{}` should not be unresolved",
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

fn assert_no_reference_containing(snapshot: &WorkspaceSnapshot, kind: &str, target_identity: &str) {
	assert!(
		snapshot.index.references.iter().all(|reference| {
			reference.kind != kind || !reference.target_identity.contains(target_identity)
		}),
		"unexpected {kind} reference containing `{target_identity}`"
	);
}

fn assert_no_symbol_containing(snapshot: &WorkspaceSnapshot, identity: &str) {
	assert!(
		snapshot
			.index
			.symbols
			.iter()
			.all(|symbol| !symbol.identity.contains(identity)),
		"unexpected symbol containing `{identity}`"
	);
}

fn assert_symbol_count_containing(snapshot: &WorkspaceSnapshot, identity: &str, expected: usize) {
	let count = snapshot
		.index
		.symbols
		.iter()
		.filter(|symbol| symbol.identity.ends_with(identity))
		.count();
	assert_eq!(count, expected, "unexpected symbol count for `{identity}`");
}

fn linked_symbol_identities(
	snapshot: &WorkspaceSnapshot,
	reference: &ReferenceRecord,
) -> Vec<String> {
	snapshot
		.linkage
		.resolved
		.iter()
		.filter(|edge| edge.reference == reference.id)
		.filter_map(|edge| {
			snapshot
				.index
				.symbols
				.iter()
				.find(|symbol| symbol.id == edge.target)
		})
		.map(|symbol| symbol.identity.to_string())
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
				.find(|reference| reference.id == unresolved.reference);
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
