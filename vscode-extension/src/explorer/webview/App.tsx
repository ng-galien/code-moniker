import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";

import type { SymbolDto } from "../../daemon/model";
import type {
	CockpitExpansionMessage,
	CockpitExpansionErrorMessage,
	CockpitContext,
	CockpitFilters,
	CockpitPayload,
	CockpitPerspective,
	CockpitPosition,
	CockpitPreferences,
	CockpitRadius,
	CockpitRelation,
	CockpitViewport,
	ExternalSelectionMessage,
	InsetMessage,
} from "../protocol";
import { glyphClass, symbolGlyph } from "../../webview-lib/symbolGlyph";
import { postFocus, postInspect } from "./actions";
import { CockpitCanvas, type CockpitSessionSnapshot } from "./CockpitCanvas";
import { CodeInset, type InsetState } from "./CodeInset";
import { vscode } from "./vscodeApi";

export function App() {
	const [cockpit, setCockpit] = useState<CockpitPayload | null>(null);
	const [inset, setInset] = useState<InsetState | null>(null);
	const [error, setError] = useState<{ prefix: string; message: string } | null>(null);
	const [loading, setLoading] = useState(false);
	const [expansion, setExpansion] = useState<CockpitExpansionMessage | null>(null);
	const [expansionError, setExpansionError] = useState<CockpitExpansionErrorMessage | null>(null);
	const [cockpitPerspective, setCockpitPerspective] = useState<CockpitPerspective>("neighborhood");
	const [cockpitFilters, setCockpitFilters] = useState<CockpitFilters>({
		incoming: true,
		outgoing: true,
		calls: true,
		data: true,
		types: true,
		references: true,
	});
	const [cockpitRadius, setCockpitRadius] = useState<CockpitRadius>({ incoming: 1, outgoing: 1 });
	const [pinned, setPinned] = useState<SymbolDto[]>([]);
	const [selectedUri, setSelectedUri] = useState<string | null>(null);
	const [externalSelection, setExternalSelection] = useState<ExternalSelectionMessage | null>(null);
	const [query, setQuery] = useState("");
	const [results, setResults] = useState<SymbolDto[]>([]);
	const [searchPending, setSearchPending] = useState(false);
	const [recenterToken, setRecenterToken] = useState(0);
	const [inspectorWidth, setInspectorWidth] = useState(420);
	const [context, setContext] = useState<CockpitContext | undefined>();
	const [cockpitPositions, setCockpitPositions] = useState<Record<string, CockpitPosition>>({});
	const [viewportCommand, setViewportCommand] = useState<{ commandId: number; viewport: CockpitViewport } | null>(null);
	const [renderProbeToken, setRenderProbeToken] = useState(0);
	const queryRef = useRef(query);
	const cockpitSessions = useRef(new Map<string, CockpitSessionSnapshot>());
	const cockpitRef = useRef<CockpitPayload | null>(cockpit);
	const insetRef = useRef<InsetState | null>(inset);
	queryRef.current = query;
	cockpitRef.current = cockpit;
	insetRef.current = inset;

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			const message = event.data;
			if (message?.type === "cockpitEmpty") {
				cockpitRef.current = null;
				insetRef.current = null;
				setCockpit(null);
				setContext(message.context as CockpitContext | undefined);
				setInset(null);
				setError(null);
				setLoading(false);
				setExpansion(null);
				setExpansionError(null);
				setViewportCommand(null);
				window.requestAnimationFrame(() => vscode.postMessage({
					type: "ack",
					prefix: "",
					nodes: 0,
					mode: "cockpit",
				}));
			} else if (message?.type === "cockpitLoading") {
				setLoading(true);
				setError(null);
			} else if (message?.type === "cockpit") {
				const payload = message.payload as CockpitPayload;
				const priorFocus = cockpitRef.current?.graph.focus.kind === "symbol"
					? cockpitRef.current.graph.focus.symbol.uri
					: undefined;
				const nextFocus = payload.graph.focus.kind === "symbol" ? payload.graph.focus.symbol.uri : undefined;
				const inspectorOpen = insetRef.current !== null;
				cockpitRef.current = payload;
				setCockpit(payload);
				setContext(payload.context);
				setCockpitPerspective(payload.preferences.perspective);
				setCockpitFilters(payload.preferences.filters);
				setCockpitRadius(payload.preferences.radius);
				setCockpitPositions(payload.preferences.positions);
				setPinned(payload.pinned);
				setSelectedUri(payload.graph.focus.kind === "symbol" ? payload.graph.focus.symbol.uri : null);
				setExternalSelection(null);
				if (inspectorOpen && nextFocus && priorFocus !== nextFocus) {
					const nextInset = { uri: nextFocus, symbol: null, source: null, loading: true };
					insetRef.current = nextInset;
					setInset(nextInset);
					postInspect(nextFocus);
				} else if (!inspectorOpen) {
					insetRef.current = null;
					setInset(null);
				} else if (insetRef.current) {
					acknowledgeInset(insetRef.current, "preserved");
				}
				setError(null);
				setLoading(false);
				setExpansion(null);
				setExpansionError(null);
				setViewportCommand(null);
				setRenderProbeToken((value) => value + 1);
			} else if (message?.type === "cockpitExpansion") {
				setExpansion(message as CockpitExpansionMessage);
				setExpansionError(null);
			} else if (message?.type === "cockpitExpansionError") {
				setExpansionError(message as CockpitExpansionErrorMessage);
			} else if (message?.type === "cockpitInspect") {
				const uri = message.uri as string;
				const nextInset = { uri, symbol: null, source: null, loading: true };
				insetRef.current = nextInset;
				setSelectedUri(uri);
				setInset(nextInset);
			} else if (message?.type === "cockpitTestViewport") {
				setViewportCommand({
					commandId: message.commandId as number,
					viewport: message.viewport as CockpitViewport,
				});
			} else if (message?.type === "searchResults") {
				if (message.query === queryRef.current) {
					setResults(message.rows as SymbolDto[]);
					setSearchPending(false);
				}
			} else if (message?.type === "externalSelection") {
				const selection = message as ExternalSelectionMessage;
				setExternalSelection(selection);
				setSelectedUri(selection.symbol.uri);
				if (insetRef.current && insetRef.current.uri !== selection.symbol.uri) {
					const nextInset = { uri: selection.symbol.uri, symbol: null, source: null, loading: true };
					insetRef.current = nextInset;
					setInset(nextInset);
					postInspect(selection.symbol.uri);
				}
			} else if (message?.type === "inset") {
				const payload = message as InsetMessage;
				if (insetRef.current?.uri === payload.uri) {
					const nextInset = { uri: payload.uri, symbol: payload.symbol, source: payload.source, loading: false };
					insetRef.current = nextInset;
					setInset(nextInset);
				}
				acknowledgeInset(
					{ uri: payload.uri, symbol: payload.symbol, source: payload.source, loading: false },
					"loaded",
				);
			} else if (message?.type === "cockpitError") {
				setError({ prefix: message.prefix as string, message: message.message as string });
				setLoading(false);
			}
		};
		window.addEventListener("message", onMessage);
		vscode.postMessage({ type: "ready" });
		return () => window.removeEventListener("message", onMessage);
	}, []);

	useEffect(() => {
		const normalized = query.trim();
		if (!normalized) {
			setResults([]);
			setSearchPending(false);
			return;
		}
		setSearchPending(true);
		const timer = window.setTimeout(() => vscode.postMessage({ type: "search", query }), 180);
		return () => window.clearTimeout(timer);
	}, [query]);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (event.target instanceof HTMLInputElement) return;
			if (event.altKey && event.key === "ArrowLeft") {
				vscode.postMessage({ type: "back" });
			} else if (event.altKey && event.key === "ArrowRight") {
				vscode.postMessage({ type: "forward" });
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, []);

	const inspect = useCallback((uri: string) => {
		setSelectedUri(uri);
		const nextInset = { uri, symbol: null, source: null, loading: true };
		insetRef.current = nextInset;
		setInset(nextInset);
		postInspect(uri);
	}, []);
	const closeInset = useCallback(() => {
		insetRef.current = null;
		setInset(null);
	}, []);

	const persistPreferences = useCallback((preferences: CockpitPreferences) => {
		vscode.postMessage({ type: "preferences", preferences });
	}, []);

	const togglePin = useCallback((symbol: SymbolDto, nextPinned: boolean) => {
		setPinned((current) =>
			nextPinned
				? current.some((row) => row.uri === symbol.uri) ? current : [...current, symbol]
				: current.filter((row) => row.uri !== symbol.uri),
		);
		vscode.postMessage({ type: "pin", uri: symbol.uri, pinned: nextPinned });
	}, []);
	const rememberSession = useCallback((focus: string, snapshot: CockpitSessionSnapshot) => {
		cockpitSessions.current.set(focus, snapshot);
	}, []);

	return (
		<div className="explorer-shell">
				<SearchBox
				query={query}
				results={results}
				pending={searchPending}
					autoFocus={!cockpit}
					context={context}
				onChange={setQuery}
				onChoose={(symbol) => {
					setQuery("");
					setResults([]);
					postFocus(symbol.uri);
				}}
			/>
			{loading && !cockpit ? (
				<div className="cockpit-state cockpit-loading" role="status">
					<span className="cockpit-spinner" aria-hidden="true" />
					<strong>Opening symbol cockpit…</strong>
					<span>Loading the focused definition and its indexed neighborhood.</span>
				</div>
			) : error ? (
				<div className="cockpit-state cockpit-error" role="alert">
					<strong>Graph query failed</strong>
					<div>Graph query failed: {error.message}</div>
					<button
						type="button"
						className="nav"
						style={{ marginTop: 8 }}
						onClick={() => postFocus(error.prefix)}
					>
						Retry
					</button>
				</div>
			) : cockpit ? (
				<>
				<CockpitView
					payload={cockpit}
					expansion={expansion}
					expansionError={expansionError}
					perspective={cockpitPerspective}
					filters={cockpitFilters}
					radius={cockpitRadius}
					pinned={pinned}
					selectedUri={selectedUri}
					externalSelection={externalSelection}
					onPerspective={(perspective) => {
						setCockpitPerspective(perspective);
						persistPreferences({ perspective, filters: cockpitFilters, radius: cockpitRadius, positions: cockpitPositions });
					}}
					onFilters={(nextFilters) => {
						setCockpitFilters(nextFilters);
						persistPreferences({ perspective: cockpitPerspective, filters: nextFilters, radius: cockpitRadius, positions: cockpitPositions });
					}}
					onRadius={(direction, value) => {
						const radius = { ...cockpitRadius, [direction]: value };
						setCockpitRadius(radius);
						persistPreferences({ perspective: cockpitPerspective, filters: cockpitFilters, radius, positions: cockpitPositions });
					}}
					onTogglePin={togglePin}
					inset={inset}
					onInspect={inspect}
					onCloseInset={closeInset}
					recenterToken={recenterToken}
					onRecenter={() => setRecenterToken((value) => value + 1)}
					inspectorWidth={inspectorWidth}
					onInspectorResize={setInspectorWidth}
					positions={cockpitPositions}
					onPosition={(uri, position) => {
						const positions = { ...cockpitPositions, [uri]: position };
						setCockpitPositions(positions);
						persistPreferences({
							perspective: cockpitPerspective,
							filters: cockpitFilters,
							radius: cockpitRadius,
							positions,
						});
					}}
					restoredSessions={cockpitSessions.current}
					onSessionChange={rememberSession}
					viewportCommand={viewportCommand}
					renderProbeToken={renderProbeToken}
				/>
				{loading && (
					<div className="cockpit-loading-overlay" role="status">
						<span className="cockpit-spinner" aria-hidden="true" />
						<span>Refreshing cockpit…</span>
					</div>
				)}
				</>
			) : (
				<div className="empty cockpit-welcome">
					<strong>{context ? `Explore ${context.label}` : "Start with a symbol."}</strong>
					<span>
						{context
							? `Search is scoped to this ${context.kind}. Choose a symbol to keep exploring in the same cockpit.`
							: "Search above, use the workspace tree, or focus the symbol at the editor cursor."}
					</span>
					{context && <code className="cockpit-context-identity">{context.identity}</code>}
				</div>
			)}
		</div>
	);
}

