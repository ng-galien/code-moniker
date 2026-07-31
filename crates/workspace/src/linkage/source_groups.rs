use crate::source::CodeIndexMaterial;

// The verdict a linkage policy renders for a (source file, target file) pair.
// Declared source groups and manifest detection both speak this language;
// declared groups are consulted first and are authoritative for any pair they
// cover, manifest detection only decides the pairs they stay silent on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::linkage) enum LinkPermission {
	Allowed,
	Blocked,
	Unknown,
}

// Source discovery owns the declared group model and records membership on
// every source file. Linkage consumes that durable classification instead of
// reparsing workspace configuration as a second, disconnected mechanism.
#[derive(Default)]
pub(in crate::linkage) struct SourceGroupPolicy;

impl SourceGroupPolicy {
	pub(in crate::linkage) fn build(_material: &CodeIndexMaterial) -> Self {
		Self
	}

	pub(in crate::linkage) fn link_permission(
		&self,
		material: &CodeIndexMaterial,
		source_file: usize,
		target_file: usize,
	) -> Option<LinkPermission> {
		let source = self.group_of(material, source_file);
		let target = self.group_of(material, target_file);
		match (source, target) {
			(None, None) => None,
			(source, target) if source == target => Some(LinkPermission::Allowed),
			_ => Some(LinkPermission::Blocked),
		}
	}

	fn group_of(&self, material: &CodeIndexMaterial, file_idx: usize) -> Option<(usize, usize)> {
		let file = material.files.get(file_idx)?;
		let source = material.source_catalog.sources.files.get(file_idx)?;
		Some((file.source_root, source.source_group?))
	}
}
