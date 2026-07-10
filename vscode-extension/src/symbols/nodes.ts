import { IdentitySegmentDto, SymbolDto } from "../daemon/model";

export type SymbolTreeNode = IdentityNode | SymbolNode | InfoNode;

// An organizational segment of the identity tree (srcset, lang, package, dir,
// module wrapper): it groups definitions but is not one itself. `label`
// overrides the segment name when a single-child chain was compacted.
export interface IdentityNode {
	kind: "identity";
	row: IdentitySegmentDto;
	label?: string;
	expand?: boolean;
}

// A definition row. In the identity tree `identity`/`hasChildren` drive lazy
// children; in file-outline contexts (cursor lookups) `children` is prebuilt
// from line-range nesting.
export interface SymbolNode {
	kind: "symbol";
	symbol: SymbolDto;
	children: SymbolNode[];
	identity?: string;
	hasChildren?: boolean;
	expand?: boolean;
}

export interface InfoNode {
	kind: "info";
	label: string;
}