function acknowledgeInset(inset: InsetState, reason: "loaded" | "preserved"): void {
	window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
		const inspector = document.querySelector<HTMLElement>("[data-code-inspector]");
		vscode.postMessage({
			type: "insetAck",
			uri: inset.uri,
			lines: inset.source ? inset.source.lines.length : 0,
			reason,
			inspectorMode: "contextual",
			graphMounted: Boolean(document.querySelector("[data-cockpit-focus]")),
			inspectorMounted: inspector?.dataset.codeInspector === inset.uri,
			legacyPathPickerPresent: Boolean(document.querySelector(".cockpit-path-picker")),
		});
	}));
}

function SearchBox({
	query,
	results,
	pending,
	autoFocus,
	context,
	onChange,
	onChoose,
}: {
	query: string;
	results: SymbolDto[];
	pending: boolean;
	autoFocus: boolean;
	context?: CockpitContext;
	onChange: (query: string) => void;
	onChoose: (symbol: SymbolDto) => void;
}) {
	const open = query.trim().length > 0;
	return (
		<div className="cockpit-search-wrap">
			<div className="cockpit-search">
				<span aria-hidden="true">⌕</span>
				<input
					type="search"
					value={query}
					autoFocus={autoFocus}
					placeholder={context ? `Find in ${context.label}…` : "Find a symbol, type, function, file…"}
					aria-label="Find a symbol"
					onChange={(event) => onChange(event.target.value)}
				/>
				{pending && <span className="search-pending">searching…</span>}
			</div>
			{open && !pending && (
				<div className="search-results" role="listbox">
					{results.length === 0 ? (
						<div className="search-empty">No matching symbol.</div>
					) : (
						results.map((symbol) => (
							<button key={symbol.uri} type="button" role="option" onClick={() => onChoose(symbol)}>
								<span className={glyphClass(symbol.kind)}>{symbolGlyph(symbol.kind)}</span>
								<span className="search-result-copy">
									<strong>{symbol.name}</strong>
									<small>{symbol.file} · {symbol.kind}</small>
								</span>
							</button>
						))
					)}
				</div>
			)}
		</div>
	);
}

