import type { IdentityGraphEdge } from "../../daemon/model";
import { postFocus } from "./actions";
import { segmentName } from "./graph/model";

// Facts of a selected rolled-up edge, floating over the canvas so selection
// never reflows the graph: endpoints, relation kinds, volume, dive shortcuts.
export function EdgePanel({
	edge,
	onClose,
}: {
	edge: IdentityGraphEdge;
	onClose: () => void;
}) {
	return (
		<aside className="edgepanel" aria-label="Edge facts">
			<div className="edgepanel-title">
				<span>
					{segmentName(edge.source)} <span className="edgepanel-arrow">⟶</span>{" "}
					{segmentName(edge.target)}
				</span>
				<span className="edgepanel-count">×{edge.count}</span>
				<button type="button" className="edgepanel-close" title="Close" onClick={onClose}>
					✕
				</button>
			</div>
			<div className="edgepanel-kinds">{edge.kinds.join(" · ")}</div>
			<div className="edgepanel-actions">
				<button type="button" onClick={() => postFocus(edge.source)}>
					Dive into {segmentName(edge.source)}
				</button>
				<button type="button" onClick={() => postFocus(edge.target)}>
					Dive into {segmentName(edge.target)}
				</button>
			</div>
		</aside>
	);
}
