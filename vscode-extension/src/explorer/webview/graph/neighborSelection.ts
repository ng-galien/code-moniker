import type { SymbolDto, SymbolGraphNeighbor } from "../../../daemon/model";

export const INITIAL_RELATION_BUDGET = 6;
export const INITIAL_DIRECTION_FLOOR = 2;
export const EXPANSION_RELATION_BATCH = 4;

export function rankCodeNeighbors(
	rows: readonly SymbolGraphNeighbor[],
	focus: SymbolDto,
): SymbolGraphNeighbor[] {
	const merged = new Map<string, SymbolGraphNeighbor>();
	for (const row of rows) {
		const current = merged.get(row.symbol.uri);
		merged.set(row.symbol.uri, current
			? {
				symbol: current.symbol,
				count: current.count + row.count,
				kinds: [...new Set([...current.kinds, ...row.kinds])],
			}
			: { ...row, kinds: [...new Set(row.kinds)] });
	}
	return [...merged.values()].sort((left, right) =>
		codeNeighborScore(right, focus) - codeNeighborScore(left, focus) ||
		left.symbol.name.localeCompare(right.symbol.name) ||
		left.symbol.uri.localeCompare(right.symbol.uri),
	);
}

export function selectInitialCodeNeighbors(
	incoming: readonly SymbolGraphNeighbor[],
	outgoing: readonly SymbolGraphNeighbor[],
	focus: SymbolDto,
	budget = INITIAL_RELATION_BUDGET,
	directionFloor = INITIAL_DIRECTION_FLOOR,
): { incoming: SymbolGraphNeighbor[]; outgoing: SymbolGraphNeighbor[] } {
	const selectedIncoming: SymbolGraphNeighbor[] = [];
	const selectedOutgoing: SymbolGraphNeighbor[] = [];
	const selectedUris = new Set<string>();
	for (const row of incoming) {
		if (selectedIncoming.length >= directionFloor || selectedUris.size >= budget) break;
		if (selectedUris.has(row.symbol.uri)) continue;
		selectedIncoming.push(row);
		selectedUris.add(row.symbol.uri);
	}
	for (const row of outgoing) {
		if (selectedOutgoing.length >= directionFloor || selectedUris.size >= budget) break;
		if (selectedUris.has(row.symbol.uri)) continue;
		selectedOutgoing.push(row);
		selectedUris.add(row.symbol.uri);
	}
	const remainder = [
		...incoming.map((row) => ({ direction: "incoming" as const, row })),
		...outgoing.map((row) => ({ direction: "outgoing" as const, row })),
	].sort((left, right) =>
		codeNeighborScore(right.row, focus) - codeNeighborScore(left.row, focus) ||
		left.row.symbol.name.localeCompare(right.row.symbol.name),
	);
	for (const candidate of remainder) {
		if (selectedIncoming.length + selectedOutgoing.length >= budget) break;
		if (selectedUris.has(candidate.row.symbol.uri)) continue;
		selectedUris.add(candidate.row.symbol.uri);
		if (candidate.direction === "incoming") selectedIncoming.push(candidate.row);
		else selectedOutgoing.push(candidate.row);
	}
	return { incoming: selectedIncoming, outgoing: selectedOutgoing };
}

export function codeNeighborScore(row: SymbolGraphNeighbor, focus: SymbolDto): number {
	const relation = Math.max(1, ...row.kinds.map(relationWeight));
	const frequency = Math.log2(Math.max(1, row.count) + 1) * 8;
	const locality = row.symbol.root === focus.root && row.symbol.file === focus.file ? 12 : 0;
	const sameLanguage = row.symbol.language === focus.language ? 2 : 0;
	return relation + frequency + locality + sameLanguage;
}

function relationWeight(kind: string): number {
	if (kind === "calls" || kind === "method_call") return 100;
	if (kind === "writes") return 92;
	if (kind === "reads") return 82;
	if (kind === "instantiates") return 76;
	if (kind === "uses_type" || kind === "returns_type") return 64;
	return 32;
}
