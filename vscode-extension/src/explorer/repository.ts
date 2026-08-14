import { GenerationCache } from "../daemon/cache";
import {
	SymbolDetailResult,
	SymbolDto,
	SymbolGraphResult,
} from "../daemon/model";
import { DaemonSession } from "../daemon/session";

// Data access for the symbol-centered cockpit, cached per workspace generation.
export class ExplorerRepository {
	private readonly cache: GenerationCache;

	constructor(
		private readonly session: DaemonSession,
	) {
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

	async symbolGraph(focus: string): Promise<SymbolGraphResult | undefined> {
		return this.cache.fetch(`symbol-graph:${focus}`, async () => {
			const response = await this.session.query({
				op: "symbol_graph",
				workspace: null,
				focus,
				direction: "both",
				relation: [],
				min_count: 1,
					// The cockpit is an ego graph. Descendant-to-descendant edges would
					// turn the focused neighborhood into a dense structural graph.
				include_internal: false,
			});
			return response.result.kind === "symbol_graph" ? response.result.data : undefined;
		});
	}

	async search(text: string, limit = 12, identityPrefix?: string): Promise<SymbolDto[]> {
		const response = await this.session.query(
			{
				op: "symbol_search",
				workspace: null,
				text,
				path: [],
				lang: [],
				kind: [],
				shape: [],
				name: null,
				include_non_navigable: false,
				include_code: false,
				context_lines: 0,
				projection: [],
			},
			{ limit: identityPrefix ? Math.max(limit * 8, 80) : limit },
		);
		if (response.result.kind !== "symbol_list") return [];
		const rows = identityPrefix
			? response.result.data.rows.filter((symbol) => {
				const identity = identityFromUri(symbol.uri);
				return identity === identityPrefix || identity.startsWith(`${identityPrefix}/`);
			})
			: response.result.data.rows;
		return rows.slice(0, limit);
	}

}

function identityFromUri(uri: string): string {
	const scheme = uri.indexOf("://");
	if (scheme < 0) return uri;
	const rooted = uri.slice(scheme + 3);
	const separator = rooted.indexOf("/");
	return separator < 0 ? "" : rooted.slice(separator + 1);
}
