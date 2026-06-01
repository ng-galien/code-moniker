use regex::Regex;

use code_moniker_core::core::moniker::Moniker;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum Lhs {
	Name,
	Lines,
	StartLine,
	EndLine,
	StartByte,
	EndByte,
	Kind,
	Shape,
	Visibility,
	Text,
	Moniker,
	Depth,
	ParentMoniker,
	Confidence,
	ParentName,
	ParentKind,
	ParentShape,
	SourceName,
	SourceKind,
	SourceShape,
	SourceVisibility,
	SourceMoniker,
	SourceParentMoniker,
	TargetName,
	TargetKind,
	TargetShape,
	TargetVisibility,
	TargetMoniker,
	TargetParentMoniker,
	SegmentName,
	SegmentKind,
}

impl Lhs {
	pub(in crate::check) fn as_str(self) -> &'static str {
		match self {
			Self::Name => "name",
			Self::Lines => "lines",
			Self::StartLine => "start_line",
			Self::EndLine => "end_line",
			Self::StartByte => "start_byte",
			Self::EndByte => "end_byte",
			Self::Kind => "kind",
			Self::Shape => "shape",
			Self::Visibility => "visibility",
			Self::Text => "text",
			Self::Moniker => "uri",
			Self::Depth => "depth",
			Self::ParentMoniker => "parent",
			Self::Confidence => "confidence",
			Self::ParentName => "parent.name",
			Self::ParentKind => "parent.kind",
			Self::ParentShape => "parent.shape",
			Self::SourceName => "source.name",
			Self::SourceKind => "source.kind",
			Self::SourceShape => "source.shape",
			Self::SourceVisibility => "source.visibility",
			Self::SourceMoniker => "source",
			Self::SourceParentMoniker => "source.parent",
			Self::TargetName => "target.name",
			Self::TargetKind => "target.kind",
			Self::TargetShape => "target.shape",
			Self::TargetVisibility => "target.visibility",
			Self::TargetMoniker => "target",
			Self::TargetParentMoniker => "target.parent",
			Self::SegmentName => "segment.name",
			Self::SegmentKind => "segment.kind",
		}
	}

	pub(in crate::check) fn from_projection_name(s: &str) -> Option<Self> {
		Some(match s {
			"name" => Self::Name,
			"lines" => Self::Lines,
			"start_line" => Self::StartLine,
			"end_line" => Self::EndLine,
			"start_byte" => Self::StartByte,
			"end_byte" => Self::EndByte,
			"kind" => Self::Kind,
			"shape" => Self::Shape,
			"visibility" => Self::Visibility,
			"text" => Self::Text,
			"uri" | "moniker" | "self" => Self::Moniker,
			"depth" => Self::Depth,
			"parent" | "self.parent" => Self::ParentMoniker,
			"confidence" => Self::Confidence,
			"parent.name" => Self::ParentName,
			"parent.kind" => Self::ParentKind,
			"parent.shape" => Self::ParentShape,
			"source" => Self::SourceMoniker,
			"source.name" => Self::SourceName,
			"source.kind" => Self::SourceKind,
			"source.shape" => Self::SourceShape,
			"source.visibility" => Self::SourceVisibility,
			"source.parent" => Self::SourceParentMoniker,
			"target" => Self::TargetMoniker,
			"target.name" => Self::TargetName,
			"target.kind" => Self::TargetKind,
			"target.shape" => Self::TargetShape,
			"target.visibility" => Self::TargetVisibility,
			"target.parent" => Self::TargetParentMoniker,
			"segment.name" => Self::SegmentName,
			"segment.kind" => Self::SegmentKind,
			_ => return None,
		})
	}

	pub(in crate::check) fn is_number_projection(self) -> bool {
		matches!(
			self,
			Self::Lines
				| Self::StartLine
				| Self::EndLine
				| Self::StartByte
				| Self::EndByte
				| Self::Depth
		)
	}

	pub(in crate::check) fn accepts_op(self, op: Op) -> bool {
		use LhsProjectionKind::*;
		use Op::*;
		match self.projection_kind() {
			Text => matches!(op, Eq | Ne | RegexMatch | RegexNoMatch),
			Number => matches!(op, Lt | Le | Gt | Ge | Eq | Ne),
			Moniker => matches!(
				op,
				Eq | Ne | AncestorOf | DescendantOf | BindMatch | PathMatch
			),
		}
	}

	fn projection_kind(self) -> LhsProjectionKind {
		match self {
			Self::Lines
			| Self::StartLine
			| Self::EndLine
			| Self::StartByte
			| Self::EndByte
			| Self::Depth => LhsProjectionKind::Number,
			Self::Moniker
			| Self::ParentMoniker
			| Self::SourceMoniker
			| Self::SourceParentMoniker
			| Self::TargetMoniker
			| Self::TargetParentMoniker => LhsProjectionKind::Moniker,
			Self::Name
			| Self::Kind
			| Self::Shape
			| Self::Visibility
			| Self::Text
			| Self::Confidence
			| Self::ParentName
			| Self::ParentKind
			| Self::ParentShape
			| Self::SourceName
			| Self::SourceKind
			| Self::SourceShape
			| Self::SourceVisibility
			| Self::TargetName
			| Self::TargetKind
			| Self::TargetShape
			| Self::TargetVisibility
			| Self::SegmentName
			| Self::SegmentKind => LhsProjectionKind::Text,
		}
	}
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LhsProjectionKind {
	Text,
	Number,
	Moniker,
}

