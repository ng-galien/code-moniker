use super::super::render::component::ComponentId;
use super::super::render::text::{Column, FitMode};
use super::super::source::SourceLineVm;

#[derive(Clone, Debug)]
pub(in crate::ui) struct PanelVm {
	pub(super) title: &'static str,
	pub(super) component: ComponentId,
	pub(super) wrap: WrapMode,
	pub(super) sections: Vec<PanelSection>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct PanelRenderState {
	pub(in crate::ui) scroll: usize,
	pub(in crate::ui) selected: Option<usize>,
	pub(in crate::ui) focused: bool,
}

impl PanelVm {
	pub(in crate::ui) fn new(title: &'static str, component: ComponentId) -> Self {
		Self {
			title,
			component,
			wrap: WrapMode::Wrap,
			sections: Vec::new(),
		}
	}

	pub(in crate::ui) fn unwrapped(mut self) -> Self {
		self.wrap = WrapMode::NoWrap;
		self
	}

	pub(in crate::ui) fn component(&self) -> ComponentId {
		self.component
	}

	pub(in crate::ui) fn navigation_len(&self) -> usize {
		self.sections.iter().map(PanelSection::navigation_len).sum()
	}

	pub(in crate::ui) fn section(&mut self, label: impl Into<String>) {
		self.sections.push(PanelSection::Heading {
			label: label.into(),
		});
	}

	pub(in crate::ui) fn component_section(
		&mut self,
		label: impl Into<String>,
		component: ComponentId,
	) {
		self.sections.push(PanelSection::ComponentHeading {
			label: label.into(),
			component,
		});
	}

	pub(in crate::ui) fn kv(
		&mut self,
		label: &'static str,
		value: impl Into<String>,
		fit: FitMode,
	) {
		self.sections.push(PanelSection::KeyValue {
			label,
			value: value.into(),
			fit,
		});
	}

	pub(in crate::ui) fn table(&mut self, columns: Vec<Column>, rows: Vec<Vec<String>>) {
		self.sections.push(PanelSection::Table { columns, rows });
	}

	pub(in crate::ui) fn muted(&mut self, text: impl Into<String>) {
		self.sections.push(PanelSection::Message {
			text: text.into(),
			tone: MessageTone::Muted,
		});
	}

	pub(in crate::ui) fn danger(&mut self, text: impl Into<String>) {
		self.sections.push(PanelSection::Message {
			text: text.into(),
			tone: MessageTone::Danger,
		});
	}

	pub(in crate::ui) fn bullet(&mut self, text: impl Into<String>) {
		self.sections
			.push(PanelSection::Bullet { text: text.into() });
	}

	pub(in crate::ui) fn source_snippet(&mut self, lines: Vec<SourceLineVm>) {
		self.sections.push(PanelSection::SourceSnippet(lines));
	}

	pub(in crate::ui) fn reference_groups(&mut self, groups: Vec<ReferenceGroupVm>, limit: usize) {
		self.sections
			.push(PanelSection::ReferenceGroups { groups, limit });
	}

	pub(in crate::ui) fn blank(&mut self) {
		self.sections.push(PanelSection::Blank);
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum WrapMode {
	Wrap,
	NoWrap,
}

#[derive(Clone, Debug)]
pub(super) enum PanelSection {
	Heading {
		label: String,
	},
	ComponentHeading {
		label: String,
		component: ComponentId,
	},
	KeyValue {
		label: &'static str,
		value: String,
		fit: FitMode,
	},
	Table {
		columns: Vec<Column>,
		rows: Vec<Vec<String>>,
	},
	Message {
		text: String,
		tone: MessageTone,
	},
	Bullet {
		text: String,
	},
	SourceSnippet(Vec<SourceLineVm>),
	ReferenceGroups {
		groups: Vec<ReferenceGroupVm>,
		limit: usize,
	},
	Blank,
}

impl PanelSection {
	fn navigation_len(&self) -> usize {
		match self {
			Self::Table { rows, .. } => rows.len(),
			Self::SourceSnippet(lines) => lines.len(),
			Self::ReferenceGroups { groups, limit } => groups.len().min(*limit),
			Self::Heading { .. }
			| Self::ComponentHeading { .. }
			| Self::KeyValue { .. }
			| Self::Message { .. }
			| Self::Bullet { .. }
			| Self::Blank => 0,
		}
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MessageTone {
	Muted,
	Danger,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ReferenceGroupVm {
	pub(in crate::ui) kinds: Vec<String>,
	pub(in crate::ui) actor: String,
	pub(in crate::ui) location: String,
	pub(in crate::ui) endpoint_label: &'static str,
	pub(in crate::ui) endpoint: String,
	pub(in crate::ui) confidence: String,
	pub(in crate::ui) receiver: Option<String>,
	pub(in crate::ui) alias: Option<String>,
}