function CockpitView({
	payload,
	expansion,
	expansionError,
	perspective,
	filters,
	radius,
	pinned,
	selectedUri,
	externalSelection,
	onPerspective,
	onFilters,
	onRadius,
	onTogglePin,
	inset,
	onInspect,
	onCloseInset,
	recenterToken,
	onRecenter,
	inspectorWidth,
	onInspectorResize,
	positions,
	onPosition,
	restoredSessions,
	onSessionChange,
	viewportCommand,
	renderProbeToken,
}: {
	payload: CockpitPayload;
	expansion: CockpitExpansionMessage | null;
	expansionError: CockpitExpansionErrorMessage | null;
	perspective: CockpitPerspective;
	filters: CockpitFilters;
	radius: CockpitRadius;
	pinned: SymbolDto[];
	selectedUri: string | null;
	externalSelection: ExternalSelectionMessage | null;
	onPerspective: (perspective: CockpitPerspective) => void;
	onFilters: (filters: CockpitFilters) => void;
	onRadius: (direction: keyof CockpitRadius, value: number) => void;
	onTogglePin: (symbol: SymbolDto, pinned: boolean) => void;
	inset: InsetState | null;
	onInspect: (uri: string) => void;
	onCloseInset: () => void;
	recenterToken: number;
	onRecenter: () => void;
	inspectorWidth: number;
	onInspectorResize: (width: number) => void;
	positions: Record<string, CockpitPosition>;
	onPosition: (uri: string, position: CockpitPosition) => void;
	restoredSessions: Map<string, CockpitSessionSnapshot>;
	onSessionChange: (focus: string, snapshot: CockpitSessionSnapshot) => void;
	viewportCommand: { commandId: number; viewport: CockpitViewport } | null;
	renderProbeToken: number;
}) {
	if (payload.graph.focus.kind !== "symbol") return null;
	const focus = payload.graph.focus.symbol;
	return (
		<>
			<div className="cockpit-toolbar">
				<HistoryButtons canBack={payload.canBack} canForward={payload.canForward} />
				<div className="cockpit-focus-context" title={`${focus.file} · ${focus.name}`}>
					<span className="cockpit-focus-name">
						<span className={glyphClass(focus.kind)}>{symbolGlyph(focus.kind)}</span>
						<strong>{focus.name}</strong>
						<span>{focus.kind}</span>
					</span>
					<span className="cockpit-focus-path">
						{payload.context && <>{payload.context.label} <i>›</i></>}
						{focus.file}
					</span>
				</div>
				<span className="cockpit-direction incoming">← {payload.graph.coverage.callers.total} callers</span>
				<span className="cockpit-direction outgoing">{payload.graph.coverage.callees.total} dependencies →</span>
				{(
					payload.graph.coverage.callers.returned < payload.graph.coverage.callers.total ||
					payload.graph.coverage.callees.returned < payload.graph.coverage.callees.total
				) && (
					<span className="unresolved" title="The daemon limited one or more relation sections">
						▲ limited
					</span>
				)}
				{payload.graph.unlinked.unresolved > 0 && (
					<span className="unresolved" title="References the index could not resolve">
						▲ {payload.graph.unlinked.unresolved}
					</span>
				)}
				<button type="button" className="nav cockpit-recenter" onClick={onRecenter} title="Frame focus and direct neighbors">
					◎ Recenter
				</button>
				<button
					type="button"
					className={inset ? "nav cockpit-code-toggle active" : "nav cockpit-code-toggle"}
					aria-pressed={Boolean(inset)}
					title={inset ? "Close the contextual code inspector" : "Review the selected symbol without leaving the cockpit"}
					onClick={() => inset ? onCloseInset() : onInspect(selectedUri ?? focus.uri)}
				>
					{inset ? "Hide code" : "View code"}
				</button>
			</div>
			<div className="cockpit-subtoolbar">
				<span className="cockpit-control-group" role="group" aria-label="Perspective">
					<span className="cockpit-control-label">View</span>
					<button
						type="button"
						className={perspective === "neighborhood" ? "cockpit-chip on" : "cockpit-chip"}
						aria-pressed={perspective === "neighborhood"}
						onClick={() => onPerspective("neighborhood")}
					>
						Neighborhood
					</button>
					<button
						type="button"
						className={perspective === "impact" ? "cockpit-chip on impact" : "cockpit-chip"}
						aria-pressed={perspective === "impact"}
						title="Show symbols that can be affected by a change to the focus"
						onClick={() => onPerspective("impact")}
					>
						Impact
					</button>
				</span>
				<span className="cockpit-control-group" role="group" aria-label="Direction">
					<span className="cockpit-control-label">Flow</span>
					<ToggleChip
						label="← in"
						pressed={perspective === "impact" || filters.incoming}
						disabled={perspective === "impact"}
						onToggle={() => onFilters({ ...filters, incoming: !filters.incoming })}
					/>
					<ToggleChip
						label="out →"
						pressed={perspective === "neighborhood" && filters.outgoing}
						disabled={perspective === "impact"}
						onToggle={() => onFilters({ ...filters, outgoing: !filters.outgoing })}
					/>
				</span>
				<span className="cockpit-control-group relations" role="group" aria-label="Relations">
					<span className="cockpit-control-label">Relations</span>
					{(["calls", "data", "types", "references"] as CockpitRelation[]).map((relation) => (
						<ToggleChip
							key={relation}
							label={relation}
							pressed={filters[relation]}
							onToggle={() => onFilters({ ...filters, [relation]: !filters[relation] })}
						/>
					))}
				</span>
				<RadiusControl
					direction="incoming"
					value={radius.incoming}
					disabled={false}
					onChange={(value) => onRadius("incoming", value)}
				/>
				<RadiusControl
					direction="outgoing"
					value={radius.outgoing}
					disabled={perspective === "impact"}
					onChange={(value) => onRadius("outgoing", value)}
				/>
			</div>
			<div
				className={inset ? "canvas-zone cockpit-zone with-inspector" : "canvas-zone cockpit-zone"}
				style={inset ? ({ "--cm-inspector-width": `${inspectorWidth}px` } as CSSProperties) : undefined}
			>
				<CockpitCanvas
					key={focus.uri}
					payload={payload}
					expansion={expansion}
					expansionError={expansionError}
					perspective={perspective}
					filters={filters}
					radius={radius}
					pinned={pinned}
					selectedUri={selectedUri}
					externalSelection={externalSelection}
					onInspect={onInspect}
					onTogglePin={onTogglePin}
					onRendered={(evidence) =>
					vscode.postMessage({
						type: "ack",
						prefix: evidence.focus,
						nodes: evidence.nodes,
						edges: evidence.edges,
						mode: "cockpit",
						perspective,
						radius,
						enabledRelations: (["calls", "data", "types", "references"] as CockpitRelation[]).filter(
							(relation) => filters[relation],
						),
						pins: pinned.length,
						framedNodes: evidence.framedNodes,
						viewportZoom: evidence.viewport.zoom,
						viewport: evidence.viewport,
						mountedEdgePaths: evidence.mountedEdgePaths,
						visibleEdgePaths: evidence.visibleEdgePaths,
						paintedEdgePaths: evidence.paintedEdgePaths,
						zoomControls: evidence.zoomControls,
						reactFlowReady: evidence.reactFlowReady,
						viewportCommandId: evidence.viewportCommandId,
					})
					}
					recenterToken={recenterToken}
					positions={positions}
					onPosition={onPosition}
					restoredSession={restoredSessions.get(focus.uri)}
					onSessionChange={onSessionChange}
					viewportCommand={viewportCommand}
					renderProbeToken={renderProbeToken}
				/>
				{inset && (
					<CodeInset
						inset={inset}
						onClose={onCloseInset}
						width={inspectorWidth}
						onResize={onInspectorResize}
					/>
				)}
			</div>
			<PerspectiveBar payload={payload} pinned={pinned} />
		</>
	);
}

