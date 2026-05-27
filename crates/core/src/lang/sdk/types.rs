use std::collections::BTreeSet;

use crate::core::moniker::Moniker;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpr {
	Named(Vec<u8>),
	Path(Vec<Vec<u8>>),
	Ref(Box<TypeExpr>),
	Pointer(Box<TypeExpr>),
	Array(Box<TypeExpr>),
	Generic {
		base: Box<TypeExpr>,
		args: Vec<TypeExpr>,
	},
	Tuple(Vec<TypeExpr>),
	TypeParam(Vec<u8>),
	Resolved(Moniker),
	ExternalOpaque {
		origin: Moniker,
	},
	Unknown,
}

impl TypeExpr {
	pub fn resolved(target: Moniker) -> Self {
		Self::Resolved(target)
	}

	pub fn external_opaque(origin: Moniker) -> Self {
		Self::ExternalOpaque { origin }
	}

	pub fn carrier(&self) -> &Self {
		match self {
			Self::Ref(inner) | Self::Pointer(inner) => inner.carrier(),
			Self::Array(_) => self,
			Self::Generic { base, .. } => base.carrier(),
			_ => self,
		}
	}

	pub fn iterable_item(&self) -> Option<&Self> {
		match self.without_indirection() {
			Self::Array(inner) => Some(inner.without_indirection()),
			Self::Generic { args, .. } => args.first().map(TypeExpr::without_indirection),
			_ => None,
		}
	}

	pub fn target(&self) -> Option<&Moniker> {
		match self.carrier() {
			Self::Resolved(target) => Some(target),
			_ => None,
		}
	}

	pub fn receiver_owner(&self) -> Option<&Moniker> {
		match self.carrier() {
			Self::Resolved(target) => Some(target),
			Self::ExternalOpaque { origin } => Some(origin),
			_ => None,
		}
	}

	fn without_indirection(&self) -> &Self {
		match self {
			Self::Ref(inner) | Self::Pointer(inner) => inner.without_indirection(),
			_ => self,
		}
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypeEnv {
	locals: Vec<LocalType>,
	type_params: BTreeSet<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalType {
	name: Vec<u8>,
	ty: TypeExpr,
}

impl TypeEnv {
	pub fn bind_local(&mut self, name: impl Into<Vec<u8>>, ty: TypeExpr) {
		self.locals.push(LocalType {
			name: name.into(),
			ty,
		});
	}

	pub fn bind_type_param(&mut self, name: impl Into<Vec<u8>>) {
		self.type_params.insert(name.into());
	}

	pub fn is_type_param(&self, name: &[u8]) -> bool {
		self.type_params.contains(name)
	}

	pub fn resolve_local(&self, name: &[u8]) -> Option<&TypeExpr> {
		self.locals
			.iter()
			.rev()
			.find(|binding| binding.name == name)
			.map(|binding| &binding.ty)
	}

	pub fn receiver_target(&self, name: &[u8]) -> Option<&Moniker> {
		self.resolve_local(name).and_then(TypeExpr::target)
	}
}
