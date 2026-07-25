import { BaseEdge, EdgeLabelRenderer, type EdgeProps } from "@xyflow/react";

import type { Point } from "./layout";

export interface RoutedEdgeData {
	points: Point[];
	count: number;
	[key: string]: unknown;
}

const CORNER_RADIUS = 9;

// Draws the polyline ELK routed around the cards instead of letting React
// Flow guess a curve. Right angles with softened corners stay followable
// where a dense level of beziers turns into spaghetti.
export function RoutedEdge({
	id,
	data,
	markerEnd,
	style,
	sourceX,
	sourceY,
	targetX,
	targetY,
}: EdgeProps) {
	const { points: routed = [], count = 1 } = (data ?? {}) as Partial<RoutedEdgeData>;
	// ELK returns a section for every edge it routes; the straight fallback
	// only covers a layout that came back without one.
	const points =
		routed.length >= 2
			? routed
			: [
					{ x: sourceX, y: sourceY },
					{ x: targetX, y: targetY },
				];
	const label = labelAnchor(points);
	return (
		<>
			{/* BaseEdge also lays a transparent fat path over the stroke: a
			    1.1px routed line is otherwise nearly impossible to click, and
			    clicking is the only way to open the edge facts. */}
			<BaseEdge id={id} path={roundedPath(points)} markerEnd={markerEnd} style={style} />
			{count > 1 && (
				<EdgeLabelRenderer>
					<div
						className="edge-label"
						style={{ transform: `translate(-50%, -50%) translate(${label.x}px, ${label.y}px)` }}
					>
						×{count}
					</div>
				</EdgeLabelRenderer>
			)}
		</>
	);
}

// Midpoint of the longest segment: the label lands on a straight run rather
// than on a corner, where it would sit on top of the bend.
function labelAnchor(points: Point[]): Point {
	let best = midpoint(points[0], points[1]);
	let bestLength = Math.hypot(points[1].x - points[0].x, points[1].y - points[0].y);
	for (let index = 2; index < points.length; index++) {
		const from = points[index - 1];
		const to = points[index];
		const length = Math.hypot(to.x - from.x, to.y - from.y);
		if (length > bestLength) {
			bestLength = length;
			best = midpoint(from, to);
		}
	}
	return best;
}

function midpoint(from: Point, to: Point): Point {
	return { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 };
}

function roundedPath(points: Point[]): string {
	let path = `M ${points[0].x},${points[0].y}`;
	for (let index = 1; index < points.length - 1; index++) {
		const previous = points[index - 1];
		const corner = points[index];
		const next = points[index + 1];
		const radius = Math.min(
			CORNER_RADIUS,
			Math.hypot(corner.x - previous.x, corner.y - previous.y) / 2,
			Math.hypot(next.x - corner.x, next.y - corner.y) / 2,
		);
		const entry = towards(corner, previous, radius);
		const exit = towards(corner, next, radius);
		path += ` L ${entry.x},${entry.y} Q ${corner.x},${corner.y} ${exit.x},${exit.y}`;
	}
	const last = points[points.length - 1];
	return `${path} L ${last.x},${last.y}`;
}

function towards(from: Point, to: Point, distance: number): Point {
	const length = Math.hypot(to.x - from.x, to.y - from.y);
	if (length === 0) {
		return from;
	}
	return {
		x: from.x + ((to.x - from.x) / length) * distance,
		y: from.y + ((to.y - from.y) / length) * distance,
	};
}
