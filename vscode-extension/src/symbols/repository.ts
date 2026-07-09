import * as vscode from "vscode";

import {
	SourceLine,
	SourceSnippet,
	SymbolDetailResult,
	SymbolDto,
	SymbolUsagesResult,
	TreeNode,
} from "../daemon/model";
import { GenerationCache } from "../daemon/cache";
import { toFsPath } from "../daemon/paths";
import { DaemonSession } from "../daemon/session";
import { InfoNode, SymbolNode, SymbolTreeNode } from "./nodes";

// Data access for the symbol tree and detail panel, all over the shared session.
// The daemon has no symbol-hierarchy query, so file symbols come back flat and we
// nest them here by line-range containment. Results are cached per workspace
// generation, so refresh cascades and re-selections cost no RPC while the
// snapshot is unchanged.
export class SymbolRepository {
	private readonly cache: GenerationCache;

	constructor(private readonly session: DaemonSession) {
		this.cache = new GenerationCache(session);
	}

	get ready(): boolean {
		return this.session.ready;
	}

	async topLevelEntries(): Promise<TreeNode[]> {
		return this.entriesUnder([]);
	}

	async childEntries(dirPath: string): Promise<TreeNode[]> {
		return this.entriesUnder([dirPath]);
	}

	async fileSymbols(filePath: string): Promise<SymbolTreeNode[]> {
		return this.cache.fetch(`file:${filePath}`, async () => {
			const response = await this.session.query(
				{
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
				},
				{ limit: PAGE_LIMIT },
			);
			if (response.result.kind !== "symbol_list") {
				return [];
			}
			const nodes: SymbolTreeNode[] = nestSymbols(response.result.data.rows);
			return withTruncationNotice(nodes, response.next_cursor != null);
		});
	}

	async symbolDetail(uri: string, contextLines = 3): Promise<SymbolDetailResult | undefined> {
		return this.cache.fetch(`detail:${contextLines}:${uri}`, async () => {
			const response = await this.session.query({
				op: "symbol_detail",
				workspace: null,
				uri,
				context_lines: contextLines,
			});
			return response.result.kind === "symbol_detail" ? response.result.data : undefined;
		});
	}

	async symbolUsages(uri: string): Promise<SymbolUsagesResult | undefined> {
		return this.cache.fetch(`usages:${uri}`, async () => {
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
		});
	}

	static async sourceSnippet(
		target: { root: string; file: string; line_range?: [number, number] | null },
		contextLines: number,
	): Promise<SourceSnippet | null> {
		if (!target.line_range) {
			return null;
		}
		try {
			const uri = vscode.Uri.file(toFsPath(target.root, target.file));
			const content = await vscode.workspace.fs.readFile(uri);
			const text = new TextDecoder().decode(content);
			return sourceSnippetFromText(target.file, text, target.line_range, contextLines);
		} catch {
			return null;
		}
	}

	private async entriesUnder(path: string[]): Promise<TreeNode[]> {
		return this.cache.fetch(`tree:${path.join("/")}`, async () => {
			const response = await this.session.query(
				{
					op: "tree_children",
					workspace: null,
					path,
					depth: 1,
					lang: [],
					projection: [],
				},
				{ limit: PAGE_LIMIT },
			);
			if (response.result.kind !== "tree_children") {
				return [];
			}
			return [...response.result.data.rows].sort(compareEntries);
		});
	}
}

const PAGE_LIMIT = 1000;

function withTruncationNotice(nodes: SymbolTreeNode[], truncated: boolean): SymbolTreeNode[] {
	if (!truncated) {
		return nodes;
	}
	const notice: InfoNode = {
		kind: "info",
		label: `only the first ${PAGE_LIMIT} symbols are shown`,
	};
	return [...nodes, notice];
}

function sourceSnippetFromText(
	file: string,
	text: string,
	range: [number, number],
	contextLines: number,
): SourceSnippet {
	const all = text.split(/\r?\n/);
	const first = Math.max(1, range[0] - contextLines);
	const last = Math.min(all.length, range[1] + contextLines);
	const lines: SourceLine[] = [];
	for (let line = first; line <= last; line++) {
		lines.push({ number: line, text: all[line - 1] ?? "" });
	}
	return {
		file,
		first_line: first,
		last_line: last,
		lines,
	};
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
		.filter((row) => row.line_range != null)
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
	for (const row of rows) {
		if (row.line_range == null) {
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