function PerspectiveBar({ payload, pinned }: { payload: CockpitPayload; pinned: SymbolDto[] }) {
	return (
		<footer className="cockpit-perspectives">
			<span className="perspective-label">⚑ Pinned</span>
			{pinned.length === 0 ? (
				<span className="perspective-muted">none</span>
			) : (
				pinned.slice(0, 4).map((symbol) => (
					<button type="button" key={symbol.uri} onClick={() => postFocus(symbol.uri)}>
						{symbol.name}
					</button>
				))
			)}
			<span className="perspective-divider" />
			<span className="perspective-label">Perspectives</span>
			{payload.perspectives.map((perspective) => (
				<span className="perspective-item" key={perspective.name}>
					<button type="button" onClick={() => vscode.postMessage({ type: "loadPerspective", name: perspective.name })}>
						{perspective.name}
					</button>
					<button
						type="button"
						className="delete-perspective"
						aria-label={`Delete perspective ${perspective.name}`}
						onClick={() => vscode.postMessage({ type: "deletePerspective", name: perspective.name })}
					>
						×
					</button>
				</span>
			))}
			<button type="button" className="save-perspective" onClick={() => vscode.postMessage({ type: "savePerspective" })}>
				+ Save perspective
			</button>
		</footer>
	);
}

function ToggleChip({
	label,
	pressed,
	disabled = false,
	onToggle,
}: {
	label: string;
	pressed: boolean;
	disabled?: boolean;
	onToggle: () => void;
}) {
	return (
		<button
			type="button"
			className={pressed ? "cockpit-chip on" : "cockpit-chip"}
			aria-pressed={pressed}
			disabled={disabled}
			onClick={onToggle}
		>
			{label}
		</button>
	);
}

