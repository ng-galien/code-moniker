use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use code_moniker_core::lang::Lang;
use code_moniker_query::{
	CapabilitySet, CheckSummaryDto, Command, CommandRequest, CommandResponse, Consistency,
	CountDto, DaemonRpcServer, DaemonWorkspaceConfig, DiffImpactFileStatus, GraphCorridorQuery,
	GraphCorridorResult, GraphPathExpectation, GraphPathQuery, GraphPathResult, GraphPathVerdict,
	GraphSymbolScope, HandshakeResponse, Page, ProtocolRequest, ProtocolResponse, Query,
	QueryCursor, QueryError, QueryRequest, QueryResult, RulesCheckQuery, RulesCheckRootResult,
	RulesCheckVerdict, SymbolGraphQuery, SymbolSearchQuery, SymbolUsagesQuery, SyntaxNodeDto,
	SyntaxTreeQuery, UsageDirection, WorkspaceEventDto, WorkspaceEventKind, WorkspaceGeneration,
	WorkspaceLifecycle, WorkspacePhase, WorkspaceSourceDocumentDto, WorkspaceSourceSetDto,
	canonical_workspace_roots,
};
use code_moniker_workspace::live::WorkspaceLiveEvent;
use code_moniker_workspace::snapshot::{
	BoundedPathCoverage, BoundedPathLimits, MemorySourceRefreshMetrics, MemorySourceRefreshMode,
	SourceFileRecord, SourceId, WorkspaceCancellation,
};
use code_moniker_workspace::source::{
	LocalResourceCache, MEMORY_SOURCE_ROOT, MEMORY_SOURCE_ROOT_LABEL, MemorySourceDocument,
	MemorySourceSet,
};
use jsonrpsee::server::Server;

use crate::WorkspaceDaemon;
use crate::helpers;
use crate::helpers::{
	common_workspace_root, path_prefix, rules_config_root, selected_roots, source_root,
};
use crate::lifecycle::{
	producer_identity, workspace_status_result, workspace_status_without_snapshot,
};
use crate::pagination::page_rows;
use crate::query::{
	GraphSearchLimitStatus, GraphSearchOperation, bounded_source_excerpt, graph_search_assessment,
	identity_rest, validate_diff_impact_file,
};
use crate::runtime::{
	DaemonRpcService, generate_token, publish_current_snapshot, query_error,
	workspace_unavailable_response,
};
use crate::source_sets::{
	MemorySourceLimits, parse_memory_source_set, validate_memory_source_set_limits,
};

mod changes;
mod graph;
mod identity;
mod lifecycle;
mod memory_sources;
mod protocol;
mod query_contracts;
mod rpc;
mod rules;
mod runtime;
mod support;
mod symbols;
mod syntax;

use support::*;
