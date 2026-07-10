import { Handle, Position } from "@xyflow/react";

import type { ScopeNodeModel } from "./model";
import { segmentName } from "./model";

// A scope-level container (package, dir, module, srcset): it reads as
// enterable — stacked outline, and a dive hint surfaces on hover.
export function ContainerCard({ data }: { data: { node: ScopeNodeModel } }) {
	const node = data.node;
	return (
		<div className="containercard" title="Double-click to dive in">
			<Handle type="target" position={Position.Top} className="port" />
			<div className="containercard-head">
				<span className="containercard-name">{segmentName(node.id)}</span>
				<span className="containercard-dive" aria-hidden="true">
					⤵
				</span>
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
