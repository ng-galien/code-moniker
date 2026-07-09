import { GenerationCache } from "../daemon/cache";
import { SymbolDetailResult, SymbolGraphResult } from "../daemon/model";
import { DaemonSession } from "../daemon/session";

// Data access for the ego-centric graph explorer: the unit neighborhood
// (symbol.graph) plus the focused symbol's source snippet, both cached per
// workspace generation.
export class ExplorerRepository {
	private readonly cache: GenerationCache;

	constructor(private readonly session: DaemonSession) {
		this.cache = new GenerationCache(session);
	}

	get ready(): boolean {
		return this.session.ready;
	}

	async unitGraph(focus: string): Promise<SymbolGraphResult | undefined> {
		return this.cache.fetch(`graph:${focus}`, async () => {
			const response = await this.session.query({
				op: "symbol_graph",
				workspace: null,
				focus,
			});
			return response.result.kind === "symbol_graph" ? response.result.data : undefined;
		});
	}

	async symbolDetail(uri: string, contextLines = 40): Promise<SymbolDetailResult | undefined> {
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
}
