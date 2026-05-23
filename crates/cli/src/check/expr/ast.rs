use regex::Regex;

use code_moniker_core::core::moniker::Moniker;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::check) enum Lhs {
	Name,
	Lines,
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
			Self::Kind => "kind",
			Self::Shape => "shape",
			Self::Visibility => "visibility",
			Self::Text => "text",
			Self::Moniker => "moniker",
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
			"kind" => Self::Kind,
			"shape" => Self::Shape,
			"visibility" => Self::Visibility,
			"text" => Self::Text,
			"moniker" | "self" => Self::Moniker,
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
		matches!(self, Self::Lines | Self::Depth)
	}
}

#[derive(Debug, Clone)]
pub(in crate::check) enum LhsExpr {
	Attr(Lhs),
	Number(NumberExpr),
	Mode(DomainValueExpr),
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
	FanIn(Binding),
	FanOut(Binding),
	Entropy(DomainValueExpr),
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
	Segments,
	OutRefs,
	InRefs,
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
}

#[derive(Debug, Clone)]
pub(in crate::check) enum Rhs {
	Number(NumberExpr),
	RegexStr(String),
	Moniker(Moniker),
	Str(String),
	PathPattern(crate::check::path::Pattern),
	Projection(Lhs),
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
