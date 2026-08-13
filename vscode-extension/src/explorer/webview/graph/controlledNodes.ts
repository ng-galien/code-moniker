import type { Node } from "@xyflow/react";

/** Keep React Flow's interactive coordinates authoritative across data updates. */
export function reconcileControlledNodes<NodeData extends Record<string, unknown>>(
	current: Array<Node<NodeData>>,
	next: Array<Node<NodeData>>,
	resetLayout: boolean,
): Array<Node<NodeData>> {
	if (resetLayout) return next;
	const currentById = new Map(current.map((node) => [node.id, node]));
	return next.map((node) => {
		const interactive = currentById.get(node.id);
		if (!interactive) return node;
		return {
			...interactive,
			...node,
			position: interactive.position,
			dragging: interactive.dragging,
			selected: interactive.selected,
		};
	});
}
