import type { SymbolDto, UsageSummaryDto } from "../../daemon/model";
import type { DetailDocument, DetailPayload, HighlightedUsageDto } from "./panel";
import type { HighlightedSourceLine, HighlightedSourceSnippet, HighlightedSourceToken } from "./highlight";

interface VsCodeApi {
	getState(): PersistedViewState | undefined;
	postMessage(message: unknown): void;
	setState(state: PersistedViewState): void;
}

interface PersistedViewState {
	hasSavedState?: boolean;
	key?: string;
	openDetails?: string[];
	openPreviews?: string[];
	scrollY?: number;
}

type WebviewMessage =
	| { type: "detail"; payload: DetailPayload }
	| { type: "document"; payload: DetailDocument }
	| { type: "usageSnippet"; requestId: string; snippet: HighlightedSourceSnippet | null };

type LineRange = [number, number];
type UsageDirectionScope = "incoming" | "outgoing";
type UsageBucketKind = "production" | "test" | "technical";
type UsageSummaryKind = UsageBucketKind | "context" | "file";

interface UsageBucket {
	kind: UsageBucketKind;
	label: string;
	rows: HighlightedUsageDto[];
}

interface UsageFileGroup {
	bucket: UsageBucketKind;
	contexts: UsageContextGroup[];
	file: string;
	rows: HighlightedUsageDto[];
	scope: UsageDirectionScope;
}

interface UsageContextGroup {
	label: string;
	occurrences: UsageOccurrence[];
	rows: HighlightedUsageDto[];
}

interface UsageOccurrence {
	key: string;
	kind: string;
	label: string;
	rows: HighlightedUsageDto[];
	sample: HighlightedUsageDto;
}

interface DetailsNode {
	body: HTMLDivElement;
	root: HTMLDetailsElement;
	summary: HTMLElement;
}

interface OpenableSource {
	file: string;
	line_range?: LineRange | null;
	root: string;
}

declare function acquireVsCodeApi(): VsCodeApi;

