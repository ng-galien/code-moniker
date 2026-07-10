import type { IdentityGraphEdge } from "../../daemon/model";
import { postFocus } from "./actions";
import { segmentName } from "./graph/model";

// Facts of a selected rolled-up edge: endpoints, relation kinds, volume, and
// dive shortcuts. Constituent pairs come one level deeper.
export function EdgePanel({ edge }: { edge: IdentityGraphEdge }) {
	return (
		<div className="edgepanel">
			<div className="edgepanel-title">
				{segmentName(edge.source)} ⟶ {segmentName(edge.target)}
				<span className="edgepanel-count">×{edge.count}</span>
			</div>
			<div className="edgepanel-kinds">{edge.kinds.join(" · ")}</div>
			<div className="edgepanel-actions">
				<button type="button" onClick={() => postFocus(edge.source)}>
					dive into {segmentName(edge.source)}
				</button>
				<button type="button" onClick={() => postFocus(edge.target)}>
					dive into {segmentName(edge.target)}
				</button>
			</div>
		</div>
	);
}
