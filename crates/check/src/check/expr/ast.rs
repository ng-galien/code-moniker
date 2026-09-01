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
	Srcset,
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
	SourceSrcset,
	SourceMoniker,
	SourceParentMoniker,
	TargetName,
	TargetKind,
	TargetShape,
	TargetVisibility,
	TargetSrcset,
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
			Self::Srcset => "srcset",
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
			Self::SourceSrcset => "source.srcset",
			Self::SourceMoniker => "source",
			Self::SourceParentMoniker => "source.parent",
			Self::TargetName => "target.name",
			Self::TargetKind => "target.kind",
			Self::TargetShape => "target.shape",
			Self::TargetVisibility => "target.visibility",
			Self::TargetSrcset => "target.srcset",
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
			"srcset" => Self::Srcset,
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
			"source.srcset" => Self::SourceSrcset,
			"source.parent" => Self::SourceParentMoniker,
			"target" => Self::TargetMoniker,
			"target.name" => Self::TargetName,
			"target.kind" => Self::TargetKind,
			"target.shape" => Self::TargetShape,
			"target.visibility" => Self::TargetVisibility,
			"target.srcset" => Self::TargetSrcset,
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

	pub(in crate::check) fn is_moniker_projection(self) -> bool {
		self.projection_kind() == LhsProjectionKind::Moniker
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
			| Self::Srcset
			| Self::Text
			| Self::Confidence
			| Self::ParentName
			| Self::ParentKind
			| Self::ParentShape
			| Self::SourceName
			| Self::SourceKind
			| Self::SourceShape
			| Self::SourceVisibility
			| Self::SourceSrcset
			| Self::TargetName
			| Self::TargetKind
			| Self::TargetShape
			| Self::TargetVisibility
			| Self::TargetSrcset
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

impl AggregateKind {
	pub(in crate::check) fn as_str(self) -> &'static str {
		match self {
			Self::Sum => "sum",
			Self::Max => "max",
			Self::Min => "min",
			Self::Avg => "avg",
			Self::Median => "median",
			Self::Percentile => "percentile",
			Self::Stddev => "stddev",
			Self::Var => "var",
			Self::Cv => "cv",
			Self::Gini => "gini",
		}
	}
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
	pub(in crate::check) filter: Option<Box<Node>>,
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
	Ast,
	Children(String),
	ChildrenByShape(String),
	Descendants(Box<Domain>),
	Pairs(Box<Domain>),
	Segments,
	OutRefs,
	InRefs,
	SourceOutRefs,
	SourceInRefs,
	TargetOutRefs,
	TargetInRefs,
	SourceAncestorOutRefs,
	SourceAncestorInRefs,
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
	CurrentProjection(Lhs),
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

impl Node {
	pub(in crate::check) fn ast_kind_names(&self) -> Result<Option<Vec<String>>, &'static str> {
		let mut analysis = AstAnalysis::default();
		analysis.visit_node(self, false, false);
		match analysis.unsupported {
			Some(reason) => Err(reason),
			None => Ok(analysis.uses_ast.then_some(analysis.kind_names)),
		}
	}
}

#[derive(Default)]
struct AstAnalysis {
	uses_ast: bool,
	kind_names: Vec<String>,
	unsupported: Option<&'static str>,
}

impl AstAnalysis {
	fn visit_node(&mut self, node: &Node, ast_filter: bool, nested_domain: bool) {
		match node {
			Node::Atom(atom) => self.visit_atom(atom, ast_filter, nested_domain),
			Node::And(nodes) | Node::Or(nodes) => {
				self.visit_nodes(nodes, ast_filter, nested_domain)
			}
			Node::Not(node) => self.visit_node(node, ast_filter, nested_domain),
			Node::Implies(left, right) => {
				self.visit_nodes([left.as_ref(), right.as_ref()], ast_filter, nested_domain);
			}
			Node::Quantifier { domain, filter, .. } => {
				self.visit_quantifier(domain, filter, ast_filter, nested_domain)
			}
			Node::VerticalLayout(layout) => self.visit_layout(layout, ast_filter),
			Node::Require(_) if ast_filter => {
				self.reject("require is not supported in an AST filter")
			}
			Node::Require(_) => {}
		}
	}

