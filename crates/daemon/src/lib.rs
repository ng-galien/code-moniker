// Daemon bootstrap clones config and handles into independently owned runtime services.
mod telemetry;

pub fn set_telemetry_export_enabled(enabled: bool) {
	telemetry::set_export_enabled(enabled);
}

mod daemon;
mod helpers;
mod lifecycle;
mod pagination;
mod query;
mod runtime;
mod runtime_dependencies;
mod source_sets;

pub use daemon::WorkspaceDaemon;
pub use runtime::{serve_foreground, serve_foreground_config, serve_foreground_config_supervised};
pub use runtime_dependencies::{
	augment_workspace_status, gate_git_query, probe_runtime_dependencies,
};

mod syntax;
pub mod views;

pub use code_moniker_workspace::snapshot::WorkspaceCancellation;

pub use code_moniker_query::{
	DaemonRegistryEntry, canonical_workspace_config, canonical_workspace_root,
	canonical_workspace_roots, claim_registry_entry, config_from_roots, config_roots,
	daemon_workspace_config, list_registry_entries, pid_is_alive, registry_dir,
	registry_path_for_config, registry_path_for_root, registry_path_for_roots,
	remove_registry_entry_if_own, update_registry_entry_if_own, validate_daemon_start_config,
	workspace_label, write_registry_entry,
};

#[cfg(test)]
mod tests;
