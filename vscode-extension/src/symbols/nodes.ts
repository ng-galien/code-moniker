import { SymbolDto, TreeNode } from "../daemon/model";

export type SymbolTreeNode = EntryNode | SymbolNode | InfoNode;

// A file or directory row sourced from `tree.children`.
export interface EntryNode {
	kind: "entry";
	tree: TreeNode;
}

// A symbol row. Children are reconstructed client-side from line-range nesting
// because the daemon returns a flat symbol list per file.
export interface SymbolNode {
	kind: "symbol";
	symbol: SymbolDto;
	children: SymbolNode[];
}

export interface InfoNode {
	kind: "info";
	label: string;
}
