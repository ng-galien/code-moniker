import type { SymbolDto, SymbolGraphEdge } from "../../daemon/model";

export interface MemberNode {
	member: SymbolDto;
	children: MemberNode[];
}

// Line-range containment nesting: a member's parent is the tightest
// enclosing member. Mirrors the symbol tree's outline reconstruction.
export function nestMembers(members: SymbolDto[], focusUri: string | null): MemberNode[] {
	const ranged = members
		.filter((member) => member.line_range != null && member.uri !== focusUri)
		.slice()
		.sort(
			(a, b) =>
				(a.line_range as number[])[0] - (b.line_range as number[])[0] ||
				(b.line_range as number[])[1] - (a.line_range as number[])[1],
		);
	const roots: MemberNode[] = [];
	const stack: MemberNode[] = [];
	for (const member of ranged) {
		const node: MemberNode = { member, children: [] };
		while (stack.length > 0 && !contains(stack[stack.length - 1].member, member)) {
			stack.pop();
		}
		if (stack.length === 0) {
			roots.push(node);
		} else {
			stack[stack.length - 1].children.push(node);
		}
		stack.push(node);
	}
	for (const member of members) {
		if (member.line_range == null && member.uri !== focusUri) {
			roots.push({ member, children: [] });
		}
	}
	return roots;
}

function contains(outer: SymbolDto, inner: SymbolDto): boolean {
	const [os, oe] = outer.line_range as [number, number];
	const [is, ie] = inner.line_range as [number, number];
	return os <= is && oe >= ie && (os < is || oe > ie);
}

export function internalCounts(edges: SymbolGraphEdge[]): Map<string, number> {
	const counts = new Map<string, number>();
	for (const edge of edges) {
		counts.set(edge.source, (counts.get(edge.source) ?? 0) + edge.count);
		counts.set(edge.target, (counts.get(edge.target) ?? 0) + edge.count);
	}
	return counts;
}
