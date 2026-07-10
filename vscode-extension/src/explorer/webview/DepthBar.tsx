import { postFocus } from "./actions";
import { segmentName } from "./graph/model";

// The drill core: a continuous rail with one notch per identity level. The
// current scope stays on top as the lens; ancestors stack below down to the
// workspace root. Diving pushes a new step on top; clicking any ancestor
// climbs straight back to it.
export function DepthBar({ prefix }: { prefix: string }) {
	const segments = prefix ? prefix.split("/") : [];
	const steps = segments.map((_, index) => segments.slice(0, index + 1).join("/")).reverse();
	return (
		<nav className="depthbar" aria-label="Scope depth">
			<div className="depthbar-label">depth</div>
			<div className="depthbar-core">
				{steps.map((identity, index) => (
					<button
						key={identity}
						type="button"
						className={index === 0 ? "depth-step current" : "depth-step"}
						title={index === 0 ? identity : `Climb to ${identity}`}
						disabled={index === 0}
						onClick={() => postFocus(identity)}
					>
						<span className="depth-name">{segmentName(identity)}</span>
						<span className="depth-kind">
							{index === 0 ? `${segmentKind(identity)} · current` : segmentKind(identity)}
						</span>
					</button>
				))}
				<button
					type="button"
					className={steps.length === 0 ? "depth-step current" : "depth-step root"}
					disabled={steps.length === 0}
					title="Climb to the workspace root"
					onClick={() => postFocus("")}
				>
					<span className="depth-name">workspace</span>
					<span className="depth-kind">{steps.length === 0 ? "root · current" : "root"}</span>
				</button>
			</div>
			<div className="depthbar-hint">double-click a card to dive · backspace to climb</div>
		</nav>
	);
}

function segmentKind(identity: string): string {
	const segment = identity.split("/").pop() ?? identity;
	const kind = segment.split(":")[0];
	return kind || "scope";
}
