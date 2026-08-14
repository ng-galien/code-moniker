import type { Edge, Node } from "@xyflow/react";

import type { CockpitCardData } from "./CockpitCard";

export interface CockpitPosition {
	x: number;
	y: number;
}

const COLUMN_STEP = 380;
const ROW_STEP = 140;
const CARD_WIDTH = 268;
const CARD_HEIGHT = 118;

// Direction is the cockpit's primary spatial language: callers remain left of
// the focus and dependencies remain right of it. Existing positions always win
// so revealing another batch never rewrites the map the user has just learned.
export function layoutCockpit(
	nodes: readonly Node<CockpitCardData>[],
	edges: readonly Edge[],
	focus: string,
	saved: Readonly<Record<string, CockpitPosition>>,
): Record<string, CockpitPosition> {
	if (!focus) return {};
	const positions: Record<string, CockpitPosition> = { ...saved };
	positions[focus] ??= { x: 0, y: 0 };
	const origin = positions[focus];
	const depths = directionalDepths(nodes, edges, focus);
	const layers = new Map<number, Node<CockpitCardData>[]>();

	for (const node of nodes) {
		if (node.id === focus || positions[node.id]) continue;
		const depth = depths.get(node.id) ?? (node.data.pinned ? 0 : 2);
		layers.set(depth, [...(layers.get(depth) ?? []), node]);
	}

	for (const [depth, layer] of [...layers].sort(([left], [right]) => left - right)) {
		const ordered = [...layer].sort((left, right) => {
			const leftAnchor = parentAnchor(left.id, depth, edges, positions);
			const rightAnchor = parentAnchor(right.id, depth, edges, positions);
			return (
				(leftAnchor ?? Number.MAX_SAFE_INTEGER) - (rightAnchor ?? Number.MAX_SAFE_INTEGER) ||
				left.data.symbol.name.localeCompare(right.data.symbol.name) ||
				left.id.localeCompare(right.id)
			);
		});
		const start = -((ordered.length - 1) * ROW_STEP) / 2;
		ordered.forEach((node, index) => {
			const anchor = parentAnchor(node.id, depth, edges, positions);
			const preferred = depth === 0
				? { x: origin.x + (index - (ordered.length - 1) / 2) * 280, y: origin.y + 330 }
				: {
					x: origin.x + depth * COLUMN_STEP,
					y: anchor ?? origin.y + start + index * ROW_STEP,
				};
			positions[node.id] = avoidCollision(preferred, positions);
		});
	}

	return positions;
}

function directionalDepths(
	nodes: readonly Node<CockpitCardData>[],
	edges: readonly Edge[],
	focus: string,
): Map<string, number> {
	const visible = new Set(nodes.map((node) => node.id));
	const depths = new Map<string, number>([[focus, 0]]);
	for (const direction of [-1, 1] as const) {
		let frontier = [focus];
		for (let distance = 1; distance <= 4 && frontier.length > 0; distance++) {
			const next = new Set<string>();
			for (const identity of frontier) {
				for (const edge of edges) {
					const candidate = direction < 0 && edge.target === identity
						? edge.source
						: direction > 0 && edge.source === identity
							? edge.target
							: undefined;
					if (!candidate || !visible.has(candidate) || depths.has(candidate)) continue;
					depths.set(candidate, direction * distance);
					next.add(candidate);
				}
			}
			frontier = [...next];
		}
	}
	return depths;
}

function parentAnchor(
	identity: string,
	depth: number,
	edges: readonly Edge[],
	positions: Readonly<Record<string, CockpitPosition>>,
): number | undefined {
	if (depth === 0) return undefined;
	const parents = edges.flatMap((edge) => {
		if (depth < 0 && edge.source === identity) return [edge.target];
		if (depth > 0 && edge.target === identity) return [edge.source];
		return [];
	});
	const anchors = parents.flatMap((parent) => positions[parent] ? [positions[parent].y] : []);
	if (anchors.length === 0) return undefined;
	return anchors.reduce((sum, value) => sum + value, 0) / anchors.length;
}

function avoidCollision(
	preferred: CockpitPosition,
	positions: Readonly<Record<string, CockpitPosition>>,
): CockpitPosition {
	const occupied = Object.values(positions);
	for (let attempt = 0; attempt < occupied.length * 2 + 6; attempt++) {
		const distance = Math.ceil(attempt / 2) * ROW_STEP;
		const offset = attempt === 0 ? 0 : attempt % 2 === 1 ? distance : -distance;
		const candidate = { x: preferred.x, y: preferred.y + offset };
		if (occupied.every((position) =>
			Math.abs(position.x - candidate.x) > CARD_WIDTH ||
			Math.abs(position.y - candidate.y) > CARD_HEIGHT,
		)) {
			return candidate;
		}
	}
	return preferred;
}
