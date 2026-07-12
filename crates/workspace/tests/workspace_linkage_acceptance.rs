use std::path::{Path, PathBuf};

use code_moniker_workspace::extract::JavaExtractionPipeline;
use code_moniker_workspace::snapshot::{
	ReferenceRecord, UnresolvedReason, WorkspaceRequest, WorkspaceSnapshot,
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
	workspace
		.queries()
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
fn rust_qualified_calls_resolve_through_module_reexports() {
	let snapshot = load_workspace("projects/rust/reexport-qualified");

	assert_call_resolves_only_to(
		&snapshot,
		"module:app/fn:run()",
		"calls",
		"version",
		0,
		"module:store/path:version",
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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(vec![fixture_path("projects/java/multiprojet")], None)
			.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(vec![fixture_path("projects/java/lombok-boundaries")], None)
			.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:MainCaller/class:MainCaller/method:readLabel(SharedRecord)",
		"getLabel",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/field:label",
	);
	assert_call_linked_to(
		&snapshot,
		"package:com/package:acme/package:nomanifest/package:caller/module:TestCaller/class:TestCaller/method:readLabel(SharedRecord)",
		"getLabel",
		0,
		"package:com/package:acme/package:nomanifest/module:SharedRecord/class:SharedRecord/field:label",
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
fn java_declared_source_groups_block_cross_group_calls() {
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

	assert_call_unresolved(
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
fn java_fluent_chains_resolve_through_return_types() {
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	assert_external_reference_from_symbol(
		&snapshot,
		"method_call",
		"class:LoggedChild/method:run()",
		"Logger",
	);
}

#[test]
fn java_foreign_package_imports_classify_external_without_manifest() {
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/no-manifest-declared")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
	let snapshot = load_workspace_with_options(
		LocalWorkspaceOptions::new(
			vec![fixture_path("projects/java/manifest-declared-override")],
			None,
		)
		.with_java_pipeline(JavaExtractionPipeline::Sdk),
	);

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
fn ts_manifest_undeclared_package_imports_are_not_external() {
	let snapshot = load_workspace("projects/ts/undeclared-manifest");

	assert_not_external_reference(
		&snapshot,
		"imports_symbol",
		"external_pkg:zustand/path:create",
	);
	assert_not_external_reference(&snapshot, "calls", "external_pkg:zustand/function:create");
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

fn assert_java_platform_refs(snapshot: &WorkspaceSnapshot) {
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:lang/path:System/method:println",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:lang/path:String/method:trim",
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
		"external_pkg:java/path:lang/path:Deprecated",
	);
	assert_external_reference(
		snapshot,
		"annotates",
		"external_pkg:java/path:lang/path:SuppressWarnings",
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
		"external_pkg:java/path:util/path:Map/path:Entry",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:util/path:Map/method:entry",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:util/path:Map/path:Entry/method:getKey",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:util/path:Map/path:Entry/method:getValue",
	);
	assert_external_reference(
		snapshot,
		"method_call",
		"external_pkg:java/path:lang/path:Class/method:getSimpleName",
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
	for (call, arity, field) in [
		("setStatus", 1, "field:status"),
		("setPriority", 1, "field:priority"),
		("getReviewed", 0, "field:reviewed"),
		("getImmutableCode", 0, "field:immutableCode"),
		("getStatus", 0, "field:status"),
		("isPriority", 0, "field:priority"),
	] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderLifecycle/class:LombokOrderLifecycle/method:activatePriorityOrder()",
			call,
			arity,
			&format!(
				"package:com/package:acme/package:order/module:LombokOrderState/class:LombokOrderState/{field}"
			),
		);
	}
	assert_linked_to(
		snapshot,
		"calls",
		"package:com/package:acme/package:order/module:LombokFieldAccessors/class:LombokFieldAccessors/method:getFieldOnly()",
		"package:com/package:acme/package:order/module:LombokFieldAccessors/class:LombokFieldAccessors/field:fieldOnly",
	);
	for call in ["builder", "build"] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderBuilderUsage/class:LombokOrderBuilderUsage/method:assemble()",
			call,
			0,
			"package:com/package:acme/package:order/module:LombokBuildableOrder/class:LombokBuildableOrder",
		);
	}
	for (call, field) in [("reference", "field:reference"), ("status", "field:status")] {
		assert_call_linked_to(
			snapshot,
			"package:com/package:acme/package:order/module:LombokOrderBuilderUsage/class:LombokOrderBuilderUsage/method:assemble()",
			call,
			1,
			&format!(
				"package:com/package:acme/package:order/module:LombokBuildableOrder/class:LombokBuildableOrder/{field}"
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
			panic!(
				"missing external {kind} reference from `{}` to target containing `{reference_target}`",
				source.identity
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
