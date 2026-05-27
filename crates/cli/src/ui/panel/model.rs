use super::super::render::component::ComponentId;
use super::super::render::text::{Column, FitMode};
use super::super::render::tree::TreeRowVm;

#[derive(Clone, Debug)]
pub(in crate::ui) struct SourceLineVm {
	pub(in crate::ui) number: u32,
	pub(in crate::ui) number_width: usize,
	pub(in crate::ui) text: String,
	pub(in crate::ui) active: bool,
}

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

	#[cfg(test)]
	pub(in crate::ui) fn component(&self) -> ComponentId {
		self.component
	}

	#[cfg(test)]
	pub(in crate::ui) fn navigation_len(&self) -> usize {
		self.sections.iter().map(PanelSection::navigation_len).sum()
	}
}

pub(in crate::ui) fn panel_section(vm: &mut PanelVm, label: impl Into<String>) {
	vm.sections.push(PanelSection::Heading {
		label: label.into(),
	});
}

pub(in crate::ui) fn panel_component_section(
	vm: &mut PanelVm,
	label: impl Into<String>,
	component: ComponentId,
) {
	vm.sections.push(PanelSection::ComponentHeading {
		label: label.into(),
		component,
	});
}

pub(in crate::ui) fn panel_kv(
	vm: &mut PanelVm,
	label: &'static str,
	value: impl Into<String>,
	fit: FitMode,
) {
	vm.sections.push(PanelSection::KeyValue {
		label,
		value: value.into(),
		fit,
	});
}

pub(in crate::ui) fn panel_table(vm: &mut PanelVm, columns: Vec<Column>, rows: Vec<Vec<String>>) {
	vm.sections.push(PanelSection::Table { columns, rows });
}

pub(in crate::ui) fn panel_muted(vm: &mut PanelVm, text: impl Into<String>) {
	vm.sections.push(PanelSection::Message {
		text: text.into(),
		tone: MessageTone::Muted,
	});
}

pub(in crate::ui) fn panel_danger(vm: &mut PanelVm, text: impl Into<String>) {
	vm.sections.push(PanelSection::Message {
		text: text.into(),
		tone: MessageTone::Danger,
	});
}

pub(in crate::ui) fn panel_bullet(vm: &mut PanelVm, text: impl Into<String>) {
	vm.sections.push(PanelSection::Bullet { text: text.into() });
}

pub(in crate::ui) fn panel_tree_rows(vm: &mut PanelVm, rows: Vec<TreeRowVm>) {
	vm.sections.push(PanelSection::TreeRows(rows));
}

pub(in crate::ui) fn panel_source_snippet(vm: &mut PanelVm, lines: Vec<SourceLineVm>) {
	vm.sections.push(PanelSection::SourceSnippet(lines));
}

pub(in crate::ui) fn panel_reference_groups(
	vm: &mut PanelVm,
	groups: Vec<ReferenceGroupVm>,
	limit: usize,
) {
	vm.sections
		.push(PanelSection::ReferenceGroups { groups, limit });
}

pub(in crate::ui) fn panel_blank(vm: &mut PanelVm) {
	vm.sections.push(PanelSection::Blank);
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
	TreeRows(Vec<TreeRowVm>),
	SourceSnippet(Vec<SourceLineVm>),
	ReferenceGroups {
		groups: Vec<ReferenceGroupVm>,
		limit: usize,
	},
	Blank,
}

impl PanelSection {
	#[cfg(test)]
	fn navigation_len(&self) -> usize {
		match self {
			Self::Table { rows, .. } => rows.len(),
			Self::TreeRows(rows) => rows.len(),
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
