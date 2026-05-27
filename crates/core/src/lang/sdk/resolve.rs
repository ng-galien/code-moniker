use crate::core::moniker::Moniker;
use crate::core::moniker::MonikerBuilder;
use crate::lang::kinds;

use super::model::{DefIndex, DiscoveredFile, ImportKind, ImportTable, ResolvedRef, TargetExpr};
use super::model::{RefHints, UnresolvedRef};
use super::scope::{Namespace, ScopeId, ScopeTree};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
	pub target: Moniker,
	pub confidence: &'static [u8],
}

pub trait LangResolverStrategy {
	fn resolve_path(
		&self,
		_target: &TargetExpr,
		_context: ResolveContext<'_>,
	) -> Option<Resolution> {
		None
	}

	fn resolve_builtin(
		&self,
		_name: &[u8],
		_namespace: Namespace,
		_context: ResolveContext<'_>,
	) -> Option<Resolution> {
		None
	}

	fn external_package_target(&self, root: &Moniker, package: &[u8], path: &[Vec<u8>]) -> Moniker {
		let mut builder = MonikerBuilder::from_view(root.as_view());
		builder.segment(kinds::EXTERNAL_PKG, package);
		for segment in path {
			builder.segment(kinds::PATH, segment);
		}
		builder.build()
	}
}

#[derive(Clone, Copy)]
pub struct ResolveContext<'a> {
	pub root: &'a Moniker,
	pub scope: ScopeId,
	pub defs: &'a DefIndex,
	pub scopes: &'a ScopeTree,
	pub imports: &'a ImportTable,
}

pub struct LocalResolver<S> {
	strategy: S,
}

impl<S> LocalResolver<S> {
	pub fn new(strategy: S) -> Self {
		Self { strategy }
	}
}

impl<S: LangResolverStrategy> LocalResolver<S> {
	pub fn resolve_file(
		&self,
		discovered: &DiscoveredFile,
		refs: impl IntoIterator<Item = UnresolvedRef>,
	) -> Vec<ResolvedRef> {
		refs.into_iter()
			.map(|reference| self.resolve_ref(discovered, reference))
			.collect()
	}

	pub fn resolve_ref(
		&self,
		discovered: &DiscoveredFile,
		reference: UnresolvedRef,
	) -> ResolvedRef {
		let namespace = reference.hints.namespace.unwrap_or(Namespace::Unified);
		let context = ResolveContext {
			root: &discovered.root,
			scope: reference.source_scope,
			defs: &discovered.def_index,
			scopes: &discovered.scopes,
			imports: &discovered.imports,
		};
		let resolution = match &reference.target {
			TargetExpr::Bare(name) => self.resolve_bare(name, namespace, context),
			TargetExpr::External { package, path } => Some(Resolution {
				target: self
					.strategy
					.external_package_target(&discovered.root, package, path),
				confidence: kinds::CONF_EXTERNAL,
			}),
			target => self.strategy.resolve_path(target, context),
		}
		.unwrap_or_else(|| Resolution {
			target: fallback_target(&discovered.root, &reference.target),
			confidence: kinds::CONF_UNRESOLVED,
		});

		ResolvedRef {
			source: reference.source,
			target: resolution.target,
			kind: reference.kind,
			position: reference.position,
			confidence: resolution.confidence,
			hints: reference.hints,
		}
	}

	fn resolve_bare(
		&self,
		name: &[u8],
		namespace: Namespace,
		context: ResolveContext<'_>,
	) -> Option<Resolution> {
		context
			.resolve_local(name, namespace)
			.or_else(|| context.resolve_import(name, namespace))
			.or_else(|| context.resolve_file_def(name, namespace))
			.or_else(|| self.strategy.resolve_builtin(name, namespace, context))
	}
}

impl ResolveContext<'_> {
	fn resolve_local(&self, name: &[u8], namespace: Namespace) -> Option<Resolution> {
		self.scopes
			.resolve(self.scope, namespace, name)
			.first()
			.map(|target| Resolution {
				target: target.clone(),
				confidence: kinds::CONF_LOCAL,
			})
	}

	fn resolve_import(&self, name: &[u8], namespace: Namespace) -> Option<Resolution> {
		self.imports
			.scoped(self.scope)
			.iter()
			.find(|import| {
				import.namespace == namespace
					&& matches!(import.kind, ImportKind::Symbol | ImportKind::Alias)
					&& import.alias == name
			})
			.map(|import| Resolution {
				target: import.target.clone(),
				confidence: import.confidence,
			})
	}

	fn resolve_file_def(&self, name: &[u8], namespace: Namespace) -> Option<Resolution> {
		self.defs
			.by_name(namespace, name)
			.first()
			.map(|target| Resolution {
				target: target.clone(),
				confidence: kinds::CONF_RESOLVED,
			})
	}
}

fn fallback_target(root: &Moniker, target: &TargetExpr) -> Moniker {
	let mut builder = MonikerBuilder::from_view(root.as_view());
	match target {
		TargetExpr::Bare(name) | TargetExpr::SelfType(name) => {
			builder.segment(kinds::PATH, name);
		}
		TargetExpr::Path(path) => {
			for segment in path {
				builder.segment(kinds::PATH, segment);
			}
		}
		TargetExpr::Receiver { name, .. } => {
			builder.segment(kinds::PATH, name);
		}
		TargetExpr::External { package, path } => {
			builder.segment(kinds::EXTERNAL_PKG, package);
			for segment in path {
				builder.segment(kinds::PATH, segment);
			}
		}
	}
	builder.build()
}

#[allow(dead_code)]
fn _assert_ref_hints_owned(_: RefHints) {}
