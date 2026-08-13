import * as vscode from "vscode";

import { DaemonRpcError, SymbolDto } from "../daemon/model";
import { highlightSource } from "../symbols/detail/highlight";
import { renderExplorerHtml } from "./html";
import { ExplorerNavigation } from "./navigation";
import {
	CockpitAck,
	CockpitContext,
	ExplorerMessage,
	ExplorerStateMessage,
	InsetAck,
	InsetMessage,
	CockpitExpansionErrorMessage,
	CockpitExpansionMessage,
	CockpitPayload,
	CockpitPreferences,
	CockpitSavedPerspective,
	CockpitViewport,
} from "./protocol";
import { ExplorerRepository } from "./repository";

const PINS_KEY = "codeMoniker.explorer.cockpit.pins";
const PREFERENCES_KEY = "codeMoniker.explorer.cockpit.preferences";
const PERSPECTIVES_KEY = "codeMoniker.explorer.cockpit.perspectives";
type FocusOrigin = "cockpit" | "editor" | "tree" | "refresh";
const DEFAULT_PREFERENCES: CockpitPreferences = {
	perspective: "neighborhood",
	filters: {
		incoming: true,
		outgoing: true,
		calls: true,
		data: true,
		types: true,
		references: true,
	},
	radius: { incoming: 1, outgoing: 1 },
	positions: {},
};

export class ExplorerPanel implements vscode.Disposable {
	private panel?: vscode.WebviewPanel;
	private readonly navigation = new ExplorerNavigation();
	private seq = 0;
	private searchSeq = 0;
	private lastMessage?: ExplorerStateMessage;
	private acks: CockpitAck[] = [];
	private insetAckList: InsetAck[] = [];
	private treeSyncRequestList: Array<{ uri: string; origin: FocusOrigin }> = [];
	private pinnedUris: string[];
	private preferences: CockpitPreferences;
	private searchContext?: CockpitContext;
	private perspectives: CockpitSavedPerspective[];
	private testViewportSequence = 0;

	// Test observability: the last message the host decided to show, and the
	// acks the webview sent back after actually rendering the cockpit and insets.
	get current(): ExplorerStateMessage | undefined {
		return this.lastMessage;
	}

	get webviewAcks(): readonly CockpitAck[] {
		return this.acks;
	}

	get insetAcks(): readonly InsetAck[] {
		return this.insetAckList;
	}

	get treeSyncRequests(): readonly { uri: string; origin: FocusOrigin }[] {
		return this.treeSyncRequestList;
	}

	get isOpen(): boolean {
		return this.panel !== undefined;
	}

	// E2E observability: exercise the same React Flow viewport API used by its
	// controls, then let the webview report the resulting transform.
	setTestViewport(viewport: CockpitViewport): number {
		const commandId = ++this.testViewportSequence;
		if (this.panel) {
			void this.panel.webview.postMessage({ type: "cockpitTestViewport", commandId, viewport });
		}
		return commandId;
	}

	// Same path as the webview's inspect message; lets the e2e suite drive
	// the inset flow, since the harness cannot click inside a webview.
	async inspect(uri: string): Promise<void> {
		if (this.panel) {
			await this.panel.webview.postMessage({ type: "cockpitInspect", uri });
			await this.sendInset(this.panel, uri);
		}
	}

	select(symbol: SymbolDto, source: "tree" | "editor"): void {
		if (this.panel) {
			void this.panel.webview.postMessage({ type: "externalSelection", symbol, source });
		}
	}

	async setPinned(uri: string, pinned: boolean): Promise<void> {
		this.pinnedUris = pinned
			? this.pinnedUris.includes(uri) ? this.pinnedUris : [...this.pinnedUris, uri]
			: this.pinnedUris.filter((candidate) => candidate !== uri);
		await this.workspaceState.update(PINS_KEY, this.pinnedUris);
	}

