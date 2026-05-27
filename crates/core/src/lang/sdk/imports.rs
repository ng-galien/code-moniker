use crate::core::moniker::Moniker;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportTree {
	Name(Vec<u8>),
	Alias {
		name: Vec<u8>,
		alias: Vec<u8>,
	},
	SelfImport,
	Wildcard,
	Path {
		prefix: Vec<Vec<u8>>,
		tree: Box<ImportTree>,
	},
	Group(Vec<ImportTree>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportLeafKind {
	Symbol,
	SelfImport,
	Wildcard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLeaf {
	pub kind: ImportLeafKind,
	pub path: Vec<Vec<u8>>,
	pub alias: Option<Vec<u8>>,
}

pub fn import_leaf_binding_name(leaf: &ImportLeaf) -> Option<&[u8]> {
	leaf.alias
		.as_deref()
		.or_else(|| leaf.path.last().map(Vec::as_slice))
}

pub fn importable_parent(
	target: &Moniker,
	is_importable_namespace: impl Fn(&Moniker) -> bool,
) -> Option<Moniker> {
	target.parent().filter(is_importable_namespace)
}

pub fn flatten_import_tree(tree: &ImportTree) -> Vec<ImportLeaf> {
	let mut leaves = Vec::new();
	flatten_into(tree, &mut Vec::new(), &mut leaves);
	leaves
}

fn flatten_into(tree: &ImportTree, prefix: &mut Vec<Vec<u8>>, leaves: &mut Vec<ImportLeaf>) {
	match tree {
		ImportTree::Name(name) => {
			leaves.push(leaf(ImportLeafKind::Symbol, path_with(prefix, name), None))
		}
		ImportTree::Alias { name, alias } => leaves.push(leaf(
			ImportLeafKind::Symbol,
			path_with(prefix, name),
			Some(alias.clone()),
		)),
		ImportTree::SelfImport => {
			leaves.push(leaf(ImportLeafKind::SelfImport, prefix.clone(), None));
		}
		ImportTree::Wildcard => {
			leaves.push(leaf(ImportLeafKind::Wildcard, prefix.clone(), None));
		}
		ImportTree::Path { prefix: path, tree } => {
			let original_len = prefix.len();
			prefix.extend(path.iter().cloned());
			flatten_into(tree, prefix, leaves);
			prefix.truncate(original_len);
		}
		ImportTree::Group(items) => {
			for item in items {
				flatten_into(item, prefix, leaves);
			}
		}
	}
}

fn path_with(prefix: &[Vec<u8>], name: &[u8]) -> Vec<Vec<u8>> {
	let mut path = prefix.to_vec();
	path.push(name.to_vec());
	path
}

fn leaf(kind: ImportLeafKind, path: Vec<Vec<u8>>, alias: Option<Vec<u8>>) -> ImportLeaf {
	ImportLeaf { kind, path, alias }
}
