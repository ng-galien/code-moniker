import { GenerationCache } from "../daemon/cache";
import { IdentitySegmentDto, IdentityGraphResult, SymbolDetailResult } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { SymbolRepository } from "../symbols/repository";

// Guards a pathological or cyclic identity chain; deep real chains are a
// handful of segments.
const CHAIN_LIMIT = 12;

// The landing identity after collapsing single-child containers, the display
// names of the segments walked through, and what lives at the landing — the
// walk already listed those children, so the caller gets them for free.
export interface CollapsedChain {
	identity: string;
	names: string[];
	children: IdentitySegmentDto[];
}

// Data access for the scoped exploration graph: one identity level projected
// as nodes/edges/ports (identity.graph), cached per workspace generation.
// Children listings are shared with the symbol tree's repository so both
// features hit one cache.
export class ExplorerRepository {
	private readonly cache: GenerationCache;

	constructor(
		private readonly session: DaemonSession,
		private readonly symbols: SymbolRepository,
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

	// Walks single-child chains below the prefix over the cheap
	// identity_children listing. One rule serves both the dive-through (where
	// a focus lands) and the card labels (what the dive will traverse). A
	// lone class counts: package → sole class → methods is one landing.
	async collapsedChain(prefix: string): Promise<CollapsedChain> {
		const names: string[] = [];
		let identity = prefix;
		let children = await this.symbols.identityRows(identity);
		for (let hop = 0; hop < CHAIN_LIMIT; hop++) {
			const only = children.length === 1 ? children[0] : undefined;
			if (!only?.has_children) {
				break;
			}
			names.push(only.name);
			identity = only.identity;
			children = await this.symbols.identityRows(identity);
		}
		return { identity, names, children };
	}
}