// Reactive renderer for the symbol detail webview. Receives `{ type: "detail",
// payload }` messages and rebuilds the panel from scratch. All text goes through
// textContent so symbol/source content is never interpreted as HTML.
(function () {
	const vscode = acquireVsCodeApi();
	const rootElement = document.getElementById("root");
	if (!rootElement) {
		return;
	}
	const root: HTMLElement = rootElement;
	const snippetRequests = new Map<string, UsageOccurrence>();
	let saveTimer: number | undefined;
	let snippetRequestSeq = 0;
	let restoringViewState = false;

	window.addEventListener("message", (event: MessageEvent<WebviewMessage>) => {
		const message = event.data;
		if (message && message.type === "detail") {
			render(message.payload);
		} else if (message && message.type === "document") {
			renderDocument(message.payload);
		} else if (message && message.type === "usageSnippet") {
			receiveUsageSnippet(message.requestId, message.snippet);
		}
	});
	window.addEventListener("scroll", scheduleViewStateSave, { passive: true });
	window.addEventListener("beforeunload", () => saveViewState());
	vscode.postMessage({ type: "ready" });

	function render(payload: DetailPayload): void {
		renderWithState("detail:" + payload.symbol.uri, () => {
			root.className = "";
			root.replaceChildren();
			root.appendChild(header(payload.symbol));
			if (payload.source) {
				root.appendChild(sourceSection(payload.symbol, payload.source));
			}
			root.appendChild(
				usagesSection("Incoming usages", payload.incoming, payload.incomingSummary, "incoming"),
			);
			root.appendChild(
				usagesSection("Outgoing usages", payload.outgoing, payload.outgoingSummary, "outgoing"),
			);
		});
	}

	function renderDocument(payload: DetailDocument): void {
		renderWithState("document:" + payload.title, () => {
			root.className = "";
			root.replaceChildren();
			const box = el("div", "header");
			const title = el("div", "title");
			title.appendChild(el("span", "kind", payload.kind));
			title.appendChild(el("span", "name", payload.title));
			box.appendChild(title);
			if (payload.description) {
				box.appendChild(el("div", "description", payload.description));
			}
			if (payload.meta && payload.meta.length > 0) {
				const meta = el("div", "meta");
				for (const row of payload.meta) {
					meta.appendChild(metaRow(row.label, row.value));
				}
				box.appendChild(meta);
			}
			root.appendChild(box);
			for (const sectionPayload of payload.sections || []) {
				const box = section(sectionPayload.title);
				if (sectionPayload.text) {
					box.body.appendChild(el("pre", "signature", sectionPayload.text));
				}
				for (const row of sectionPayload.rows || []) {
					box.body.appendChild(detailRow(row.label, row.value));
				}
				root.appendChild(box.root);
			}
		});
	}

	function header(symbol: SymbolDto): HTMLElement {
		const box = el("div", "header");
		const top = el("div", "header-top");
		const title = el("div", "title");
		title.appendChild(el("span", "kind", symbol.kind));
		title.appendChild(el("span", "name", symbol.name));
		top.appendChild(title);
		top.appendChild(openSourceLink(symbol, "Open source"));
		box.appendChild(top);

		if (symbol.signature) {
			box.appendChild(el("pre", "signature", symbol.signature));
		}

		const meta = el("div", "meta");
		meta.appendChild(metaRow("visibility", symbol.visibility));
		meta.appendChild(metaRow("file", symbol.file));
		if (symbol.line_range) {
			meta.appendChild(metaRow("lines", symbol.line_range[0] + "–" + symbol.line_range[1]));
		}
		meta.appendChild(metaRow("moniker", symbol.uri));
		box.appendChild(meta);
		return box;
	}

	function openSourceLink(source: OpenableSource, text: string): HTMLButtonElement {
		const link = el("button", "source-link", text);
		link.type = "button";
		link.addEventListener("click", (event) => {
			event.preventDefault();
			saveViewState();
			vscode.postMessage({
				type: "openSource",
				target: {
					root: source.root,
					file: source.file,
					line: source.line_range ? source.line_range[0] : 1,
				},
			});
		});
		return link;
	}

	function sourceSection(symbol: SymbolDto, source: HighlightedSourceSnippet): HTMLElement {
		const box = section("Source");
		box.body.appendChild(sourceCodeBlock(source, symbol.line_range));
		return box.root;
	}

	function usagesSection(
		title: string,
		rows: HighlightedUsageDto[],
		summary: UsageSummaryDto | null,
		scope: UsageDirectionScope,
	): HTMLElement {
		const box = section(title + " (" + rows.length + ")");
		if (summary && summary.dominant_prefix) {
			box.body.appendChild(el("div", "summary", summary.shared_helper_signal + " · " + summary.dominant_prefix));
		}
		if (rows.length === 0) {
			box.body.appendChild(el("div", "empty-row", "none"));
			return box.root;
		}
		box.body.appendChild(usageNavigator(rows, scope));
		return box.root;
	}

	function usageNavigator(rows: HighlightedUsageDto[], scope: UsageDirectionScope): HTMLElement {
		const tree = el("div", "usage-tree");
		for (const bucket of usageBuckets(rows)) {
			if (bucket.rows.length === 0) {
				continue;
			}
			const bucketNode = details("usage-bucket", bucket.kind !== "technical", `${scope}:bucket:${bucket.kind}`);
			bucketNode.summary.appendChild(
				usageSummaryLine(bucket.label, bucketMeta(bucket.rows), bucket.rows.length, bucket.kind),
			);
			for (const group of groupUsages(bucket.rows, bucket.kind, scope)) {
				bucketNode.body.appendChild(usageFileNode(group));
			}
			tree.appendChild(bucketNode.root);
		}
		return tree;
	}

	function usageFileNode(group: UsageFileGroup): HTMLDetailsElement {
		const fileNode = details("usage-file", true, `${group.scope}:file:${group.bucket}:${group.file}`);
		fileNode.summary.appendChild(fileSummaryLine(group));
		fileNode.root.title = group.file;
		for (const context of group.contexts) {
			const contextNode = details(
				"usage-context",
				group.contexts.length <= 3,
				`${group.scope}:context:${group.bucket}:${group.file}:${context.label}`,
			);
			contextNode.summary.appendChild(
				usageSummaryLine(compactSymbol(context.label), actionMeta(context.rows), context.rows.length, "context"),
			);
			contextNode.root.title = context.label;
			for (const occurrence of context.occurrences) {
				contextNode.body.appendChild(usageItem(occurrence));
			}
			fileNode.body.appendChild(contextNode.root);
		}
		return fileNode.root;
	}

	function details(className: string, open: boolean, stateKey: string): DetailsNode {
		const root = document.createElement("details");
		root.className = className;
		root.open = open;
		root.dataset.stateKey = stateKey;
		root.addEventListener("toggle", scheduleViewStateSave);
		const summary = document.createElement("summary");
		const body = el("div", "details-body");
		root.appendChild(summary);
		root.appendChild(body);
		return { root, summary, body };
	}

	function usageSummaryLine(label: string, meta: string, count: number, kind: UsageSummaryKind): HTMLElement {
		const row = el("span", "usage-summary-line");
		row.appendChild(el("span", "usage-summary-kind usage-kind-" + kind, kindLabel(kind)));
		const text = el("span", "usage-summary-text");
		text.appendChild(el("span", "usage-summary-label", label || "unknown"));
		if (meta) {
			text.appendChild(el("span", "usage-summary-meta", meta));
		}
		row.appendChild(text);
		row.appendChild(el("span", "usage-summary-count", String(count)));
		return row;
	}

	function fileSummaryLine(group: UsageFileGroup): HTMLElement {
		const row = el("span", "usage-summary-line");
		row.appendChild(el("span", "usage-summary-kind usage-kind-file", fileKind(group.file)));
		const file = splitFile(group.file);
		const text = el("span", "usage-summary-text");
		text.appendChild(el("span", "usage-summary-label", file.name));
		if (file.dir) {
			text.appendChild(el("span", "usage-summary-meta", file.dir));
		}
		row.appendChild(text);
		row.appendChild(el("span", "usage-summary-count", String(group.rows.length)));
		return row;
	}

	function usageItem(occurrence: UsageOccurrence): HTMLElement {
		const item = el("div", "usage-item");
		item.dataset.previewKey = occurrence.key;
		const row = usageLeaf(occurrence);
		item.appendChild(row);
		row.addEventListener("click", (event) => {
			event.preventDefault();
			toggleUsagePreview(item, occurrence);
		});
		return item;
	}

	function usageLeaf(occurrence: UsageOccurrence): HTMLButtonElement {
		const row = el("button", "usage-leaf");
		row.type = "button";
		row.title = occurrenceTooltip(occurrence);
		row.setAttribute("aria-expanded", "false");
		row.appendChild(el("span", "usage-action", usageAction(occurrence.kind)));
		row.appendChild(el("span", "usage-actor", occurrence.label));
		const hint = occurrence.sample.line_range
			? (occurrence.rows.length > 1 ? `Show code · ${occurrence.rows.length} refs` : "Show code")
			: `${occurrence.rows.length} ref${occurrence.rows.length > 1 ? "s" : ""}`;
		row.appendChild(el("span", "usage-preview-hint", hint));
		return row;
	}

	function toggleUsagePreview(item: HTMLElement, occurrence: UsageOccurrence): void {
		const current = item.querySelector<HTMLElement>(":scope > .usage-preview");
		const button = item.querySelector<HTMLButtonElement>(":scope > .usage-leaf");
		if (current) {
			item.classList.remove("open");
			button?.setAttribute("aria-expanded", "false");
			const hint = button?.querySelector(".usage-preview-hint");
			if (hint && occurrence.sample.line_range) {
				hint.textContent = occurrence.rows.length > 1 ? `Show code · ${occurrence.rows.length} refs` : "Show code";
			}
			current.remove();
			if (!restoringViewState) {
				saveViewState();
			}
			return;
		}
		item.classList.add("open");
		button?.setAttribute("aria-expanded", "true");
		const hint = button?.querySelector(".usage-preview-hint");
		if (hint && occurrence.sample.line_range) {
			hint.textContent = "Hide code";
		}
		const preview = el("div", "usage-preview");
		renderUsagePreview(preview, occurrence);
		preview.appendChild(openSourceLink(occurrence.sample, "Open source"));
		item.appendChild(preview);
		if (!restoringViewState) {
			saveViewState();
		}
	}

	function renderUsagePreview(preview: HTMLElement, occurrence: UsageOccurrence): void {
		preview.replaceChildren();
		if (occurrence.sample.snippet) {
			preview.appendChild(sourceCodeBlock(occurrence.sample.snippet, occurrence.sample.line_range, "compact"));
			return;
		}
		if (occurrence.sample.snippet === null || !occurrence.sample.line_range) {
			preview.appendChild(el("div", "empty-row", "No preview available."));
			return;
		}
		preview.appendChild(el("div", "empty-row", "Loading source..."));
		requestUsageSnippet(occurrence);
	}

	function requestUsageSnippet(occurrence: UsageOccurrence): void {
		const requestId = "usage-snippet:" + (++snippetRequestSeq);
		snippetRequests.set(requestId, occurrence);
		vscode.postMessage({
			type: "loadUsageSnippet",
			requestId,
			target: occurrence.sample,
		});
	}

	function receiveUsageSnippet(requestId: string, snippet: HighlightedSourceSnippet | null): void {
		const occurrence = snippetRequests.get(requestId);
		snippetRequests.delete(requestId);
		if (!occurrence) {
			return;
		}
		occurrence.sample.snippet = snippet;
		const item = findUsageItem(occurrence.key);
		const preview = item?.querySelector<HTMLElement>(":scope > .usage-preview");
		if (!preview) {
			return;
		}
		renderUsagePreview(preview, occurrence);
		preview.appendChild(openSourceLink(occurrence.sample, "Open source"));
	}

	function findUsageItem(previewKey: string): HTMLElement | undefined {
		return Array.from(root.querySelectorAll<HTMLElement>(".usage-item[data-preview-key]"))
			.find((item) => item.dataset.previewKey === previewKey);
	}

	function occurrenceTooltip(occurrence: UsageOccurrence): string {
		const usage = occurrence.sample;
		return [
			usageAction(usage.kind),
			occurrence.label,
			usage.file,
			occurrence.rows.length > 1 ? `${occurrence.rows.length} references` : usage.location,
		].filter(Boolean).join(" · ");
	}

	function usageTarget(usage: HighlightedUsageDto): string {
		return compactSymbol(usage.endpoint || usage.actor || usage.context || usage.prefix || "usage");
	}

	function usageAction(kind: string): string {
		const normalized = kind.toLowerCase();
		const labels: Record<string, string> = {
			calls: "calls",
			method_call: "calls",
			reads: "reads",
			writes: "writes",
			instantiates: "creates",
			extends: "extends",
			implements: "implements",
			annotates: "annotates",
			returns_type: "returns type",
			uses_type: "uses type",
			imports_symbol: "imports",
			imports_module: "imports",
		};
		return labels[normalized] || normalized.replaceAll("_", " ");
	}

	function kindLabel(kind: UsageSummaryKind): string {
		const labels: Record<UsageSummaryKind, string> = {
			production: "code",
			test: "tests",
			technical: "types",
			context: "scope",
			file: "file",
		};
		return labels[kind] || kind;
	}

	function sourceCodeBlock(
		source: HighlightedSourceSnippet,
		active?: LineRange | null,
		density?: "compact",
	): HTMLElement {
		const code = el("div", density === "compact" ? "code code-compact" : "code");
		for (const line of source.lines) {
			const row = el("div", "code-line");
			if (active && line.number >= active[0] && line.number <= active[1]) {
				row.classList.add("active");
			}
			row.appendChild(el("span", "gutter", String(line.number)));
			const src = el("code", "src");
			appendTokens(src, line);
			row.appendChild(src);
			code.appendChild(row);
		}
		return code;
	}

	function usageBuckets(rows: HighlightedUsageDto[]): UsageBucket[] {
		const buckets: UsageBucket[] = [
			{ kind: "production", label: "Production", rows: [] },
			{ kind: "test", label: "Tests", rows: [] },
			{ kind: "technical", label: "Type-only and imports", rows: [] },
		];
		for (const usage of rows) {
			buckets[bucketIndex(usage)].rows.push(usage);
		}
		return buckets;
	}

	function bucketIndex(usage: HighlightedUsageDto): 0 | 1 | 2 {
		if (isTechnicalUsage(usage)) {
			return 2;
		}
		if (isTestFile(usage.file)) {
			return 1;
		}
		return 0;
	}

	function isTechnicalUsage(usage: HighlightedUsageDto): boolean {
		const kind = usage.kind.toLowerCase();
		return kind.startsWith("imports_")
			|| kind === "uses_type"
			|| kind === "returns_type"
			|| kind === "annotates";
	}

	function isTestFile(file: string): boolean {
		return /(^|[/.])(__tests__|tests?|specs?)([/.]|$)/i.test(file)
			|| /\.(test|spec)\.[^.]+$/i.test(file);
	}

	function splitFile(file: string): { dir: string; name: string } {
		const parts = String(file || "unknown").split("/");
		const name = parts.pop() || "unknown";
		const dir = parts.slice(-2).join("/");
		return { dir, name };
	}

	function compactSymbol(value: string): string {
		return String(value || "unknown")
			.replace(/\s+/g, " ")
			.replace(/\(([^)]{56})[^)]*\)/, "($1...)");
	}

	function groupUsages(
		rows: HighlightedUsageDto[],
		bucket: UsageBucketKind,
		scope: UsageDirectionScope,
	): UsageFileGroup[] {
		const files = new Map<string, HighlightedUsageDto[]>();
		for (const usage of rows) {
			const file = usage.file || "unknown";
			if (!files.has(file)) {
				files.set(file, []);
			}
			files.get(file)?.push(usage);
		}
		return Array.from(files.entries())
			.map(([file, fileRows]) => ({
				bucket,
				file,
				rows: sortUsages(fileRows),
				scope,
				contexts: groupUsageContexts(sortUsages(fileRows)),
			}))
			.sort((a, b) => b.rows.length - a.rows.length || a.file.localeCompare(b.file));
	}

	function groupUsageContexts(rows: HighlightedUsageDto[]): UsageContextGroup[] {
		const contexts = new Map<string, HighlightedUsageDto[]>();
		for (const usage of rows) {
			const label = usage.actor || usage.context || usage.endpoint || usage.prefix || "unknown";
			if (!contexts.has(label)) {
				contexts.set(label, []);
			}
			contexts.get(label)?.push(usage);
		}
		return Array.from(contexts.entries())
			.map(([label, contextRows]) => ({
				label,
				rows: sortUsages(contextRows),
				occurrences: groupOccurrences(sortUsages(contextRows)),
			}))
			.sort((a, b) => b.rows.length - a.rows.length || a.label.localeCompare(b.label));
	}

	function groupOccurrences(rows: HighlightedUsageDto[]): UsageOccurrence[] {
		const groups = new Map<string, HighlightedUsageDto[]>();
		for (const usage of rows) {
			const key = usage.kind.toLowerCase() + ":" + usageTarget(usage);
			if (!groups.has(key)) {
				groups.set(key, []);
			}
			groups.get(key)?.push(usage);
		}
		return Array.from(groups.entries())
			.flatMap(([key, groupRows]) => {
				const first = groupRows[0];
				if (!first) {
					return [];
				}
				return [{
					key: "usage:" + first.direction + ":" + key + ":" + first.file + ":" + first.location,
					kind: first.kind,
					label: usageTarget(first),
					rows: groupRows,
					sample: previewSample(groupRows),
				}];
			})
			.sort((a, b) => actionRank(a.kind) - actionRank(b.kind) || b.rows.length - a.rows.length || a.label.localeCompare(b.label));
	}

	function previewSample(rows: HighlightedUsageDto[]): HighlightedUsageDto {
		return rows.find((row) => row.line_range && !isTechnicalUsage(row))
			|| rows.find((row) => row.line_range)
			|| rows[0];
	}

	function sortUsages(rows: HighlightedUsageDto[]): HighlightedUsageDto[] {
		return [...rows].sort((a, b) =>
			actionRank(a.kind) - actionRank(b.kind)
			|| usageTarget(a).localeCompare(usageTarget(b))
			|| String(a.location || "").localeCompare(String(b.location || "")),
		);
	}

	function actionRank(kind: string): number {
		const normalized = kind.toLowerCase();
		const ranks: Record<string, number> = {
			calls: 0,
			method_call: 0,
			instantiates: 1,
			writes: 2,
			reads: 3,
			extends: 4,
			implements: 4,
			uses_type: 8,
			returns_type: 8,
			imports_symbol: 9,
			imports_module: 9,
		};
		return ranks[normalized] ?? 5;
	}

	function bucketMeta(rows: HighlightedUsageDto[]): string {
		const files = new Set(rows.map((row) => row.file || "unknown")).size;
		const contexts = new Set(rows.map((row) => row.actor || row.context || row.endpoint || row.prefix || "unknown")).size;
		return `${files} file${files > 1 ? "s" : ""} · ${contexts} scope${contexts > 1 ? "s" : ""}`;
	}

	function actionMeta(rows: HighlightedUsageDto[]): string {
		const counts = new Map<string, number>();
		for (const row of rows) {
			const action = usageAction(row.kind);
			counts.set(action, (counts.get(action) || 0) + 1);
		}
		return Array.from(counts.entries())
			.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
			.slice(0, 2)
			.map(([action, count]) => count > 1 ? `${count} ${action}` : action)
			.join(" · ");
	}

	function fileKind(file: string): string {
		if (isTestFile(file)) {
			return "test";
		}
		return file.split(".").pop()?.toLowerCase() || "file";
	}

	function renderWithState(key: string, build: () => void): void {
		const previous = readViewState(key);
		build();
		restoreViewState(previous);
	}

	function readViewState(key: string): PersistedViewState {
		const persisted = vscode.getState() || {};
		const sameView = persisted.key === key;
		return {
			hasSavedState: sameView,
			key,
			scrollY: sameView ? (persisted.scrollY ?? window.scrollY) : 0,
			openDetails: sameView ? (persisted.openDetails || []) : [],
			openPreviews: sameView ? (persisted.openPreviews || []) : [],
		};
	}

	function restoreViewState(state: PersistedViewState): void {
		restoringViewState = true;
		const openDetails = new Set(state.openDetails);
		if (state.hasSavedState) {
			for (const node of Array.from(root.querySelectorAll<HTMLDetailsElement>("details[data-state-key]"))) {
				const stateKey = node.dataset.stateKey;
				node.open = Boolean(stateKey && openDetails.has(stateKey));
			}
		}
		const openPreviews = new Set(state.openPreviews);
		for (const item of Array.from(root.querySelectorAll<HTMLElement>(".usage-item[data-preview-key]"))) {
			const previewKey = item.dataset.previewKey;
			if (previewKey && openPreviews.has(previewKey)) {
				const row = item.querySelector<HTMLButtonElement>(":scope > .usage-leaf");
				row?.click();
			}
		}
		restoringViewState = false;
		requestAnimationFrame(() => {
			window.scrollTo(0, state.scrollY || 0);
			saveViewState(state.key);
		});
	}

	function scheduleViewStateSave(): void {
		if (restoringViewState) {
			return;
		}
		if (saveTimer !== undefined) {
			cancelAnimationFrame(saveTimer);
		}
		saveTimer = requestAnimationFrame(() => {
			saveTimer = undefined;
			saveViewState();
		});
	}

	function saveViewState(key?: string): void {
		const previous = vscode.getState() || {};
		vscode.setState({
			key: key || previous.key,
			scrollY: window.scrollY,
			openDetails: Array.from(root.querySelectorAll<HTMLDetailsElement>("details[data-state-key]"))
				.filter((node) => node.open)
				.map((node) => node.dataset.stateKey)
				.filter((stateKey): stateKey is string => Boolean(stateKey)),
			openPreviews: Array.from(root.querySelectorAll<HTMLElement>(".usage-item.open[data-preview-key]"))
				.map((node) => node.dataset.previewKey)
				.filter((previewKey): previewKey is string => Boolean(previewKey)),
		});
	}

	function section(titleText: string): { body: HTMLDivElement; root: HTMLDivElement } {
		const root = el("div", "section");
		root.appendChild(el("div", "section-title", titleText));
		const body = el("div", "section-body");
		root.appendChild(body);
		return { root, body };
	}

	function metaRow(label: string, value: string): HTMLElement {
		const row = el("div", "meta-row");
		row.appendChild(el("span", "meta-label", label));
		row.appendChild(el("span", "meta-value", value || "—"));
		return row;
	}

	function detailRow(label: string, value: string): HTMLElement {
		const row = el("div", "detail-row");
		row.appendChild(el("span", "detail-label", label));
		row.appendChild(el("span", "detail-value", value || "—"));
		return row;
	}

	function appendTokens(parent: HTMLElement, line: HighlightedSourceLine): void {
		const tokens: HighlightedSourceToken[] = line.tokens && line.tokens.length > 0
			? line.tokens
			: [{ text: line.text || " " }];
		for (const token of tokens) {
			const span = el("span", "tok", token.text);
			if (isHexColor(token.lightColor)) {
				span.style.setProperty("--tok-light", token.lightColor);
			}
			if (isHexColor(token.darkColor)) {
				span.style.setProperty("--tok-dark", token.darkColor);
			}
			if (token.fontStyle) {
				applyFontStyle(span, token.fontStyle);
			}
			parent.appendChild(span);
		}
	}

	function isHexColor(value: unknown): value is string {
		return typeof value === "string" && /^#[0-9a-fA-F]{3,8}$/.test(value);
	}

	function applyFontStyle(span: HTMLElement, fontStyle: number): void {
		if ((fontStyle & 1) !== 0) {
			span.style.fontStyle = "italic";
		}
		if ((fontStyle & 2) !== 0) {
			span.style.fontWeight = "600";
		}
		if ((fontStyle & 4) !== 0) {
			span.style.textDecoration = "underline";
		}
	}

	function el<K extends keyof HTMLElementTagNameMap>(
		tag: K,
		className?: string,
		text?: string,
	): HTMLElementTagNameMap[K] {
		const node = document.createElement(tag);
		if (className) {
			node.className = className;
		}
		if (text !== undefined) {
			node.textContent = text;
		}
		return node;
	}
})();
