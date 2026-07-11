import { useEffect, useState } from "react";

import type { IdentityGraphEdge } from "../../daemon/model";
import type { InsetMessage, ScopePayload } from "../protocol";
import { postFocus, postInspect } from "./actions";
import { CodeInset, type InsetState } from "./CodeInset";
import { DepthBar } from "./DepthBar";
import { EdgePanel } from "./EdgePanel";
import { ScopeCanvas } from "./ScopeCanvas";
import { segmentName, type ScopeFilters } from "./graph/model";
import { vscode } from "./vscodeApi";

// Scoped exploration of the identity graph: depth ladder on the left, the
// current level's rolled-up graph on the canvas, edge facts on demand.
// Keyboard: Backspace climbs to the parent scope, Alt+←/→ walk history.
export function App() {
	const [scope, setScope] = useState<ScopePayload | null>(null);
	const [filters, setFilters] = useState<ScopeFilters>({ instantiates: false, types: false });
	const [selectedEdge, setSelectedEdge] = useState<IdentityGraphEdge | null>(null);
	const [inset, setInset] = useState<InsetState | null>(null);
	const [error, setError] = useState<{ prefix: string; message: string } | null>(null);

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			const message = event.data;
			if (message?.type === "scope") {
				const payload = message.payload as ScopePayload;
				setScope(payload);
				setSelectedEdge(null);
				setInset(null);
				setError(null);
				vscode.postMessage({
					type: "ack",
					prefix: payload.graph.prefix,
					nodes: payload.graph.nodes.length,
				});
			} else if (message?.type === "inset") {
				const payload = message as InsetMessage;
				setInset((current) =>
					current && current.uri === payload.uri
						? { uri: payload.uri, symbol: payload.symbol, source: payload.source, loading: false }
						: current,
				);
				vscode.postMessage({
					type: "insetAck",
					uri: payload.uri,
					lines: payload.source ? payload.source.lines.length : 0,
				});
			} else if (message?.type === "scopeError") {
				setError({ prefix: message.prefix as string, message: message.message as string });
			}
		};
		window.addEventListener("message", onMessage);
		vscode.postMessage({ type: "ready" });
		return () => window.removeEventListener("message", onMessage);
	}, []);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Backspace" && scope) {
				const prefix = scope.graph.prefix;
				if (prefix) {
					postFocus(prefix.includes("/") ? prefix.slice(0, prefix.lastIndexOf("/")) : "");
				}
			} else if (event.altKey && event.key === "ArrowLeft") {
				vscode.postMessage({ type: "back" });
			} else if (event.altKey && event.key === "ArrowRight") {
				vscode.postMessage({ type: "forward" });
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [scope]);

	if (error) {
		return (
			<div className="empty">
				<div>Scope query failed: {error.message}</div>
				<button
					type="button"
					className="nav"
					style={{ marginTop: 8 }}
					onClick={() => vscode.postMessage({ type: "focus", prefix: error.prefix })}
				>
					Retry
				</button>
			</div>
		);
	}
	if (!scope) {
		return (
			<div className="empty">
				Open a scope from the Symbols tree (right-click → Open in Graph Explorer) or place the
				cursor in code and run “Focus Symbol at Cursor”.
			</div>
		);
	}
	const graph = scope.graph;
	return (
		<>
			<div className="toolbar">
				<button
					type="button"
					className="nav"
					title="Back (Alt+←)"
					disabled={!scope.canBack}
					onClick={() => vscode.postMessage({ type: "back" })}
				>
					←
				</button>
				<button
					type="button"
					className="nav"
					title="Forward (Alt+→)"
					disabled={!scope.canForward}
					onClick={() => vscode.postMessage({ type: "forward" })}
				>
					→
				</button>
				<span className="focus-label">{graph.prefix ? segmentName(graph.prefix) : "workspace"}</span>
				<span className="scope-facts">
					{graph.nodes.length} nodes · {graph.edges.length} edges
				</span>
				<span className="filter-group" role="group" aria-label="Relations">
					<span className="filterchip fixed" title="Calls always draw">
						calls
					</span>
					<button
						type="button"
						className={filters.instantiates ? "filterchip on toggle" : "filterchip toggle"}
						aria-pressed={filters.instantiates}
						onClick={() => setFilters({ ...filters, instantiates: !filters.instantiates })}
					>
						instantiates
					</button>
					<button
						type="button"
						className={filters.types ? "filterchip on toggle" : "filterchip toggle"}
						aria-pressed={filters.types}
						onClick={() => setFilters({ ...filters, types: !filters.types })}
					>
						types
					</button>
				</span>
				{graph.unresolved_refs > 0 && (
					<span className="unresolved" title="References the index could not resolve">
						{graph.unresolved_refs} unresolved
					</span>
				)}
			</div>
			<div className="scope-layout">
				<DepthBar prefix={graph.prefix} />
				<div className="canvas-zone">
					<ScopeCanvas
						graph={graph}
						filters={filters}
						onSelectEdge={setSelectedEdge}
						onInspect={(uri) => {
							setInset({ uri, symbol: null, source: null, loading: true });
							postInspect(uri);
						}}
					/>
					{selectedEdge && (
						<EdgePanel edge={selectedEdge} onClose={() => setSelectedEdge(null)} />
					)}
					{inset && <CodeInset inset={inset} onClose={() => setInset(null)} />}
				</div>
			</div>
		</>
	);
}
