import type { IdentityGraphEdge } from "../../daemon/model";
import { segmentName } from "../../shared/identity";
import { postFocus } from "./actions";

// Facts of a selected rolled-up edge, floating in a fixed corner over the
// canvas: endpoints, relation kinds, volume, dive shortcuts. A fixed corner
// beats anchoring at the click — an anchored panel overflows the canvas and
// sits on top of the graph it describes. Selection never reflows the graph.
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
