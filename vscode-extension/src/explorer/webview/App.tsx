import { useEffect, useState } from "react";

import type { IdentityGraphEdge } from "../../daemon/model";
import type { ScopePayload } from "../protocol";
import { DepthBar } from "./DepthBar";
import { EdgePanel } from "./EdgePanel";
import { ScopeCanvas } from "./ScopeCanvas";
import type { ScopeFilters } from "./graph/model";
import { vscode } from "./vscodeApi";

// Scoped exploration of the identity graph: depth ladder on the left, the
// current level's rolled-up graph on the canvas, edge facts on demand.
export function App() {
	const [scope, setScope] = useState<ScopePayload | null>(null);
	const [filters, setFilters] = useState<ScopeFilters>({ instantiates: false, types: false });
	const [selectedEdge, setSelectedEdge] = useState<IdentityGraphEdge | null>(null);
	const [error, setError] = useState<{ prefix: string; message: string } | null>(null);

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			const message = event.data;
			if (message?.type === "scope") {
				setScope(message.payload as ScopePayload);
				setSelectedEdge(null);
				setError(null);
			} else if (message?.type === "scopeError") {
				setError({ prefix: message.prefix as string, message: message.message as string });
			}
		};
		window.addEventListener("message", onMessage);
		vscode.postMessage({ type: "ready" });
		return () => window.removeEventListener("message", onMessage);
	}, []);

	if (error) {
		return (
			<div className="empty">
				<div>scope query failed: {error.message}</div>
				<button
					type="button"
					className="nav"
					style={{ marginTop: 8 }}
					onClick={() => vscode.postMessage({ type: "focus", prefix: error.prefix })}
				>
					retry
				</button>
			</div>
		);
	}
	if (!scope) {
		return <div className="empty">Open a scope to explore its graph.</div>;
	}
	const graph = scope.graph;
	return (
		<>
			<div className="toolbar">
				<button
					type="button"
					className="nav"
					disabled={!scope.canBack}
					onClick={() => vscode.postMessage({ type: "back" })}
				>
					←
				</button>
				<button
					type="button"
					className="nav"
					disabled={!scope.canForward}
					onClick={() => vscode.postMessage({ type: "forward" })}
				>
					→
				</button>
				<span className="focus-label">{graph.prefix || "workspace"}</span>
				<span className="filterchip on">calls</span>
				<span
					className={filters.instantiates ? "filterchip on toggle" : "filterchip toggle"}
					onClick={() => setFilters({ ...filters, instantiates: !filters.instantiates })}
				>
					instantiates
				</span>
				<span
					className={filters.types ? "filterchip on toggle" : "filterchip toggle"}
					onClick={() => setFilters({ ...filters, types: !filters.types })}
				>
					types
				</span>
				{graph.unresolved_refs > 0 && (
					<span className="unresolved">{graph.unresolved_refs} unresolved ref(s)</span>
				)}
			</div>
			<div className="scope-layout">
				<DepthBar prefix={graph.prefix} />
				<ScopeCanvas graph={graph} filters={filters} onSelectEdge={setSelectedEdge} />
				{selectedEdge && <EdgePanel edge={selectedEdge} />}
			</div>
		</>
	);
}
