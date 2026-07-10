import { Handle, Position } from "@xyflow/react";

import type { ScopeNodeModel } from "./model";
import { segmentName } from "./model";

// A scope-level container (package, dir, module, srcset): name dominant,
// kind and def count as facts, in/out rolled-up call volumes. Double-click
// dives into it.
export function ContainerCard({ data }: { data: { node: ScopeNodeModel } }) {
	const node = data.node;
	return (
		<div className="containercard">
			<Handle type="target" position={Position.Top} className="port" />
			<div className="containercard-head">
				<span className="containercard-mark">▸</span>
				<span className="containercard-name">{segmentName(node.id)}</span>
			</div>
			<div className="containercard-meta">
				{node.row.kind} · {node.row.defs} defs
			</div>
			<div className="fncard-degrees">
				{node.callsIn > 0 && <span className="deg-in">⟵ {node.callsIn}</span>}
				{node.callsOut > 0 && <span className="deg-out">⟶ {node.callsOut}</span>}
			</div>
			<Handle type="source" position={Position.Bottom} className="port" />
		</div>
	);
}