	async setPreferences(preferences: CockpitPreferences): Promise<void> {
		this.preferences = normalizePreferences(preferences);
		await this.workspaceState.update(PREFERENCES_KEY, this.preferences);
	}

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly repository: ExplorerRepository,
		private readonly workspaceState: vscode.Memento,
		private readonly onSelectSymbol?: (symbol: SymbolDto) => void | Thenable<void>,
	) {
		this.pinnedUris = workspaceState.get<string[]>(PINS_KEY, []);
		this.preferences = normalizePreferences(
			workspaceState.get<CockpitPreferences>(PREFERENCES_KEY, DEFAULT_PREFERENCES),
		);
		this.perspectives = workspaceState
			.get<CockpitSavedPerspective[]>(PERSPECTIVES_KEY, [])
			.map(normalizePerspective)
			.filter((value): value is CockpitSavedPerspective => Boolean(value));
	}

	async focus(prefix: string, origin: FocusOrigin = "cockpit"): Promise<void> {
		this.navigation.push(prefix);
		await this.show(prefix, origin);
	}

	openContext(context: CockpitContext): void {
		this.searchContext = context;
		this.navigation.clear();
		const token = ++this.seq;
		this.renderEmpty(this.ensurePanel(), token);
	}

	async open(): Promise<void> {
		this.searchContext = undefined;
		this.navigation.clear();
		const token = ++this.seq;
		this.renderEmpty(this.ensurePanel(), token);
	}

	async refreshCurrent(): Promise<void> {
		const current = this.navigation.current;
		if (current !== undefined && this.panel) {
			await this.show(current, "refresh");
		}
	}

	dispose(): void {
		this.panel?.dispose();
	}

	private async show(prefix: string, origin: FocusOrigin): Promise<void> {
		const token = ++this.seq;
		const panel = this.ensurePanel();
		try {
			await this.render(panel, token, prefix, origin);
		} catch (error) {
			if (token === this.seq && this.panel === panel) {
				// Store the error so a late webview ready handshake can replay it
				// has mounted, the ready handshake replays it instead of
				// leaving the user on a silent empty state.
				this.lastMessage = {
					type: "cockpitError",
					prefix,
					message: error instanceof Error ? error.message : String(error),
				};
				void panel.webview.postMessage(this.lastMessage);
			}
		}
	}

	private async render(
		panel: vscode.WebviewPanel,
		token: number,
		prefix: string,
		origin: FocusOrigin,
	): Promise<void> {
		if (!prefix) {
			this.renderEmpty(panel, token);
			return;
		}
		await this.renderCockpit(panel, token, prefix, origin);
	}

	private async renderCockpit(
		panel: vscode.WebviewPanel,
		token: number,
		focus: string,
		origin: FocusOrigin,
	): Promise<void> {
		if (token !== this.seq || this.panel !== panel) return;
		this.lastMessage = { type: "cockpitLoading", prefix: focus };
		void panel.webview.postMessage(this.lastMessage);
		let graph;
		try {
			graph = await this.repository.symbolGraph(focus);
		} catch (error) {
			if (
				error instanceof DaemonRpcError &&
				(error.code === "focus_is_directory" || error.code === "focus_not_found")
			) {
				this.renderEmpty(panel, token);
				return;
			}
			throw error;
		}
		if (!graph || graph.focus.kind !== "symbol") {
			this.renderEmpty(panel, token);
			return;
		}
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		this.navigation.replace(graph.focus.symbol.uri);
		panel.title = `${graph.focus.symbol.name} · Code Cockpit`;
		const pinned = (
			await Promise.all(
				this.pinnedUris.map(async (uri) =>
					(await this.repository.symbolDetail(uri))?.symbol,
				),
			)
		).filter((symbol): symbol is SymbolDto => Boolean(symbol));
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		const payload: CockpitPayload = {
			graph,
			canBack: this.navigation.canBack,
			canForward: this.navigation.canForward,
			pinned,
			preferences: this.preferences,
			context: this.searchContext,
			perspectives: this.perspectives,
		};
		this.lastMessage = { type: "cockpit", payload };
		void panel.webview.postMessage(this.lastMessage);
		if (origin === "cockpit" || origin === "editor") {
			this.treeSyncRequestList.push({ uri: graph.focus.symbol.uri, origin });
			void Promise.resolve(this.onSelectSymbol?.(graph.focus.symbol)).catch(() => undefined);
		}
	}

	private renderEmpty(panel: vscode.WebviewPanel, token: number): void {
		if (token !== this.seq || this.panel !== panel) return;
		panel.title = "Code Cockpit";
		this.lastMessage = { type: "cockpitEmpty", context: this.searchContext };
		void panel.webview.postMessage(this.lastMessage);
	}

	private ensurePanel(): vscode.WebviewPanel {
		if (this.panel) {
			this.panel.reveal(undefined, true);
			return this.panel;
		}
		const panel = vscode.window.createWebviewPanel(
			"codeMoniker.graphExplorer",
			"Code Cockpit",
			{ viewColumn: vscode.ViewColumn.Active, preserveFocus: false },
			{
				enableScripts: true,
				retainContextWhenHidden: true,
				localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
			},
		);
		panel.iconPath = {
			light: vscode.Uri.joinPath(this.extensionUri, "icons", "graph-light.svg"),
			dark: vscode.Uri.joinPath(this.extensionUri, "icons", "graph-dark.svg"),
		};
		panel.webview.html = renderExplorerHtml(panel.webview, this.extensionUri);
		panel.onDidDispose(() => {
			this.panel = undefined;
		});
		panel.webview.onDidReceiveMessage((message: ExplorerMessage) => {
			if (message?.type === "focus" && message.prefix !== undefined) {
				void this.focus(message.prefix, "cockpit");
			} else if (message?.type === "expand" && message.uri) {
				void this.sendExpansion(panel, message);
			} else if (message?.type === "search") {
				void this.sendSearch(panel, message.query);
			} else if (message?.type === "pin") {
				void this.setPinned(message.uri, message.pinned);
			} else if (message?.type === "preferences") {
				void this.setPreferences(message.preferences);
			} else if (message?.type === "savePerspective") {
				void this.savePerspective();
			} else if (message?.type === "loadPerspective") {
				void this.loadPerspective(message.name);
			} else if (message?.type === "deletePerspective") {
				void this.deletePerspective(message.name);
			} else if (message?.type === "back") {
				void this.step(-1);
			} else if (message?.type === "forward") {
				void this.step(1);
			} else if (message?.type === "inspect" && message.uri) {
				void this.sendInset(panel, message.uri);
			} else if (message?.type === "openSource" && message.target) {
				void vscode.commands.executeCommand("codeMoniker.symbols.openSource", message.target);
			} else if (message?.type === "ready" && this.lastMessage) {
				void panel.webview.postMessage(this.lastMessage);
			} else if (message?.type === "ack") {
				this.acks.push({
					prefix: message.prefix,
					nodes: message.nodes,
					edges: message.edges,
					mode: message.mode,
					perspective: message.perspective,
					radius: message.radius,
					enabledRelations: message.enabledRelations,
					pins: message.pins,
					framedNodes: message.framedNodes,
					viewportZoom: message.viewportZoom,
					mountedEdgePaths: message.mountedEdgePaths,
					visibleEdgePaths: message.visibleEdgePaths,
					paintedEdgePaths: message.paintedEdgePaths,
					zoomControls: message.zoomControls,
					reactFlowReady: message.reactFlowReady,
					viewport: message.viewport,
					viewportCommandId: message.viewportCommandId,
				});
			} else if (message?.type === "insetAck") {
				this.insetAckList.push({
					uri: message.uri,
					lines: message.lines,
					reason: message.reason,
					inspectorMode: message.inspectorMode,
					graphMounted: message.graphMounted,
					inspectorMounted: message.inspectorMounted,
					legacyPathPickerPresent: message.legacyPathPickerPresent,
				});
			}
		});
		this.panel = panel;
		return panel;
	}

	private async sendSearch(panel: vscode.WebviewPanel, query: string): Promise<void> {
		const token = ++this.searchSeq;
		const normalized = query.trim();
		try {
			const rows = normalized
				? await this.repository.search(normalized, 12, this.searchContext?.identity)
				: [];
			if (token === this.searchSeq && this.panel === panel) {
				void panel.webview.postMessage({ type: "searchResults", query, rows });
			}
		} catch {
			if (token === this.searchSeq && this.panel === panel) {
				void panel.webview.postMessage({ type: "searchResults", query, rows: [] });
			}
		}
	}

	private async savePerspective(): Promise<void> {
		const focus = this.navigation.current;
		if (!focus) return;
		const name = await vscode.window.showInputBox({
			prompt: "Name this Code Moniker cockpit perspective",
			placeHolder: "e.g. parser impact",
			validateInput: (value) => value.trim() ? undefined : "Enter a name",
		});
		if (!name?.trim()) return;
		const perspective: CockpitSavedPerspective = {
			name: name.trim(),
			focus,
			pinnedUris: [...this.pinnedUris],
			preferences: this.preferences,
		};
		this.perspectives = [
			...this.perspectives.filter((candidate) => candidate.name !== perspective.name),
			perspective,
		].sort((left, right) => left.name.localeCompare(right.name));
		await this.workspaceState.update(PERSPECTIVES_KEY, this.perspectives);
		await this.refreshCurrent();
	}

	private async loadPerspective(name: string): Promise<void> {
		const perspective = this.perspectives.find((candidate) => candidate.name === name);
		if (!perspective) return;
		this.preferences = normalizePreferences(perspective.preferences);
		this.pinnedUris = [...perspective.pinnedUris];
		await Promise.all([
			this.workspaceState.update(PREFERENCES_KEY, this.preferences),
			this.workspaceState.update(PINS_KEY, this.pinnedUris),
		]);
		this.navigation.push(perspective.focus);
		await this.show(perspective.focus, "cockpit");
	}

	private async deletePerspective(name: string): Promise<void> {
		const next = this.perspectives.filter((candidate) => candidate.name !== name);
		if (next.length === this.perspectives.length) return;
		this.perspectives = next;
		await this.workspaceState.update(PERSPECTIVES_KEY, this.perspectives);
		await this.refreshCurrent();
	}

	private async sendExpansion(
		panel: vscode.WebviewPanel,
		request: Extract<ExplorerMessage, { type: "expand" }>,
	): Promise<void> {
		try {
			const graph = await this.repository.symbolGraph(request.uri);
			if (this.panel !== panel || !graph || graph.focus.kind !== "symbol") {
				return;
			}
			const message: CockpitExpansionMessage = {
				type: "cockpitExpansion",
				uri: request.uri,
				requestId: request.requestId,
				rootFocus: request.rootFocus,
				generation: request.generation,
				graph,
			};
			void panel.webview.postMessage(message);
		} catch (error) {
			const message: CockpitExpansionErrorMessage = {
				type: "cockpitExpansionError",
				uri: request.uri,
				requestId: request.requestId,
				rootFocus: request.rootFocus,
				generation: request.generation,
				message: error instanceof Error ? error.message : String(error),
			};
			void panel.webview.postMessage(message);
		}
	}

	// The code zone of one definition, highlighted host-side. Failures fall
	// back to a null source; the webview says so instead of staying silent.
	private async sendInset(panel: vscode.WebviewPanel, uri: string): Promise<void> {
		try {
			const detail = await this.repository.symbolDetail(uri);
			if (this.panel !== panel || !detail?.symbol) {
				return;
			}
			const source = detail.source
				? await highlightSource(detail.source, detail.symbol.language)
				: null;
			if (this.panel !== panel) {
				return;
			}
			const message: InsetMessage = { type: "inset", uri, symbol: detail.symbol, source };
			void panel.webview.postMessage(message);
		} catch {
			void panel.webview.postMessage({ type: "inset", uri, symbol: null, source: null });
		}
	}

	private async step(delta: number): Promise<void> {
		const focus = this.navigation.move(delta < 0 ? -1 : 1);
		if (focus) await this.show(focus, "cockpit");
	}
}

