import { useEffect, useState } from "react";

import type { UnitPayload } from "../protocol";
import { Center } from "./Center";
import { NeighborColumn } from "./NeighborColumn";
import { Toolbar } from "./Toolbar";
import { vscode } from "./vscodeApi";

// Graph explorer webview: renders the ego-centric triptych from posted
// "unit" messages. Facts only; clicks post focus/openSource messages back.
export function App() {
	const [unit, setUnit] = useState<UnitPayload | null>(null);

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			const message = event.data;
			if (message?.type === "unit") {
				setUnit(message.payload as UnitPayload);
			}
		};
		window.addEventListener("message", onMessage);
		vscode.postMessage({ type: "ready" });
		return () => window.removeEventListener("message", onMessage);
	}, []);

	if (!unit) {
		return <div className="empty">Focus a symbol to explore its call graph.</div>;
	}
	return (
		<>
			<Toolbar unit={unit} />
			<div className="triptych">
				<NeighborColumn title="Callers" side="left" neighbors={unit.callers} unit={unit} />
				<Center unit={unit} />
				<NeighborColumn title="Callees" side="right" neighbors={unit.callees} unit={unit} />
			</div>
		</>
	);
}
