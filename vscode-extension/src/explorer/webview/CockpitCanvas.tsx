import {
	Background,
	BackgroundVariant,
	Controls,
	MarkerType,
	MiniMap,
	ReactFlow,
	type ReactFlowInstance,
	type Edge,
	type Node,
	type Viewport,
	useNodesState,
} from "@xyflow/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { SymbolDto, SymbolGraphNeighbor, SymbolGraphResult } from "../../daemon/model";
import type {
	CockpitExpansionMessage,
	CockpitExpansionErrorMessage,
	CockpitFilters,
	CockpitPayload,
	CockpitPerspective,
	CockpitPosition,
	CockpitRadius,
	CockpitRelation,
	CockpitViewport,
	ExternalSelectionMessage,
} from "../protocol";
import { postExpand, postFocus, postOpenSource } from "./actions";
import { CockpitCard, type CockpitCardData } from "./graph/CockpitCard";
import { CockpitEdge as CockpitEdgeView, type CockpitEdgeData } from "./graph/CockpitEdge";
import { layoutCockpit } from "./graph/cockpitLayout";
import { reconcileControlledNodes } from "./graph/controlledNodes";
import { selectActiveEdgeRelations } from "./graph/edgeRelations";
import {
	EXPANSION_RELATION_BATCH,
	INITIAL_RELATION_BUDGET,
	rankCodeNeighbors,
	selectInitialCodeNeighbors,
} from "./graph/neighborSelection";
import { ExpansionCoordinator } from "./expansionCoordinator";

const COCKPIT_NODE_BUDGET = 36;
const COLUMN_GAP = 380;
const ROW_GAP = 140;

interface CockpitCanvasProps {
	payload: CockpitPayload;
	expansion: CockpitExpansionMessage | null;
	expansionError: CockpitExpansionErrorMessage | null;
	perspective: CockpitPerspective;
	filters: CockpitFilters;
	radius: CockpitRadius;
	pinned: SymbolDto[];
	selectedUri: string | null;
	externalSelection: ExternalSelectionMessage | null;
	onInspect: (uri: string) => void;
	onTogglePin: (symbol: SymbolDto, pinned: boolean) => void;
	onRendered: (evidence: CockpitRenderEvidence) => void;
	recenterToken: number;
	positions: Record<string, CockpitPosition>;
	onPosition: (uri: string, position: CockpitPosition) => void;
	restoredSession?: CockpitSessionSnapshot;
	onSessionChange: (focus: string, snapshot: CockpitSessionSnapshot) => void;
	viewportCommand: { commandId: number; viewport: CockpitViewport } | null;
	renderProbeToken: number;
}

export interface CockpitRenderEvidence {
	focus: string;
	nodes: number;
	edges: number;
	framedNodes: number;
	viewport: CockpitViewport;
	mountedEdgePaths: number;
	visibleEdgePaths: number;
	paintedEdgePaths: number;
	zoomControls: number;
	reactFlowReady: boolean;
	viewportCommandId?: number;
}

interface LoadedNeighborhood {
	incoming: SymbolGraphNeighbor[];
	outgoing: SymbolGraphNeighbor[];
	truncatedIncoming: number;
	truncatedOutgoing: number;
}

interface ExplorationSnapshot {
	nodes: Node<CockpitCardData>[];
	edges: CockpitEdge[];
	neighborhoods: Map<string, LoadedNeighborhood>;
	positions: Record<string, { x: number; y: number }>;
	viewport: CockpitViewport;
}

type CockpitEdge = Edge<CockpitEdgeData>;
export interface CockpitSessionSnapshot {
	nodes: Array<{
		symbol: SymbolDto;
		position: { x: number; y: number };
		depth: number;
		direction: CockpitCardData["direction"];
		loaded: boolean;
	}>;
	edges: CockpitEdge[];
	neighborhoods: Array<[string, LoadedNeighborhood]>;
	positions: Record<string, { x: number; y: number }>;
	viewport: CockpitViewport;
}

const NODE_TYPES = { cockpitCard: CockpitCard };
const EDGE_TYPES = { cockpitEdge: CockpitEdgeView };

