import {
	SymbolDetailResult,
	SymbolDto,
	SymbolUsagesResult,
	TreeNode,
} from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { SymbolNode } from "./nodes";

// Data access for the symbol tree and detail panel, all over the shared session.
// The daemon has no symbol-hierarchy query, so file symbols come back flat and we
// nest them here by line-range containment.
export class SymbolRepository {
	constructor(private readonly session: DaemonSession) {}

	get ready(): boolean {
		return this.session.ready;
	}

	async topLevelEntries(): Promise<TreeNode[]> {
		return this.entriesUnder([]);
	}

	async childEntries(dirPath: string): Promise<TreeNode[]> {
		return this.entriesUnder([dirPath]);
	}

	async fileSymbols(filePath: string): Promise<SymbolNode[]> {
		const response = await this.session.query({
			op: "symbol_search",
			workspace: null,
			text: null,
			path: [filePath],
			lang: [],
			kind: [],
			shape: [],
			name: null,
			include_non_navigable: false,
			include_code: false,
			context_lines: 0,
			projection: [],
		});
		if (response.result.kind !== "symbol_list") {
			return [];
		}
		return nestSymbols(response.result.data.rows);
	}

	async symbolDetail(uri: string, contextLines = 3): Promise<SymbolDetailResult | undefined> {
		const response = await this.session.query({
			op: "symbol_detail",
			workspace: null,
			uri,
			context_lines: contextLines,
		});
		return response.result.kind === "symbol_detail" ? response.result.data : undefined;
	}

	async symbolUsages(uri: string): Promise<SymbolUsagesResult | undefined> {
		const response = await this.session.query({
			op: "symbol_usages",
			workspace: null,
			uri,
			direction: "both",
			path: [],
			lang: [],
			projection: [],
		});
		return response.result.kind === "symbol_usages" ? response.result.data : undefined;
	}

	private async entriesUnder(path: string[]): Promise<TreeNode[]> {
		const response = await this.session.query({
			op: "tree_children",
			workspace: null,
			path,
			depth: 1,
			lang: [],
			projection: [],
		});
		if (response.result.kind !== "tree_children") {
			return [];
		}
		return [...response.result.data.rows].sort(compareEntries);
	}
}

function compareEntries(a: TreeNode, b: TreeNode): number {
	if (a.kind !== b.kind) {
		return a.kind === "directory" ? -1 : 1;
	}
	return a.path.localeCompare(b.path);
}

// Rebuilds the symbol outline from a flat list using interval nesting: a symbol's
// parent is the tightest enclosing symbol by line range.
export function nestSymbols(rows: SymbolDto[]): SymbolNode[] {
	const ranged = rows
		.filter((row) => row.line_range !== null)
		.sort((a, b) => {
			const [as, ae] = a.line_range as [number, number];
			const [bs, be] = b.line_range as [number, number];
			return as - bs || be - ae;
		});
	const roots: SymbolNode[] = [];
	const stack: SymbolNode[] = [];
	for (const symbol of ranged) {
		const node: SymbolNode = { kind: "symbol", symbol, children: [] };
		while (stack.length > 0 && !strictlyContains(stack[stack.length - 1].symbol, symbol)) {
			stack.pop();
		}
		if (stack.length === 0) {
			roots.push(node);
		} else {
			stack[stack.length - 1].children.push(node);
		}
		stack.push(node);
	}
	// Symbols without a line range cannot nest; surface them at the top level.
	for (const row of rows) {
		if (row.line_range === null) {
			roots.push({ kind: "symbol", symbol: row, children: [] });
		}
	}
	return roots;
}

function strictlyContains(outer: SymbolDto, inner: SymbolDto): boolean {
	const [os, oe] = outer.line_range as [number, number];
	const [is, ie] = inner.line_range as [number, number];
	return os <= is && oe >= ie && (os < is || oe > ie);
}