function normalizePreferences(value: CockpitPreferences): CockpitPreferences {
	const filters = value?.filters ?? DEFAULT_PREFERENCES.filters;
	const rawRadius: unknown = value?.radius;
	const legacyRadius = typeof rawRadius === "number" ? rawRadius : undefined;
	const radius = typeof rawRadius === "object" && rawRadius
		? rawRadius as Partial<CockpitPreferences["radius"]>
		: { incoming: legacyRadius, outgoing: legacyRadius };
	return {
		perspective: value?.perspective === "impact" ? "impact" : "neighborhood",
		filters: {
			incoming: filters.incoming !== false,
			outgoing: filters.outgoing !== false,
			calls: filters.calls !== false,
			data: filters.data !== false,
			types: filters.types !== false,
			references: filters.references !== false,
		},
		radius: {
			incoming: normalizeRadius(radius.incoming, DEFAULT_PREFERENCES.radius.incoming),
			outgoing: normalizeRadius(radius.outgoing, DEFAULT_PREFERENCES.radius.outgoing),
		},
		positions: normalizePositions(value?.positions),
	};
}

function normalizeRadius(value: unknown, fallback: number): number {
	return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 4
		? value
		: fallback;
}

function normalizePositions(value: unknown): CockpitPreferences["positions"] {
	if (!value || typeof value !== "object") return {};
	const positions: CockpitPreferences["positions"] = {};
	for (const [uri, candidate] of Object.entries(value)) {
		if (!candidate || typeof candidate !== "object") continue;
		const point = candidate as { x?: unknown; y?: unknown };
		if (typeof point.x === "number" && Number.isFinite(point.x) && typeof point.y === "number" && Number.isFinite(point.y)) {
			positions[uri] = { x: point.x, y: point.y };
		}
	}
	return positions;
}

function normalizePerspective(value: CockpitSavedPerspective): CockpitSavedPerspective | undefined {
	if (!value || typeof value.name !== "string" || typeof value.focus !== "string") return undefined;
	return {
		name: value.name,
		focus: value.focus,
		pinnedUris: Array.isArray(value.pinnedUris)
			? value.pinnedUris.filter((uri): uri is string => typeof uri === "string")
			: [],
		preferences: normalizePreferences(value.preferences),
	};
}