export function CockpitCanvas({
	payload,
	expansion,
	expansionError,
	perspective,
	filters,
	radius,
	pinned,
	selectedUri,
	externalSelection,
	onInspect,
	onTogglePin,
	onRendered,
	recenterToken,
	positions,
	onPosition,
	restoredSession,
	onSessionChange,
	viewportCommand,
	renderProbeToken,
}: CockpitCanvasProps) {
	const focus = payload.graph.focus.kind === "symbol" ? payload.graph.focus.symbol : null;
	const [nodes, setNodes] = useState<Node<CockpitCardData>[]>([]);
	const [edges, setEdges] = useState<CockpitEdge[]>([]);
	const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
	const [historyVersion, setHistoryVersion] = useState(0);
	const nodesRef = useRef(nodes);
	const edgesRef = useRef(edges);
	const neighborhoods = useRef(new Map<string, LoadedNeighborhood>());
	const loading = useRef(new Set<string>());
	const expansions = useRef(new ExpansionCoordinator());
	const flow = useRef<ReactFlowInstance<Node<CockpitCardData>, CockpitEdge> | null>(null);
	const canvas = useRef<HTMLDivElement | null>(null);
	const onRenderedRef = useRef(onRendered);
	onRenderedRef.current = onRendered;
	const positionCache = useRef<Record<string, { x: number; y: number }>>({});
	const forceControlledLayout = useRef(false);
	const frameIdentities = useRef(new Set<string>());
	const initializedFocus = useRef<string | null>(null);
	const handledRootGraph = useRef<SymbolGraphResult | null>(null);
	const sessionReady = useRef(false);
	const initialFrameRequested = useRef(false);
	const restoredViewportApplied = useRef(false);
	const hasReportedEvidence = useRef(false);
	const lastRenderProbeToken = useRef(renderProbeToken);
	const lastViewportCommandId = useRef<number | undefined>(undefined);
	const lastRecenterToken = useRef(recenterToken);
	const viewportRef = useRef<CockpitViewport>({ x: 0, y: 0, zoom: 1 });
	const [viewportVersion, setViewportVersion] = useState(0);
	const positionsRef = useRef(positions);
	positionsRef.current = positions;
	const pinnedRef = useRef(pinned);
	pinnedRef.current = pinned;
	const controlsRef = useRef({ filters, perspective, radius });
	const undoStack = useRef<ExplorationSnapshot[]>([]);
	const redoStack = useRef<ExplorationSnapshot[]>([]);
	controlsRef.current = { filters, perspective, radius };

	useEffect(() => {
		nodesRef.current = nodes;
	}, [nodes]);
	useEffect(() => {
		edgesRef.current = edges;
	}, [edges]);

	const updateNodes = useCallback((next: Node<CockpitCardData>[]) => {
		nodesRef.current = next;
		setNodes(next);
	}, []);
	const updateEdges = useCallback((next: CockpitEdge[]) => {
		edgesRef.current = next;
		setEdges(next);
	}, []);
	const captureSnapshot = useCallback((): ExplorationSnapshot => ({
		nodes: nodesRef.current.map((node) => ({
			...node,
			position: { ...node.position },
			data: { ...node.data },
		})),
		edges: edgesRef.current.map((edge) => ({ ...edge, data: edge.data ? { ...edge.data } : edge.data })),
		neighborhoods: new Map([...neighborhoods.current].map(([uri, value]) => [uri, {
			incoming: [...value.incoming],
			outgoing: [...value.outgoing],
			truncatedIncoming: value.truncatedIncoming,
			truncatedOutgoing: value.truncatedOutgoing,
		}])),
		positions: Object.fromEntries(
			Object.entries(positionCache.current).map(([uri, position]) => [uri, { ...position }]),
		),
		viewport: { ...viewportRef.current },
	}), []);
	const restoreSnapshot = useCallback((snapshot: ExplorationSnapshot) => {
		neighborhoods.current = new Map(snapshot.neighborhoods);
		positionCache.current = { ...snapshot.positions };
		viewportRef.current = { ...snapshot.viewport };
		expansions.current.reset();
		loading.current.clear();
		forceControlledLayout.current = true;
		updateNodes(snapshot.nodes);
		updateEdges(snapshot.edges);
		void flow.current?.setViewport(snapshot.viewport, { duration: 0 });
		setHistoryVersion((value) => value + 1);
	}, [updateEdges, updateNodes]);
	const checkpoint = useCallback(() => {
		undoStack.current.push(captureSnapshot());
		redoStack.current = [];
		setHistoryVersion((value) => value + 1);
	}, [captureSnapshot]);
	const undo = useCallback(() => {
		const prior = undoStack.current.pop();
		if (!prior) return;
		redoStack.current.push(captureSnapshot());
		restoreSnapshot(prior);
	}, [captureSnapshot, restoreSnapshot]);
	const redo = useCallback(() => {
		const next = redoStack.current.pop();
		if (!next) return;
		undoStack.current.push(captureSnapshot());
		restoreSnapshot(next);
	}, [captureSnapshot, restoreSnapshot]);

	const handleInspect = useCallback((uri: string) => onInspect(uri), [onInspect]);
	const handleTogglePin = useCallback(
		(symbol: SymbolDto, nextPinned: boolean) => onTogglePin(symbol, nextPinned),
		[onTogglePin],
	);

	const reveal = useCallback(
		(
			center: SymbolDto,
			graph: SymbolGraphResult,
			batch: number,
			direction?: "incoming" | "outgoing",
			replaceCenterRelations = false,
		) => {
			const incoming = rankCodeNeighbors(graph.callers, center);
			const outgoing = rankCodeNeighbors(graph.callees, center);
			neighborhoods.current.set(center.uri, {
				incoming,
				outgoing,
				truncatedIncoming: Math.max(0, graph.coverage.callers.total - graph.coverage.callers.returned),
				truncatedOutgoing: Math.max(0, graph.coverage.callees.total - graph.coverage.callees.returned),
			});

			const centerNode = nodesRef.current.find((node) => node.id === center.uri);
			const centerDepth = centerNode?.data.depth ?? 0;
			const controls = controlsRef.current;
			const present = new Set(nodesRef.current.map((node) => node.id));
			const canRevealIncoming = centerDepth < controls.radius.incoming;
			const canRevealOutgoing = centerDepth < controls.radius.outgoing;
			const availableIncoming = direction !== "outgoing" && canRevealIncoming && allowsIncoming(controls)
				? eligible(incoming, controls.filters).filter((row) => !present.has(row.symbol.uri))
				: [];
			const availableOutgoing = direction !== "incoming" && canRevealOutgoing && allowsOutgoing(controls)
				? eligible(outgoing, controls.filters).filter((row) => !present.has(row.symbol.uri))
				: [];
			const directLoaded = direction === undefined
				? nodesRef.current.filter((node) =>
					node.id !== center.uri &&
					node.data.depth === centerDepth + 1 && node.data.direction !== "pinned",
				).length
				: 0;
			const initial = direction === undefined
				? selectInitialCodeNeighbors(
					availableIncoming,
					availableOutgoing,
					center,
					Math.max(0, batch - directLoaded),
				)
				: undefined;
			const selectedIncoming = initial?.incoming ?? (direction === "incoming" ? availableIncoming.slice(0, batch) : []);
			for (const row of selectedIncoming) present.add(row.symbol.uri);
			const selectedOutgoing = initial?.outgoing ?? (direction === "outgoing"
				? availableOutgoing.filter((row) => !present.has(row.symbol.uri)).slice(0, batch)
				: []);
			for (const row of selectedOutgoing) present.add(row.symbol.uri);

			const additions = [
				...positionedNodes(center.uri, "incoming", selectedIncoming, nodesRef.current, centerDepth + 1),
				...positionedNodes(center.uri, "outgoing", selectedOutgoing, nodesRef.current, centerDepth + 1),
			];
			let nextNodes = [...nodesRef.current];
			for (const addition of additions) {
				const existing = nextNodes.find((node) => node.id === addition.id);
				if (!existing) {
					nextNodes.push(addition);
				} else if (addition.data.depth < existing.data.depth) {
					nextNodes = nextNodes.map((node) =>
						node.id === addition.id
							? {
								...node,
								data: { ...node.data, depth: addition.data.depth, direction: addition.data.direction },
							}
							: node,
					);
				}
			}
			nextNodes = nextNodes.map((node) =>
				node.id === center.uri
					? { ...node, data: { ...node.data, loaded: true, loading: false } }
					: node,
			);
			nextNodes = pruneNodes(
				nextNodes,
				payload.graph.focus.kind === "symbol" ? payload.graph.focus.symbol.uri : center.uri,
				new Set(pinnedRef.current.map((symbol) => symbol.uri)),
			);
			loading.current.delete(center.uri);
			updateNodes(nextNodes);

			const loaded = new Set(nextNodes.map((node) => node.id));
			const retainedEdges = replaceCenterRelations
				? edgesRef.current.filter((edge) => edge.source !== center.uri && edge.target !== center.uri)
				: edgesRef.current;
			const nextEdges = new Map(retainedEdges.map((edge) => [edge.id, edge]));
			for (const neighbor of incoming) {
				if (loaded.has(neighbor.symbol.uri)) {
					const edge = graphEdge(neighbor.symbol.uri, center.uri, neighbor);
					nextEdges.set(edge.id, edge);
				}
			}
			for (const neighbor of outgoing) {
				if (loaded.has(neighbor.symbol.uri)) {
					const edge = graphEdge(center.uri, neighbor.symbol.uri, neighbor);
					nextEdges.set(edge.id, edge);
				}
			}
			updateEdges([...nextEdges.values()].filter((edge) => loaded.has(edge.source) && loaded.has(edge.target)));
		},
		[updateEdges, updateNodes],
	);

	const handleExpand = useCallback(
		(uri: string, direction: "incoming" | "outgoing") => {
			const known = neighborhoods.current.get(uri);
			const node = nodesRef.current.find((candidate) => candidate.id === uri);
			if (!node || node.data.depth >= controlsRef.current.radius[direction]) return;
			if (loading.current.has(uri)) return;
			checkpoint();
			if (known) {
				reveal(
					node.data.symbol,
					{
						...payload.graph,
						focus: { kind: "symbol", symbol: node.data.symbol },
						callers: known.incoming,
						callees: known.outgoing,
						coverage: {
							...payload.graph.coverage,
							callers: coverageFor(known.incoming, known.truncatedIncoming),
							callees: coverageFor(known.outgoing, known.truncatedOutgoing),
						},
					},
					EXPANSION_RELATION_BATCH,
					direction,
				);
				return;
			}
			loading.current.add(uri);
			const rootFocus = payload.graph.focus.kind === "symbol"
				? payload.graph.focus.symbol.uri
				: "";
			const request = expansions.current.begin(uri, rootFocus, direction);
			updateNodes(
				nodesRef.current.map((candidate) =>
					candidate.id === uri
						? { ...candidate, data: { ...candidate.data, loading: true } }
						: candidate,
				),
			);
			postExpand(uri, request.requestId, request.rootFocus, request.generation);
		},
		[checkpoint, payload.graph, reveal, updateNodes],
	);

	const makeNode = useCallback(
		(
			symbol: SymbolDto,
			position: { x: number; y: number },
			isFocus: boolean,
			depth: number,
			direction: CockpitCardData["direction"],
		): Node<CockpitCardData> => ({
			id: symbol.uri,
			type: "cockpitCard",
			position,
			dragHandle: ".cockpit-drag-handle",
			ariaLabel: `${symbol.name}, ${symbol.kind}. Use the drag handle to reposition.`,
			data: {
				symbol,
					focus: isFocus,
					depth,
					direction,
				loaded: false,
				loading: false,
				hiddenIncoming: 0,
				hiddenOutgoing: 0,
				truncated: 0,
				expandable: true,
					incomingLimited: false,
					outgoingLimited: false,
				pinned: false,
				selected: false,
				onFocus: postFocus,
				onInspect: handleInspect,
				onOpen: postOpenSource,
				onExpand: handleExpand,
				onTogglePin: handleTogglePin,
			},
		}),
		[handleExpand, handleInspect, handleTogglePin],
	);

	// A new root focus remounts this component (keyed by URI in App), so this
	// initialization runs once. Expansions update the same React Flow instance
	// and preserve its viewport and every existing card position.
	useEffect(() => {
		if (!focus) return;
		if (initializedFocus.current === focus.uri) return;
		initializedFocus.current = focus.uri;
		sessionReady.current = false;
		expansions.current.reset();
		loading.current.clear();
		restoredViewportApplied.current = false;
		initialFrameRequested.current = Boolean(restoredSession?.viewport);
		positionCache.current = restoredSession
			? { ...restoredSession.positions }
			: { ...positionsRef.current };
		undoStack.current = [];
		redoStack.current = [];
		setHistoryVersion((value) => value + 1);
		if (restoredSession) {
			viewportRef.current = { ...restoredSession.viewport };
			handledRootGraph.current = null;
			const restoredNodes = restoredSession.nodes.map((entry) => {
				const node = makeNode(
					entry.symbol,
					entry.position,
					entry.symbol.uri === focus.uri,
					entry.depth,
					entry.direction,
				);
				return { ...node, data: { ...node.data, loaded: entry.loaded } };
			});
			neighborhoods.current = new Map(restoredSession.neighborhoods);
			updateNodes(restoredNodes);
			updateEdges(restoredSession.edges);
			sessionReady.current = true;
			return;
		}
		const root = makeNode(focus, { x: 0, y: 0 }, true, 0, "focus");
		const anchors = payload.pinned
			.filter((symbol) => symbol.uri !== focus.uri)
			.map((symbol, index) => makeNode(symbol, { x: 0, y: 250 + index * ROW_GAP }, false, 0, "pinned"));
		updateNodes([root, ...anchors]);
		updateEdges([]);
		neighborhoods.current.clear();
		handledRootGraph.current = payload.graph;
		reveal(focus, payload.graph, INITIAL_RELATION_BUDGET);
		sessionReady.current = true;
	}, [focus, makeNode, payload.graph, restoredSession, reveal, updateEdges, updateNodes]);

	useEffect(() => {
		if (!focus || initializedFocus.current !== focus.uri || handledRootGraph.current === payload.graph) {
			return;
		}
		handledRootGraph.current = payload.graph;
		reveal(focus, payload.graph, INITIAL_RELATION_BUDGET, undefined, true);
	}, [focus, payload.graph, reveal]);

	useEffect(() => {
		if (!focus || !sessionReady.current || nodes.length === 0) return;
		onSessionChange(focus.uri, {
			nodes: nodes.map((node) => ({
				symbol: node.data.symbol,
				position: positionCache.current[node.id] ?? node.position,
				depth: node.data.depth,
				direction: node.data.direction,
				loaded: node.data.loaded,
			})),
			edges: edges.map((edge) => ({ ...edge, data: edge.data ? { ...edge.data } : edge.data })),
			neighborhoods: [...neighborhoods.current].map(([uri, value]) => [uri, {
				incoming: [...value.incoming],
				outgoing: [...value.outgoing],
				truncatedIncoming: value.truncatedIncoming,
				truncatedOutgoing: value.truncatedOutgoing,
			}]),
			positions: { ...positionCache.current },
			viewport: { ...viewportRef.current },
		});
	}, [edges, focus, nodes, onSessionChange, viewportVersion]);

	useEffect(() => {
		if (!focus || !expansion || expansion.graph.focus.kind !== "symbol") {
			return;
		}
		const pending = expansions.current.take(expansion, focus.uri);
		if (!pending) return;
		if (!nodesRef.current.some((node) => node.id === expansion.uri)) {
			loading.current.delete(expansion.uri);
			return;
		}
		reveal(
			expansion.graph.focus.symbol,
			expansion.graph,
			EXPANSION_RELATION_BATCH,
			pending.direction,
		);
	}, [expansion, focus, reveal]);

	useEffect(() => {
		if (!focus || !expansionError || !loading.current.has(expansionError.uri)) return;
		const pending = expansions.current.take(expansionError, focus.uri);
		if (!pending) return;
		loading.current.delete(expansionError.uri);
		updateNodes(
			nodesRef.current.map((node) =>
				node.id === expansionError.uri
					? { ...node, data: { ...node.data, loading: false } }
					: node,
			),
		);
	}, [expansionError, focus, updateNodes]);

	useEffect(() => {
		const onKey = (event: KeyboardEvent) => {
			if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "z") return;
			if (event.target instanceof HTMLInputElement) return;
			event.preventDefault();
			if (event.shiftKey) redo();
			else undo();
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [redo, undo]);

	const projection = useMemo(
		() => projectGraph(
			focus?.uri ?? "",
			nodes,
			edges,
			neighborhoods.current,
			perspective,
			filters,
			radius,
			new Set(pinned.map((symbol) => symbol.uri)),
			selectedUri,
		),
		[edges, filters, focus?.uri, nodes, perspective, pinned, radius, selectedUri],
	);
	const positionedProjection = useMemo(() => {
		if (!focus) return projection;
		const positions = layoutCockpit(
			projection.nodes,
			projection.edges,
			focus.uri,
			positionCache.current,
		);
		positionCache.current = positions;
		return {
			...projection,
			nodes: projection.nodes.map((node) => ({
				...node,
				position: positions[node.id] ?? node.position,
			})),
			edges: projection.edges.map((edge) => ({
				...edge,
				selected: edge.id === selectedEdgeId,
				data: edge.data ? { ...edge.data, showLabel: true } : edge.data,
			})),
		};
	}, [focus, projection, selectedEdgeId]);
	const [controlledNodes, setControlledNodes, onControlledNodesChange] =
		useNodesState<Node<CockpitCardData>>([]);
	const [flowReady, setFlowReady] = useState(false);
	useEffect(() => {
		const resetLayout = forceControlledLayout.current;
		forceControlledLayout.current = false;
		setControlledNodes((current) =>
			reconcileControlledNodes(current, positionedProjection.nodes, resetLayout),
		);
	}, [positionedProjection.nodes, setControlledNodes]);
	frameIdentities.current = new Set([
		focus?.uri ?? "",
		...pinned.map((symbol) => symbol.uri),
		...positionedProjection.edges.flatMap((edge) =>
			edge.source === focus?.uri || edge.target === focus?.uri
				? [edge.source, edge.target]
				: [],
		),
	].filter(Boolean));
	const externalSelectionVisible = externalSelection
		? positionedProjection.nodes.some((node) => node.id === externalSelection.symbol.uri)
		: true;

	const reportRenderedEvidence = useCallback((attempt = 0, viewportCommandId?: number): void => {
		const instance = flow.current;
		const root = canvas.current;
		if (!instance || !root || !focus) return;
		const edgePaths = Array.from(root.querySelectorAll<SVGPathElement>(".react-flow__edge-path"));
		const mounted = edgePaths.filter((path) => {
			if (!path.getAttribute("d")?.trim()) return false;
			try {
				return path.getTotalLength() > 0;
			} catch {
				return false;
			}
		});
		const renderedEdges = instance.getEdges().length;
		if (mounted.length < renderedEdges && attempt < 8) {
			window.requestAnimationFrame(() => reportRenderedEvidence(attempt + 1, viewportCommandId));
			return;
		}
		const rootRect = root.getBoundingClientRect();
		const visibleEdgePaths = mounted.filter((path) => {
			const rect = path.getBoundingClientRect();
			return (rect.width > 0 || rect.height > 0) &&
				rect.right >= rootRect.left && rect.left <= rootRect.right &&
				rect.bottom >= rootRect.top && rect.top <= rootRect.bottom;
		}).length;
		const paintedEdgePaths = mounted.filter((path) => {
			const style = window.getComputedStyle(path);
			return style.stroke !== "none" && style.stroke !== "transparent" &&
				Number.parseFloat(style.strokeWidth) > 0 && Number.parseFloat(style.strokeOpacity || "1") > 0;
		}).length;
		const zoomControls = root.querySelectorAll(".react-flow__controls-button").length;
		const pane = root.querySelector<HTMLElement>(".react-flow__pane");
		const viewportElement = root.querySelector<HTMLElement>(".react-flow__viewport");
		const viewport = instance.getViewport();
		viewportRef.current = viewport;
		const reactFlowReady = Boolean(
			pane && viewportElement &&
			window.getComputedStyle(pane).position === "absolute" &&
			window.getComputedStyle(pane).touchAction === "none" &&
			zoomControls >= 3,
		);
		hasReportedEvidence.current = true;
		onRenderedRef.current({
			focus: focus.uri,
			nodes: instance.getNodes().length,
			edges: renderedEdges,
			framedNodes: instance.getNodes().filter((node) => frameIdentities.current.has(node.id)).length,
			viewport,
			mountedEdgePaths: mounted.length,
			visibleEdgePaths,
			paintedEdgePaths,
			zoomControls,
			reactFlowReady,
			viewportCommandId,
		});
	}, [focus]);

	const frameNeighborhood = useCallback((instance = flow.current) => {
		if (!instance || !focus) return;
		const liveInstance = instance;
		const focusUri = focus.uri;
		const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
		const fitMeasuredNeighborhood = (attempt: number) => {
			// Read the live React Flow state after the initial neighborhood has
			// landed. onInit can run while the canvas still contains only the root.
			const framed = liveInstance.getNodes().filter((node) => frameIdentities.current.has(node.id));
			if (framed.length === 0) {
				if (attempt < 8) window.requestAnimationFrame(() => fitMeasuredNeighborhood(attempt + 1));
				return;
			}
			const measured = framed.every((node) =>
				(node.measured?.width ?? node.width ?? 0) > 0 &&
				(node.measured?.height ?? node.height ?? 0) > 0,
			);
			if (!measured && attempt < 8) {
				window.requestAnimationFrame(() => fitMeasuredNeighborhood(attempt + 1));
				return;
			}
			void liveInstance.fitView({
				nodes: framed,
				padding: 0.12,
				minZoom: 0.72,
				maxZoom: 1.05,
				duration: reducedMotion ? 0 : 180,
			}).then(() => reportRenderedEvidence(0));
		};
		window.requestAnimationFrame(() => window.requestAnimationFrame(() => fitMeasuredNeighborhood(0)));
	}, [focus?.uri, reportRenderedEvidence]);

	useEffect(() => {
		if (!focus || !flowReady || !flow.current || controlledNodes.length === 0) return;
		const recentered = lastRecenterToken.current !== recenterToken;
		if (initialFrameRequested.current && !recentered) return;
		initialFrameRequested.current = true;
		lastRecenterToken.current = recenterToken;
		frameNeighborhood(flow.current);
	}, [controlledNodes.length, flowReady, focus, frameNeighborhood, recenterToken]);

	useEffect(() => {
		if (!flowReady || !flow.current || !restoredSession?.viewport || restoredViewportApplied.current) return;
		restoredViewportApplied.current = true;
		viewportRef.current = { ...restoredSession.viewport };
		void flow.current.setViewport(restoredSession.viewport, { duration: 0 })
			.then(() => reportRenderedEvidence(0));
	}, [flowReady, reportRenderedEvidence, restoredSession]);

	useEffect(() => {
		if (!viewportCommand || !flowReady || !flow.current || lastViewportCommandId.current === viewportCommand.commandId) return;
		lastViewportCommandId.current = viewportCommand.commandId;
		viewportRef.current = { ...viewportCommand.viewport };
		void flow.current.setViewport(viewportCommand.viewport, { duration: 0 }).then(() => {
			setViewportVersion((value) => value + 1);
			reportRenderedEvidence(0, viewportCommand.commandId);
		});
	}, [flowReady, reportRenderedEvidence, viewportCommand]);

	useEffect(() => {
		if (lastRenderProbeToken.current === renderProbeToken) return;
		lastRenderProbeToken.current = renderProbeToken;
		if (!hasReportedEvidence.current) return;
		window.requestAnimationFrame(() => window.requestAnimationFrame(() => reportRenderedEvidence(0)));
	}, [renderProbeToken, reportRenderedEvidence]);

	const onNodesChange = useCallback(
		(changes: Parameters<typeof onControlledNodesChange>[0]) => {
			for (const change of changes) {
				if (change.type === "position" && change.position) {
					positionCache.current[change.id] = change.position;
				}
			}
			onControlledNodesChange(changes);
		},
		[onControlledNodesChange],
	);
	const nodeTypes = useMemo(() => NODE_TYPES, []);
	const edgeTypes = useMemo(() => EDGE_TYPES, []);
	const visibleFiles = useMemo(
		() => new Set(positionedProjection.nodes.map((node) => node.data.symbol.file)).size,
		[positionedProjection.nodes],
	);
	const selectedEdge = selectedEdgeId
		? positionedProjection.edges.find((edge) => edge.id === selectedEdgeId)
		: undefined;
	const selectedEdgeSource = selectedEdge
		? positionedProjection.nodes.find((node) => node.id === selectedEdge.source)?.data.symbol
		: undefined;
	const selectedEdgeTarget = selectedEdge
		? positionedProjection.nodes.find((node) => node.id === selectedEdge.target)?.data.symbol
		: undefined;

	if (!focus) return <div className="muted graph-empty">Choose a symbol to start exploring.</div>;
	return (
		<div ref={canvas} className="cockpit-canvas" data-cockpit-focus={focus.uri}>
			<ReactFlow
				nodes={controlledNodes}
				edges={positionedProjection.edges}
				nodeTypes={nodeTypes}
				edgeTypes={edgeTypes}
				onNodesChange={onNodesChange}
				onInit={(instance) => {
					flow.current = instance;
					setFlowReady(true);
				}}
				onMoveEnd={(_, viewport: Viewport) => {
					viewportRef.current = viewport;
					setViewportVersion((value) => value + 1);
				}}
				onNodeDragStop={(_, node) => {
					positionCache.current[node.id] = node.position;
					updateNodes(nodesRef.current.map((candidate) =>
						candidate.id === node.id ? { ...candidate, position: node.position } : candidate,
					));
					onPosition(node.id, node.position);
				}}
				onEdgeClick={(_, edge) => setSelectedEdgeId(edge.id)}
				onPaneClick={() => setSelectedEdgeId(null)}
				minZoom={0.25}
				maxZoom={1.8}
				panOnScroll={false}
				panOnDrag
				zoomOnScroll
				zoomOnPinch
				zoomOnDoubleClick={false}
				nodesConnectable={false}
				nodesFocusable
				edgesFocusable
				autoPanOnNodeFocus
				deleteKeyCode={null}
				selectNodesOnDrag={false}
				elevateEdgesOnSelect
				elementsSelectable
			>
				<Background variant={BackgroundVariant.Dots} gap={20} size={1} />
				<MiniMap
					aria-label="Graph overview; drag or scroll here to navigate"
					position="bottom-left"
					pannable
					zoomable
					nodeColor={(node) => miniMapNodeColor(node as Node<CockpitCardData>)}
					nodeStrokeWidth={2}
				/>
				<Controls
					aria-label="Graph zoom and framing controls"
					showInteractive={false}
					position="bottom-right"
				/>
			</ReactFlow>
			<div className="cockpit-rollup" aria-live="polite">
				<span className="cockpit-map-label">Visible map</span>
				<span>{positionedProjection.nodes.length} symbols</span>
				<span>{positionedProjection.edges.length} relations</span>
				<span>{visibleFiles} files</span>
				{projection.frontier > 0 && <strong>+{projection.frontier} hidden</strong>}
				<span className="cockpit-map-direction incoming">← callers</span>
				<span className="cockpit-map-focus">focus</span>
				<span className="cockpit-map-direction outgoing">dependencies →</span>
			</div>
			<div className="cockpit-canvas-actions" data-history-version={historyVersion}>
				<button type="button" disabled={undoStack.current.length === 0} onClick={undo} title="Undo expansion (Ctrl/Cmd+Z)">
					↶ Expand
				</button>
				<button type="button" disabled={redoStack.current.length === 0} onClick={redo} title="Redo expansion (Ctrl/Cmd+Shift+Z)">
					↷
				</button>
			</div>
			<div className="cockpit-navigation-hint" aria-hidden="true">
				Drag canvas to pan · scroll to zoom
			</div>
			{selectedEdge && selectedEdgeSource && selectedEdgeTarget && (
				<div className="cockpit-edge-inspector" role="status" aria-label="Selected relation">
					<span className={`cockpit-edge-swatch ${selectedEdge.data?.relation ?? "references"}`} aria-hidden="true" />
					<span className="cockpit-edge-endpoint" title={selectedEdgeSource.name}>{selectedEdgeSource.name}</span>
					<span className="cockpit-edge-relation">{selectedEdge.data?.label ?? "references"} →</span>
					<span className="cockpit-edge-endpoint" title={selectedEdgeTarget.name}>{selectedEdgeTarget.name}</span>
					<button type="button" aria-label="Close relation details" onClick={() => setSelectedEdgeId(null)}>×</button>
				</div>
			)}
			{expansionError && <div className="cockpit-toast">Could not expand: {expansionError.message}</div>}
			{externalSelection && !externalSelectionVisible && (
				<div className="cockpit-external-selection" role="status">
					<span>
						<strong>{externalSelection.symbol.name}</strong> selected in {externalSelection.source}
					</span>
					<button type="button" onClick={() => postFocus(externalSelection.symbol.uri)}>Focus</button>
				</div>
			)}
		</div>
	);

	function positionedNodes(
		centerId: string,
		direction: "incoming" | "outgoing",
		neighbors: SymbolGraphNeighbor[],
		current: Node<CockpitCardData>[],
		depth: number,
	): Node<CockpitCardData>[] {
		const center = current.find((node) => node.id === centerId)?.position ?? { x: 0, y: 0 };
		const occupied = current.map((node) => node.position);
		return neighbors.map((neighbor, index) => {
			const desired = {
				x: center.x + (direction === "incoming" ? -COLUMN_GAP : COLUMN_GAP),
				y: center.y + (index - (neighbors.length - 1) / 2) * ROW_GAP,
			};
			const position = freePosition(desired, occupied);
			occupied.push(position);
			return makeNode(neighbor.symbol, position, false, depth, direction);
		});
	}
}

function projectGraph(
	focus: string,
	nodes: Node<CockpitCardData>[],
	edges: CockpitEdge[],
	neighborhoods: Map<string, LoadedNeighborhood>,
	perspective: CockpitPerspective,
	filters: CockpitFilters,
	radius: CockpitRadius,
	pinned: Set<string>,
	selectedUri: string | null,
): { nodes: Node<CockpitCardData>[]; edges: CockpitEdge[]; frontier: number } {
	const enabledEdges = edges
		.map((edge) => projectEdgeRelations(edge, filters))
		.filter((edge): edge is CockpitEdge => Boolean(edge));
	const visible = new Set<string>();
	const depths = new Map<string, number>();
	if (focus) {
		visible.add(focus);
		depths.set(focus, 0);
	}
	for (const uri of pinned) {
		visible.add(uri);
		depths.set(uri, 0);
	}
	const walk = (direction: "incoming" | "outgoing", limit: number) => {
		if (limit <= 0) return;
		const allowed = direction === "incoming"
			? allowsIncoming({ filters, perspective })
			: allowsOutgoing({ filters, perspective });
		if (!allowed) return;
		let frontier = [focus];
		const visited = new Set(frontier);
		for (let distance = 1; distance <= limit && frontier.length > 0; distance++) {
			const nextFrontier = new Set<string>();
			for (const current of frontier) {
				for (const edge of enabledEdges) {
					const next = direction === "incoming" && edge.target === current
						? edge.source
						: direction === "outgoing" && edge.source === current
							? edge.target
							: undefined;
					if (!next || visited.has(next)) continue;
					visited.add(next);
					visible.add(next);
					depths.set(next, direction === "incoming" ? -distance : distance);
					nextFrontier.add(next);
				}
			}
			frontier = [...nextFrontier];
		}
	};
	walk("incoming", radius.incoming);
	walk("outgoing", radius.outgoing);

	let frontier = 0;
	const projectedNodes = nodes
		.filter((node) => visible.has(node.id))
		.map((node) => {
			const known = neighborhoods.get(node.id);
			const signedDepth = depths.get(node.id) ?? (node.data.direction === "incoming" ? -node.data.depth : node.data.depth);
			const depth = Math.abs(signedDepth);
			const incomingRows = known && allowsIncoming({ filters, perspective })
				? eligible(known.incoming, filters)
				: [];
			const outgoingRows = known && allowsOutgoing({ filters, perspective })
				? eligible(known.outgoing, filters)
				: [];
			const hiddenIncoming = incomingRows.filter((row) => !visible.has(row.symbol.uri)).length;
			const hiddenOutgoing = outgoingRows.filter((row) => !visible.has(row.symbol.uri)).length;
			const truncated = known
				? (allowsIncoming({ filters, perspective }) ? known.truncatedIncoming : 0) +
					(allowsOutgoing({ filters, perspective }) ? known.truncatedOutgoing : 0)
				: 0;
			const hidden = hiddenIncoming + hiddenOutgoing + truncated;
			frontier += hidden;
			return {
				...node,
				data: {
					...node.data,
					depth,
					hiddenIncoming,
					hiddenOutgoing,
					truncated,
					pinned: pinned.has(node.id),
					selected: selectedUri === node.id,
					expandable: hiddenIncoming + hiddenOutgoing > 0 || !node.data.loaded,
					incomingLimited: depth >= radius.incoming && hiddenIncoming > 0,
					outgoingLimited: depth >= radius.outgoing && hiddenOutgoing > 0,
				},
			};
		});
	const projectedEdges = enabledEdges.filter((edge) => visible.has(edge.source) && visible.has(edge.target));
	return { nodes: projectedNodes, edges: projectedEdges, frontier };
}

function eligible(rows: SymbolGraphNeighbor[], filters: CockpitFilters): SymbolGraphNeighbor[] {
	return rows.filter((row) => relationGroups(row.kinds).some((relation) => relationEnabled(relation, filters)));
}

function relationEnabled(relation: CockpitRelation, filters: CockpitFilters): boolean {
	return filters[relation];
}

function allowsIncoming(controls: { filters: CockpitFilters; perspective: CockpitPerspective }): boolean {
	return controls.perspective === "impact" || controls.filters.incoming;
}

function allowsOutgoing(controls: { filters: CockpitFilters; perspective: CockpitPerspective }): boolean {
	return controls.perspective === "neighborhood" && controls.filters.outgoing;
}

function coverageFor(rows: SymbolGraphNeighbor[], truncated: number) {
	return { matching: rows.length + truncated, returned: rows.length, total: rows.length + truncated };
}

function freePosition(
	desired: { x: number; y: number },
	occupied: { x: number; y: number }[],
): { x: number; y: number } {
	for (let distance = 0; distance < 40; distance++) {
		const step = distance === 0 ? 0 : Math.ceil(distance / 2) * (distance % 2 === 1 ? 1 : -1);
		const candidate = { x: desired.x, y: desired.y + step * ROW_GAP };
		if (!occupied.some((point) => Math.abs(point.x - candidate.x) < 220 && Math.abs(point.y - candidate.y) < 82)) {
			return candidate;
		}
	}
	return desired;
}

function graphEdge(source: string, target: string, neighbor: SymbolGraphNeighbor): CockpitEdge {
	const relations = relationGroups(neighbor.kinds);
	const relation = relations[0] ?? "references";
	const relationLabels = Object.fromEntries(
		relations.map((candidate) => [candidate, edgeLabel(neighbor, candidate)]),
	) as Partial<Record<CockpitRelation, string>>;
	return {
		id: `${source}->${target}`,
		source,
		target,
		type: "cockpitEdge",
		className: `cockpit-edge ${relation}`,
		data: {
			relation,
			relations,
			relationLabels,
			label: relations.map((candidate) => relationLabels[candidate]).filter(Boolean).join(" · "),
			showLabel: true,
		},
		markerEnd: {
			type: MarkerType.ArrowClosed,
			color: relationStroke(relation),
			width: 14,
			height: 14,
			markerUnits: "userSpaceOnUse",
		},
		style: { strokeWidth: Math.min(1.7 + Math.log2(neighbor.count) * 0.22, 3.2) },
	};
}

function projectEdgeRelations(edge: CockpitEdge, filters: CockpitFilters): CockpitEdge | undefined {
	const data = edge.data;
	const enabled = selectActiveEdgeRelations(
		data?.relations ?? [data?.relation ?? "references"],
		data?.relationLabels ?? {},
		filters,
	);
	if (!enabled) return undefined;
	const { relation, relations: enabledRelations, label } = enabled;
	const labels = data?.relationLabels ?? {};
	return {
		...edge,
		className: `cockpit-edge ${relation}`,
		data: {
			...data,
			relation,
			relations: enabledRelations,
			relationLabels: labels,
			label,
			showLabel: data?.showLabel ?? true,
		},
		markerEnd: {
			type: MarkerType.ArrowClosed,
			color: relationStroke(relation),
			width: 14,
			height: 14,
			markerUnits: "userSpaceOnUse",
		},
	};
}

function relationStroke(relation: CockpitRelation): string {
	switch (relation) {
		case "calls": return "var(--vscode-charts-orange)";
		case "data": return "var(--vscode-charts-green)";
		case "types": return "var(--vscode-charts-purple)";
		case "references": return "var(--vscode-charts-blue)";
	}
}

function relationGroups(kinds: string[]): CockpitRelation[] {
	const groups = new Set<CockpitRelation>();
	for (const kind of kinds) {
		groups.add(relationGroupForKind(kind));
	}
	return [...groups];
}

function edgeLabel(neighbor: SymbolGraphNeighbor, relation: CockpitRelation): string {
	const kind = neighbor.kinds.find((candidate) => relationGroupForKind(candidate) === relation)
		?.replaceAll("_", " ") ?? relation;
	return neighbor.count > 1 ? `${kind} ×${neighbor.count}` : kind;
}

function relationGroupForKind(kind: string): CockpitRelation {
	if (kind === "calls" || kind === "method_call") return "calls";
	if (kind === "reads" || kind === "writes") return "data";
	if (kind === "uses_type" || kind === "returns_type" || kind === "instantiates") return "types";
	return "references";
}

function miniMapNodeColor(node: Node<CockpitCardData>): string {
	if (node.data.focus) return "var(--vscode-focusBorder)";
	if (node.data.pinned) return "var(--vscode-charts-yellow)";
	if (node.data.direction === "incoming") return "var(--vscode-charts-blue)";
	return "var(--vscode-charts-orange)";
}

function pruneNodes(
	nodes: Node<CockpitCardData>[],
	focus: string,
	pinned: ReadonlySet<string>,
): Node<CockpitCardData>[] {
	if (nodes.length <= COCKPIT_NODE_BUDGET) return nodes;
	const protectedIds = new Set([focus, ...pinned]);
	const removable = nodes
		.filter((node) => !protectedIds.has(node.id))
		.sort((left, right) =>
			right.data.depth - left.data.depth ||
			left.data.symbol.name.localeCompare(right.data.symbol.name) ||
			left.id.localeCompare(right.id),
		);
	const remove = new Set(removable.slice(0, nodes.length - COCKPIT_NODE_BUDGET).map((node) => node.id));
	return nodes.filter((node) => !remove.has(node.id));
}
