use code_moniker_core::core::moniker::{Moniker, Segment};

use crate::linkage::candidate::LinkageCandidate;
use crate::linkage::language::{LanguageLinkageStrategy, language_strategy};
use crate::snapshot::ReferenceRecord;
use crate::source::CodeIndexMaterial;

pub(super) struct LinkageQuery<'a> {
	pub(super) material: &'a CodeIndexMaterial,
	pub(super) target: &'a Moniker,
	pub(super) target_first: Option<Segment<'a>>,
	pub(super) target_last: Option<Segment<'a>>,
	pub(super) target_segment_count: usize,
	pub(super) reference_kind: &'a str,
	pub(super) call_name: Option<&'a str>,
	pub(super) call_arity: Option<usize>,
	pub(super) confidence: Option<&'a str>,
	pub(super) source_file: usize,
	strategy: &'static dyn LanguageLinkageStrategy,
}

#[derive(Clone, Copy)]
pub(super) struct ReferenceLocation {
	pub(super) source_file: usize,
	pub(super) reference: usize,
}

pub(super) struct ReferenceLocations {
	ordered: Vec<ReferenceLocation>,
	by_file: Vec<Vec<usize>>,
}

impl ReferenceLocations {
	pub(super) fn from_material(material: &CodeIndexMaterial) -> Self {
		let mut ordered = Vec::new();
		let mut by_file = Vec::with_capacity(material.files.len());
		for (source_file, file) in material.files.iter().enumerate() {
			let mut file_references = Vec::with_capacity(file.graph.ref_count());
			for reference in 0..file.graph.ref_count() {
				let reference_idx = ordered.len();
				ordered.push(ReferenceLocation {
					source_file,
					reference,
				});
				file_references.push(reference_idx);
			}
			by_file.push(file_references);
		}
		Self { ordered, by_file }
	}

	pub(super) fn get(&self, reference_idx: usize) -> Option<ReferenceLocation> {
		self.ordered.get(reference_idx).copied()
	}

	pub(super) fn reference_idx(&self, source_file: usize, reference: usize) -> Option<usize> {
		self.by_file.get(source_file)?.get(reference).copied()
	}
}

impl<'a> LinkageQuery<'a> {
	pub(super) fn new(
		reference: &'a ReferenceRecord,
		material: &'a CodeIndexMaterial,
	) -> Option<Self> {
		let (source_file, reference_idx) = material.identity.reference_location(&reference.id)?;
		Self::at(
			reference,
			material,
			ReferenceLocation {
				source_file,
				reference: reference_idx,
			},
		)
	}

	pub(super) fn at(
		reference: &'a ReferenceRecord,
		material: &'a CodeIndexMaterial,
		location: ReferenceLocation,
	) -> Option<Self> {
		let file = material.files.get(location.source_file)?;
		if location.reference >= file.graph.ref_count() {
			return None;
		}
		let target = &file.graph.ref_at(location.reference).target;
		let segment_summary = segment_summary(target);
		Some(Self {
			material,
			target,
			target_first: segment_summary.first,
			target_last: segment_summary.last,
			target_segment_count: segment_summary.count,
			reference_kind: reference.kind.as_str(),
			call_name: reference.call_name.as_deref(),
			call_arity: reference.call_arity,
			confidence: reference.confidence.as_deref(),
			source_file: location.source_file,
			strategy: language_strategy(file.lang),
		})
	}

	pub(super) fn matches(&self, candidate: &LinkageCandidate<'_>) -> bool {
		self.strategy.matches(self, candidate)
	}

	pub(super) fn target_segments(&self) -> impl Iterator<Item = Segment<'a>> + '_ {
		self.target.as_view().segments()
	}
}

struct SegmentSummary<'a> {
	first: Option<Segment<'a>>,
	last: Option<Segment<'a>>,
	count: usize,
}

fn segment_summary(target: &Moniker) -> SegmentSummary<'_> {
	let mut summary = SegmentSummary {
		first: None,
		last: None,
		count: 0,
	};
	for segment in target.as_view().segments() {
		if summary.first.is_none() {
			summary.first = Some(segment);
		}
		summary.last = Some(segment);
		summary.count += 1;
	}
	summary
}