	fn visit_nodes<'a>(
		&mut self,
		nodes: impl IntoIterator<Item = &'a Node>,
		ast_filter: bool,
		nested_domain: bool,
	) {
		for node in nodes {
			self.visit_node(node, ast_filter, nested_domain);
		}
	}

	fn visit_quantifier(
		&mut self,
		domain: &Domain,
		filter: &Node,
		ast_filter: bool,
		nested_domain: bool,
	) {
		if ast_filter {
			self.reject("nested quantifiers are not supported in an AST filter");
		}
		let ast_domain = domain.contains_ast();
		self.uses_ast |= ast_domain;
		if ast_domain && nested_domain {
			self.reject("ast cannot be nested under another domain");
		}
		if ast_domain && *domain != Domain::Ast {
			self.reject("ast must be used as a direct quantifier domain");
		}
		self.visit_node(filter, ast_domain, nested_domain || !ast_domain);
	}

	fn visit_layout(&mut self, layout: &VerticalLayout, ast_filter: bool) {
		if ast_filter {
			self.reject("vertical_layout is not supported in an AST filter");
		}
		if layout.domain.contains_ast() {
			self.uses_ast = true;
			self.reject("vertical_layout does not support the ast domain");
		}
	}

	fn visit_atom(&mut self, atom: &Atom, ast_filter: bool, nested_domain: bool) {
		if ast_filter {
			let lhs = atom.lhs.ast_filter_projection();
			let rhs_kind = atom.rhs.ast_filter_projection_kind();
			if lhs.is_none() || rhs_kind.is_none() {
				self.reject("AST filters only support AST scalar projections and literals");
			} else if lhs.is_some_and(|lhs| !lhs.accepts_op(atom.op)) {
				self.reject("operator is not supported for this AST projection");
			} else if lhs.map(Lhs::projection_kind) != rhs_kind {
				self.reject("AST filter operands must have the same scalar type");
			}
			if matches!(atom.lhs, LhsExpr::Attr(Lhs::Kind | Lhs::ParentKind)) {
				match &atom.rhs {
					Rhs::Str(kind) => self.kind_names.push(kind.clone()),
					Rhs::RegexStr(_) => {
						self.reject("regex comparisons are not supported for AST kind projections")
					}
					_ => {}
				}
			}
		}
		self.visit_lhs(&atom.lhs, nested_domain);
		self.visit_rhs(&atom.rhs, nested_domain);
	}

	fn visit_lhs(&mut self, lhs: &LhsExpr, nested_domain: bool) {
		match lhs {
			LhsExpr::Number(expr) => self.visit_number(expr, nested_domain),
			LhsExpr::Collection(expr) => self.visit_collection(expr),
			LhsExpr::Mode(expr) => self.visit_domain_value(expr, nested_domain),
			LhsExpr::Attr(_) | LhsExpr::PairProjection(_) | LhsExpr::SegmentOf { .. } => {}
		}
	}

	fn visit_rhs(&mut self, rhs: &Rhs, nested_domain: bool) {
		match rhs {
			Rhs::Number(expr) => self.visit_number(expr, nested_domain),
			Rhs::Collection(expr) => self.visit_collection(expr),
			_ => {}
		}
	}

	fn visit_number(&mut self, expr: &NumberExpr, nested_domain: bool) {
		match expr {
			NumberExpr::Count { domain, filter } => {
				let ast_domain = domain.contains_ast();
				self.uses_ast |= ast_domain;
				if ast_domain && nested_domain {
					self.reject("ast cannot be nested under another domain");
				}
				if ast_domain && *domain != Domain::Ast {
					self.reject("ast must be used as a direct count domain");
				}
				if let Some(filter) = filter {
					self.visit_node(filter, ast_domain, nested_domain || !ast_domain);
				}
			}
			NumberExpr::Aggregate { domain, expr, .. } => {
				if domain.contains_ast() {
					self.uses_ast = true;
					self.reject("aggregates do not support the ast domain");
				}
				self.visit_number(expr, true);
			}
			NumberExpr::Entropy(expr) => self.visit_domain_value(expr, nested_domain),
			NumberExpr::Size(expr) => self.visit_collection(expr),
			NumberExpr::Literal(_) | NumberExpr::Projection(_) | NumberExpr::Metric { .. } => {}
		}
	}

	fn visit_domain_value(&mut self, expr: &DomainValueExpr, nested_domain: bool) {
		let ast_domain = expr.domain.contains_ast();
		self.uses_ast |= ast_domain;
		if ast_domain && nested_domain {
			self.reject("ast cannot be nested under another domain");
		}
		if ast_domain {
			self.reject("mode and entropy do not support the ast domain");
		}
		if let Some(filter) = &expr.filter {
			self.visit_node(filter, ast_domain, true);
		}
		if let ValueExpr::Number(expr) = expr.expr.as_ref() {
			self.visit_number(expr, true);
		}
	}

	fn visit_collection(&mut self, expr: &CollectionExpr) {
		match expr {
			CollectionExpr::Projection(projection) => {
				if projection.domain.contains_ast() {
					self.uses_ast = true;
					self.reject("collection projections do not support the ast domain");
				}
			}
			CollectionExpr::PairProjection(projection) => {
				if projection.domain.contains_ast() {
					self.uses_ast = true;
					self.reject("pair projections do not support the ast domain");
				}
			}
			CollectionExpr::Unique(inner) => self.visit_collection(inner),
			CollectionExpr::Binary { left, right, .. } => {
				self.visit_collection(left);
				self.visit_collection(right);
			}
		}
	}

	fn reject(&mut self, reason: &'static str) {
		if self.unsupported.is_none() {
			self.unsupported = Some(reason);
		}
	}
}

impl Domain {
	fn contains_ast(&self) -> bool {
		matches!(self, Self::Ast)
			|| matches!(self, Self::Descendants(inner) | Self::Pairs(inner) if inner.contains_ast())
	}
}

impl LhsExpr {
	fn ast_filter_projection(&self) -> Option<Lhs> {
		match self {
			Self::Attr(lhs) | Self::Number(NumberExpr::Projection(lhs))
				if lhs.is_ast_projection() =>
			{
				Some(*lhs)
			}
			_ => None,
		}
	}
}

impl Lhs {
	fn is_ast_projection(self) -> bool {
		matches!(
			self,
			Self::Kind
				| Self::Text | Self::StartByte
				| Self::EndByte
				| Self::StartLine
				| Self::EndLine
				| Self::Lines
				| Self::ParentKind
		)
	}
}

impl Rhs {
	fn ast_filter_projection_kind(&self) -> Option<LhsProjectionKind> {
		match self {
			Self::Str(_) | Self::RegexStr(_) => Some(LhsProjectionKind::Text),
			Self::Number(NumberExpr::Literal(_)) => Some(LhsProjectionKind::Number),
			Self::Number(NumberExpr::Projection(lhs)) | Self::Projection(lhs)
				if lhs.is_ast_projection() =>
			{
				Some(lhs.projection_kind())
			}
			_ => None,
		}
	}
}

#[derive(Debug, Clone)]
pub(in crate::check) struct Expr {
	pub(in crate::check) root: Node,
}
