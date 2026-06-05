use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceWatchRoot {
	pub path: PathBuf,
	pub git_root: Option<PathBuf>,
	pub ignored_paths: Vec<PathBuf>,
	pub notes_path: Option<PathBuf>,
	pub is_source: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceLiveEvent {
	GitBaseChanged,
	Notes,
	GitBaseAndNotes,
	SourcesChanged(Vec<PathBuf>),
	SourcesAndNotes(Vec<PathBuf>),
	SourcesAndGitBase(Vec<PathBuf>),
	SourcesGitBaseAndNotes(Vec<PathBuf>),
	RescanRequired,
	RescanAndNotes,
	RescanAndGitBase,
	RescanGitBaseAndNotes,
}

impl WorkspaceLiveEvent {
	pub fn coalesce(self, other: Self) -> Self {
		WorkspaceLiveRefreshPlan::from_event(self)
			.coalesce(WorkspaceLiveRefreshPlan::from_event(other))
			.into_event()
	}

	pub fn source_paths(&self) -> Option<&[PathBuf]> {
		match self {
			Self::SourcesChanged(paths)
			| Self::SourcesAndNotes(paths)
			| Self::SourcesAndGitBase(paths)
			| Self::SourcesGitBaseAndNotes(paths) => Some(paths),
			Self::GitBaseChanged
			| Self::Notes
			| Self::GitBaseAndNotes
			| Self::RescanRequired
			| Self::RescanAndNotes
			| Self::RescanAndGitBase
			| Self::RescanGitBaseAndNotes => None,
		}
	}

	pub fn includes_notes(&self) -> bool {
		matches!(
			self,
			Self::Notes
				| Self::GitBaseAndNotes
				| Self::SourcesAndNotes(_)
				| Self::SourcesGitBaseAndNotes(_)
				| Self::RescanAndNotes
				| Self::RescanGitBaseAndNotes
		)
	}

	pub fn includes_git_base(&self) -> bool {
		matches!(
			self,
			Self::GitBaseChanged
				| Self::GitBaseAndNotes
				| Self::SourcesAndGitBase(_)
				| Self::SourcesGitBaseAndNotes(_)
				| Self::RescanAndGitBase
				| Self::RescanGitBaseAndNotes
		)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceLiveRefreshPlan {
	rescan: bool,
	source_paths: Vec<PathBuf>,
	git_base: bool,
	notes: bool,
}

impl WorkspaceLiveRefreshPlan {
	pub fn from_event(event: WorkspaceLiveEvent) -> Self {
		refresh_plan_from_event(event)
	}

	pub fn requires_rescan(&self) -> bool {
		self.rescan
	}

	pub fn source_paths(&self) -> &[PathBuf] {
		&self.source_paths
	}

	pub fn includes_git_base(&self) -> bool {
		self.git_base
	}

	pub fn includes_notes(&self) -> bool {
		self.notes
	}

	pub fn coalesce(mut self, other: Self) -> Self {
		self.rescan |= other.rescan;
		self.git_base |= other.git_base;
		self.notes |= other.notes;
		for path in other.source_paths {
			push_unique(&mut self.source_paths, path);
		}
		self
	}

	pub fn into_event(self) -> WorkspaceLiveEvent {
		refresh_plan_into_event(self)
	}
}

fn refresh_plan_from_event(event: WorkspaceLiveEvent) -> WorkspaceLiveRefreshPlan {
	match event {
		WorkspaceLiveEvent::RescanRequired => WorkspaceLiveRefreshPlan {
			rescan: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanAndNotes => WorkspaceLiveRefreshPlan {
			rescan: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanAndGitBase => WorkspaceLiveRefreshPlan {
			rescan: true,
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::RescanGitBaseAndNotes => WorkspaceLiveRefreshPlan {
			rescan: true,
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::GitBaseChanged => WorkspaceLiveRefreshPlan {
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::Notes => WorkspaceLiveRefreshPlan {
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::GitBaseAndNotes => WorkspaceLiveRefreshPlan {
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesChanged(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesAndNotes(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesAndGitBase(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			git_base: true,
			..WorkspaceLiveRefreshPlan::default()
		},
		WorkspaceLiveEvent::SourcesGitBaseAndNotes(source_paths) => WorkspaceLiveRefreshPlan {
			source_paths,
			git_base: true,
			notes: true,
			..WorkspaceLiveRefreshPlan::default()
		},
	}
}

fn refresh_plan_into_event(plan: WorkspaceLiveRefreshPlan) -> WorkspaceLiveEvent {
	if plan.rescan {
		return match (plan.git_base, plan.notes) {
			(true, true) => WorkspaceLiveEvent::RescanGitBaseAndNotes,
			(true, false) => WorkspaceLiveEvent::RescanAndGitBase,
			(false, true) => WorkspaceLiveEvent::RescanAndNotes,
			(false, false) => WorkspaceLiveEvent::RescanRequired,
		};
	}
	match (plan.source_paths.is_empty(), plan.git_base, plan.notes) {
		(false, true, true) => WorkspaceLiveEvent::SourcesGitBaseAndNotes(plan.source_paths),
		(false, true, false) => WorkspaceLiveEvent::SourcesAndGitBase(plan.source_paths),
		(false, false, true) => WorkspaceLiveEvent::SourcesAndNotes(plan.source_paths),
		(false, false, false) => WorkspaceLiveEvent::SourcesChanged(plan.source_paths),
		(true, true, true) => WorkspaceLiveEvent::GitBaseAndNotes,
		(true, true, false) => WorkspaceLiveEvent::GitBaseChanged,
		(true, false, true) => WorkspaceLiveEvent::Notes,
		(true, false, false) => WorkspaceLiveEvent::RescanRequired,
	}
}

pub(super) fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
	if !paths.iter().any(|existing| existing == &path) {
		paths.push(path);
	}
}
