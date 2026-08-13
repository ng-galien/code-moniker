import {
	BaseEdge,
	EdgeLabelRenderer,
	getBezierPath,
	type EdgeProps,
	useStore,
} from "@xyflow/react";

import type { CockpitRelation } from "../../protocol";
import { showCockpitEdgeLabel } from "./zoom";

export interface CockpitEdgeData extends Record<string, unknown> {
	relation: CockpitRelation;
	relations: CockpitRelation[];
	relationLabels: Partial<Record<CockpitRelation, string>>;
	label: string;
	showLabel: boolean;
}

export function CockpitEdge(props: EdgeProps) {
	const data = props.data as CockpitEdgeData | undefined;
	const showLabel = useStore((state) => showCockpitEdgeLabel(state.transform[2]));
	const [path, labelX, labelY] = getBezierPath({ ...props, curvature: 0.32 });
	const stroke = relationStroke(data?.relation ?? "references");
	return (
		<>
			<BaseEdge
				id={props.id}
				path={path}
				markerEnd={props.markerEnd}
				className={`cockpit-edge-path ${data?.relation ?? "references"}`}
				style={{ ...props.style, stroke, strokeOpacity: props.selected ? 1 : 0.86 }}
				interactionWidth={24}
			/>
			{data?.showLabel && (showLabel || props.selected) && (
				<EdgeLabelRenderer>
					<span
						className={`cockpit-edge-label nodrag nopan ${props.selected ? "selected" : ""}`}
						style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
					>
						{data.label}
					</span>
				</EdgeLabelRenderer>
			)}
		</>
	);
}

function relationStroke(relation: CockpitRelation): string {
	switch (relation) {
		case "calls": return "var(--vscode-charts-orange)";
		case "data": return "var(--vscode-charts-green)";
		case "types": return "var(--vscode-charts-purple)";
		case "references": return "var(--vscode-charts-blue)";
	}
}
