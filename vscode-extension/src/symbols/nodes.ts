import { SymbolDto, TreeNode } from "../daemon/model";

export type SymbolTreeNode = EntryNode | SymbolNode | InfoNode;

// A file or directory row sourced from `tree.children`. `label` overrides the
// basename when a single-child directory chain was compacted into one row;
// `expand` asks the tree to unroll the row because it is its parent's only
// child.
export interface EntryNode {
	kind: "entry";
	tree: TreeNode;
	label?: string;
	expand?: boolean;
}

// A symbol row. Children are reconstructed client-side from line-range nesting
// because the daemon returns a flat symbol list per file.
export interface SymbolNode {
	kind: "symbol";
	symbol: SymbolDto;
	children: SymbolNode[];
	expand?: boolean;
}

export interface InfoNode {
	kind: "info";
	label: string;
}
