import { postFocus } from "./actions";
import { segmentName } from "./graph/model";

// The depth ladder: the current scope stays on top, ancestors stack below it
// down to the workspace root. Diving pushes a new step on top; clicking any
// ancestor climbs straight back to it.
export function DepthBar({ prefix }: { prefix: string }) {
	const segments = prefix ? prefix.split("/") : [];
	const steps = segments.map((_, index) => segments.slice(0, index + 1).join("/")).reverse();
	return (
		<div className="depthbar">
			<div className="depthbar-label">depth</div>
			{steps.map((identity, index) => (
				<div
					key={identity}
					className={index === 0 ? "depth-step current" : "depth-step"}
					title={identity}
					onClick={() => index !== 0 && postFocus(identity)}
				>
					{segmentName(identity)}
				</div>
			))}
			<div
				className={steps.length === 0 ? "depth-step current" : "depth-step root"}
				onClick={() => steps.length !== 0 && postFocus("")}
			>
				⌂ workspace
			</div>
		</div>
	);
}
