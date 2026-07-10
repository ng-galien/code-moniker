import { GenerationCache } from "../daemon/cache";
import { IdentityGraphResult, SymbolDetailResult } from "../daemon/model";
import { DaemonSession } from "../daemon/session";

// Data access for the scoped exploration graph: one identity level projected
// as nodes/edges/ports (identity.graph), cached per workspace generation.
export class ExplorerRepository {
	private readonly cache: GenerationCache;

	constructor(private readonly session: DaemonSession) {
		this.cache = new GenerationCache(session);
	}

	get ready(): boolean {
		return this.session.ready;
	}

	async symbolDetail(uri: string, contextLines = 4): Promise<SymbolDetailResult | undefined> {
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

	async scopeGraph(prefix: string): Promise<IdentityGraphResult | undefined> {
		return this.cache.fetch(`scope:${prefix}`, async () => {
			const response = await this.session.query({
				op: "identity_graph",
				workspace: null,
				prefix,
			});
			return response.result.kind === "identity_graph" ? response.result.data : undefined;
		});
	}
}