#[derive(Debug, Clone)]
pub(in crate::check) enum LhsExpr {
	Attr(Lhs),
	Number(NumberExpr),
	Collection(CollectionExpr),
	Mode(DomainValueExpr),
	PairProjection(PairProjection),
	SegmentOf { scope: SegmentScope, kind: String },
}

#[derive(Debug, Clone)]
pub(in crate::check) enum NumberExpr {
	Literal(f64),
	Projection(Lhs),
	Count {
		domain: Domain,
		filter: Option<Box<Node>>,
	},
	Aggregate {
		kind: AggregateKind,
		domain: Domain,
		expr: Box<NumberExpr>,
		percentile: Option<f64>,
	},
	Metric {
		kind: MetricKind,
		binding: Binding,
	},
	Entropy(DomainValueExpr),
	Size(CollectionExpr),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum AggregateKind {
	Sum,
	Max,
	Min,
	Avg,
	Median,
	Percentile,
	Stddev,
	Var,
	Cv,
	Gini,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum Binding {
	Self_,
	Each,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum MetricKind {
	Lcom4,
	Cbo,
	Rfc,
	Wmc,
	Dit,
	Noc,
	FanIn,
	FanOut,
}

impl MetricKind {
	pub(in crate::check) fn as_str(self) -> &'static str {
		match self {
			Self::Lcom4 => "lcom4",
			Self::Cbo => "cbo",
			Self::Rfc => "rfc",
			Self::Wmc => "wmc",
			Self::Dit => "dit",
			Self::Noc => "noc",
			Self::FanIn => "fan_in",
			Self::FanOut => "fan_out",
		}
	}
}

#[derive(Debug, Clone)]
pub(in crate::check) struct DomainValueExpr {
	pub(in crate::check) domain: Domain,
	pub(in crate::check) expr: Box<ValueExpr>,
}

#[derive(Debug, Clone)]
pub(in crate::check) enum ValueExpr {
	Item,
	Projection(Lhs),
	Number(NumberExpr),
}

#[derive(Debug, Clone)]
pub(in crate::check) enum CollectionExpr {
	Projection(CollectionProjection),
	PairProjection(PairCollectionProjection),
	Unique(Box<CollectionExpr>),
	Binary {
		op: CollectionOp,
		left: Box<CollectionExpr>,
		right: Box<CollectionExpr>,
	},
}

#[derive(Debug, Clone)]
pub(in crate::check) struct CollectionProjection {
	pub(in crate::check) domain: Domain,
	pub(in crate::check) path: Vec<String>,
}

#[derive(Debug, Clone)]
pub(in crate::check) struct PairCollectionProjection {
	pub(in crate::check) side: PairSide,
	pub(in crate::check) domain: Domain,
	pub(in crate::check) path: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum CollectionOp {
	Intersect,
	Union,
	Difference,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum PairSide {
	A,
	B,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) struct PairProjection {
	pub(in crate::check) side: PairSide,
	pub(in crate::check) lhs: Lhs,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum SegmentScope {
	Def,
	Source,
	Target,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(in crate::check) enum Domain {
	Children(String),
	ChildrenByShape(String),
	Descendants(Box<Domain>),
	Pairs(Box<Domain>),
	Segments,
	OutRefs,
	InRefs,
}

#[derive(Debug, Clone)]
pub(in crate::check) struct VerticalLayout {
	pub(in crate::check) domain: Domain,
	pub(in crate::check) public_first: bool,
	pub(in crate::check) private_after_first_use: bool,
	pub(in crate::check) max_gap: u32,
	pub(in crate::check) raw: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum QuantKind {
	Any,
	All,
	None,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum Op {
	Eq,
	Ne,
	Lt,
	Le,
	Gt,
	Ge,
	RegexMatch,
	RegexNoMatch,
	AncestorOf,
	DescendantOf,
	BindMatch,
	PathMatch,
	Subset,
}

#[derive(Debug, Clone)]
pub(in crate::check) enum Rhs {
	Number(NumberExpr),
	RegexStr(String),
	Moniker(Moniker),
	Str(String),
	PathPattern(crate::check::path::Pattern),
	Projection(Lhs),
	PairProjection(PairProjection),
	Collection(CollectionExpr),
}

#[derive(Debug, Clone)]
pub(in crate::check) struct Atom {
	pub(in crate::check) lhs: LhsExpr,
	pub(in crate::check) op: Op,
	pub(in crate::check) rhs: Rhs,
	pub(in crate::check) raw: String,
	pub(in crate::check) regex: Option<Regex>,
}

#[derive(Debug, Clone)]
pub(in crate::check) enum Node {
	Atom(Atom),
	And(Vec<Node>),
	Or(Vec<Node>),
	Not(Box<Node>),
	Implies(Box<Node>, Box<Node>),
	Require(String),
	VerticalLayout(VerticalLayout),
	Quantifier {
		kind: QuantKind,
		domain: Domain,
		filter: Box<Node>,
	},
}

#[derive(Debug, Clone)]
pub(in crate::check) struct Expr {
	pub(in crate::check) root: Node,
}