function RadiusControl({
	direction,
	value,
	disabled,
	onChange,
}: {
	direction: keyof CockpitRadius;
	value: number;
	disabled: boolean;
	onChange: (value: number) => void;
}) {
	const label = direction === "incoming" ? "upstream" : "downstream";
	return (
		<span className="cockpit-control-group radius" role="group" aria-label={`${label} radius`}>
			<span className="cockpit-control-label">{label}</span>
			<button
				type="button"
				className="cockpit-radius-step"
				disabled={disabled || value <= 0}
				aria-label={`Decrease ${label} radius`}
				onClick={() => onChange(Math.max(0, value - 1))}
			>
				−
			</button>
			<strong className="cockpit-radius-value">{value}</strong>
			<button
				type="button"
				className="cockpit-radius-step"
				disabled={disabled || value >= 4}
				aria-label={`Increase ${label} radius`}
				onClick={() => onChange(Math.min(4, value + 1))}
			>
				+
			</button>
		</span>
	);
}

function HistoryButtons({ canBack, canForward }: { canBack: boolean; canForward: boolean }) {
	return (
		<>
			<button
				type="button"
				className="nav"
				title="Back (Alt+←)"
				disabled={!canBack}
				onClick={() => vscode.postMessage({ type: "back" })}
			>
				←
			</button>
			<button
				type="button"
				className="nav"
				title="Forward (Alt+→)"
				disabled={!canForward}
				onClick={() => vscode.postMessage({ type: "forward" })}
			>
				→
			</button>
		</>
	);
}
