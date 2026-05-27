use crate::core::code_graph::{CodeGraph, DefAttrs, GraphError, RefAttrs};

use super::model::{DiscoveredFile, ResolvedRef};

#[derive(Debug)]
pub enum EmitError {
	Graph(GraphError),
}

impl std::fmt::Display for EmitError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Graph(err) => write!(f, "{err}"),
		}
	}
}

impl std::error::Error for EmitError {}

impl From<GraphError> for EmitError {
	fn from(value: GraphError) -> Self {
		Self::Graph(value)
	}
}

pub struct GraphEmitter;

impl GraphEmitter {
	pub fn emit(discovered: &DiscoveredFile, refs: &[ResolvedRef]) -> Result<CodeGraph, EmitError> {
		let mut builder = GraphBuilder::new(discovered, refs.len());
		builder.emit_defs(discovered)?;
		builder.emit_refs(refs)?;
		Ok(builder.finish())
	}
}

struct GraphBuilder {
	graph: CodeGraph,
}

impl GraphBuilder {
	fn new(discovered: &DiscoveredFile, ref_count: usize) -> Self {
		Self {
			graph: CodeGraph::with_capacity(
				discovered.root.clone(),
				discovered.root_kind,
				discovered.defs.len() + 1,
				ref_count,
			),
		}
	}

	fn emit_defs(&mut self, discovered: &DiscoveredFile) -> Result<(), EmitError> {
		for def in &discovered.defs {
			let attrs = DefAttrs {
				visibility: def.visibility,
				signature: &def.signature,
				call_name: &def.call_name,
				call_arity: def.call_arity,
				..DefAttrs::default()
			};
			self.graph.add_def_attrs(
				def.moniker.clone(),
				def.kind,
				&def.parent,
				def.position,
				&attrs,
			)?;
		}
		Ok(())
	}

	fn emit_refs(&mut self, refs: &[ResolvedRef]) -> Result<(), EmitError> {
		for reference in refs {
			let attrs = RefAttrs {
				receiver_hint: &reference.hints.receiver_hint,
				alias: &reference.hints.alias,
				confidence: reference.confidence,
				call_name: &reference.hints.call_name,
				call_arity: reference.hints.call_arity,
				..RefAttrs::default()
			};
			self.graph.add_ref_attrs(
				&reference.source,
				reference.target.clone(),
				reference.kind,
				reference.position,
				&attrs,
			)?;
		}
		Ok(())
	}

	fn finish(self) -> CodeGraph {
		self.graph
	}
}
