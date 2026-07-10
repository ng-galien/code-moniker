import type { UnitPayload } from "../protocol";
import { postFocus } from "./actions";
import { vscode } from "./vscodeApi";

export function Toolbar({ unit }: { unit: UnitPayload }) {
	const focus = unit.focus;
	return (
		<div className="toolbar">
			<button
				type="button"
				className="nav"
				disabled={!unit.canBack}
				onClick={() => vscode.postMessage({ type: "back" })}
			>
				←
			</button>
			<button
				type="button"
				className="nav"
				disabled={!unit.canForward}
				onClick={() => vscode.postMessage({ type: "forward" })}
			>
				→
			</button>
			{focus.kind === "symbol" && (
				<>
					<span className="crumb link" onClick={() => postFocus(focus.symbol.file)}>
						{focus.symbol.file}
					</span>
					<span className="crumb-sep">▸</span>
				</>
			)}
			<span className="focus-label">
				{focus.kind === "symbol"
					? `${focus.symbol.kind} ${focus.symbol.name}`
					: `file ${focus.path}`}
			</span>
			{unit.unresolvedRefs > 0 && (
				<span className="unresolved">{unit.unresolvedRefs} unresolved ref(s)</span>
			)}
		</div>
	);
}
