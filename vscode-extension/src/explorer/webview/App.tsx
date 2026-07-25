import { Fragment, useCallback, useEffect, useState } from "react";

import type { IdentityGraphEdge } from "../../daemon/model";
import type { InsetMessage, ScopePayload } from "../protocol";
import { ancestors, parentPrefix, segmentName } from "../../shared/identity";
import { postFocus, postInspect } from "./actions";
import { CodeInset, type InsetState } from "./CodeInset";
import { EdgePanel } from "./EdgePanel";
import { ScopeCanvas } from "./ScopeCanvas";
import type { ScopeFilters } from "./graph/model";
import { vscode } from "./vscodeApi";

// Scoped exploration of the identity graph: clickable breadcrumb in the
// toolbar, the current level's rolled-up graph on the canvas, edge facts in
// a corner panel. Keyboard: Backspace climbs to the parent scope, Alt+←/→
// walk history. Double-click on the background climbs too.
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

	// Keyed on the prefix, not the scope object: an outline update replaces
	// the scope without moving it, and must not re-bind the key listener.
	const prefix = scope?.graph.prefix;
	const climb = useCallback(() => {
		if (prefix) {
			postFocus(parentPrefix(prefix));
		}
	}, [prefix]);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Backspace") {
				climb();
			} else if (event.altKey && event.key === "ArrowLeft") {
				vscode.postMessage({ type: "back" });
			} else if (event.altKey && event.key === "ArrowRight") {
				vscode.postMessage({ type: "forward" });
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [climb]);

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
				Select a scope in the Code Moniker view, then use the graph button in its toolbar.
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
				<Breadcrumb prefix={graph.prefix} />
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
				<span
					className="scope-facts"
					title={`${graph.nodes.length} nodes · ${graph.edges.length} rolled-up edges`}
				>
					{graph.nodes.length} ▪ {graph.edges.length}
				</span>
				{graph.unlinked.unresolved > 0 && (
					<span
						className="unresolved"
						title={`References the index could not resolve (external: ${graph.unlinked.external} · manifest-blocked: ${graph.unlinked.manifest_blocked})`}
					>
						▲ {graph.unlinked.unresolved}
					</span>
				)}
			</div>
			<div className="canvas-zone">
				<ScopeCanvas
					graph={graph}
					filters={filters}
					outline={scope.outline}
					onSelectEdge={setSelectedEdge}
					onClimb={climb}
					onInspect={(uri) => {
						setInset({ uri, symbol: null, source: null, loading: true });
						postInspect(uri);
					}}
				/>
				{selectedEdge && <EdgePanel edge={selectedEdge} onClose={() => setSelectedEdge(null)} />}
				{inset && <CodeInset inset={inset} onClose={() => setInset(null)} />}
			</div>
		</>
	);
}

// The breadcrumb is the depth control: one clickable step per ancestor, the
// current scope highlighted. It replaces the old vertical depth rail. The
// separator is its own element so the current step's highlight wraps the
// name alone, not the chevron before it.
function Breadcrumb({ prefix }: { prefix: string }) {
	const steps = ["", ...ancestors(prefix)];
	return (
		<nav className="breadcrumb" aria-label="Scope path">
			{steps.map((identity, index) => {
				const current = index === steps.length - 1;
				return (
					<Fragment key={identity}>
						{index > 0 && <span className="crumb-sep">›</span>}
						<button
							type="button"
							className={current ? "crumb-btn current" : "crumb-btn"}
							disabled={current}
							title={identity || "Workspace root"}
							onClick={() => postFocus(identity)}
						>
							{identity ? segmentName(identity) : "workspace"}
						</button>
					</Fragment>
				);
			})}
		</nav>
	);
}
