import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { HighlightedSourceSnippet } from "../highlight";
import type { DetailDocument, DetailPayload } from "../panel";
import { groupUsages, isTypeSymbolKind, usageBuckets, type UsageOccurrence } from "../usageModel";
import { DetailView, DocumentView } from "./DetailView";
import { vscode } from "./vscodeApi";
import { ViewContext, type ViewActions } from "./viewContext";

type ViewModel =
	| { kind: "detail"; key: string; payload: DetailPayload }
	| { kind: "document"; key: string; payload: DetailDocument };

// Symbol detail webview: renders detail/document messages, persists the view
// state (open folds, open previews, scroll) through the webview state bridge,
// and resolves usage snippets lazily via request/response with the host.
export function App() {
	const [view, setView] = useState<ViewModel | null>(null);
	const [openDetails, setOpenDetails] = useState<ReadonlySet<string>>(new Set());
	const [openPreviews, setOpenPreviews] = useState<ReadonlySet<string>>(new Set());
	const [snippets, setSnippets] = useState<ReadonlyMap<string, HighlightedSourceSnippet | null>>(
		new Map(),
	);
	const pendingSnippets = useRef(new Map<string, string>());
	const requestedSnippets = useRef(new Set<string>());
	const requestSeq = useRef(0);
	const restoreScroll = useRef<number | null>(null);

	const applyView = useCallback((next: ViewModel) => {
		const persisted = vscode.getState() || {};
		const same = persisted.key === next.key;
		restoreScroll.current = same ? (persisted.scrollY ?? 0) : 0;
		pendingSnippets.current = new Map();
		requestedSnippets.current = new Set();
		setSnippets(new Map());
		setOpenDetails(same ? new Set(persisted.openDetails || []) : defaultOpenDetails(next));
		setOpenPreviews(same ? new Set(persisted.openPreviews || []) : new Set());
		setView(next);
	}, []);

	useEffect(() => {
		const onMessage = (event: MessageEvent) => {
			const message = event.data;
			if (message?.type === "detail") {
				const payload = message.payload as DetailPayload;
				applyView({ kind: "detail", key: "detail:" + payload.symbol.uri, payload });
			} else if (message?.type === "document") {
				const payload = message.payload as DetailDocument;
				applyView({ kind: "document", key: "document:" + payload.title, payload });
			} else if (message?.type === "usageSnippet") {
				const key = pendingSnippets.current.get(message.requestId);
				pendingSnippets.current.delete(message.requestId);
				if (key !== undefined) {
					const snippet = message.snippet as HighlightedSourceSnippet | null;
					setSnippets((previous) => new Map(previous).set(key, snippet));
				}
			}
		};
		window.addEventListener("message", onMessage);
		vscode.postMessage({ type: "ready" });
		return () => window.removeEventListener("message", onMessage);
	}, [applyView]);

	// Persist fold/preview state as it changes; scroll is persisted separately
	// on scroll events so it survives without re-rendering.
	useEffect(() => {
		if (!view) {
			return;
		}
		vscode.setState({
			key: view.key,
			scrollY: window.scrollY,
			openDetails: [...openDetails],
			openPreviews: [...openPreviews],
		});
	}, [view, openDetails, openPreviews]);

	useEffect(() => {
		let frame: number | undefined;
		const onScroll = () => {
			if (frame !== undefined) {
				return;
			}
			frame = requestAnimationFrame(() => {
				frame = undefined;
				vscode.setState({ ...(vscode.getState() || {}), scrollY: window.scrollY });
			});
		};
		window.addEventListener("scroll", onScroll, { passive: true });
		return () => {
			window.removeEventListener("scroll", onScroll);
			if (frame !== undefined) {
				cancelAnimationFrame(frame);
			}
		};
	}, []);

	useLayoutEffect(() => {
		if (view && restoreScroll.current != null) {
			const y = restoreScroll.current;
			restoreScroll.current = null;
			requestAnimationFrame(() => window.scrollTo(0, y));
		}
	}, [view]);

	const ensureSnippet = useCallback((occurrence: UsageOccurrence) => {
		if (occurrence.sample.snippet !== undefined || requestedSnippets.current.has(occurrence.key)) {
			return;
		}
		requestedSnippets.current.add(occurrence.key);
		const requestId = "usage-snippet:" + ++requestSeq.current;
		pendingSnippets.current.set(requestId, occurrence.key);
		vscode.postMessage({ type: "loadUsageSnippet", requestId, target: occurrence.sample });
	}, []);

	const actions = useMemo<ViewActions>(
		() => ({
			openDetails,
			openPreviews,
			snippets,
			setDetailOpen: (key, open) =>
				setOpenDetails((previous) => withMembership(previous, key, open)),
			setPreviewOpen: (key, open) =>
				setOpenPreviews((previous) => withMembership(previous, key, open)),
			ensureSnippet,
		}),
		[openDetails, openPreviews, snippets, ensureSnippet],
	);

	if (!view) {
		return <div className="empty">Select a symbol to inspect it.</div>;
	}
	return (
		<ViewContext.Provider value={actions}>
			{view.kind === "detail" ? (
				<DetailView payload={view.payload} />
			) : (
				<DocumentView payload={view.payload} />
			)}
		</ViewContext.Provider>
	);
}

function withMembership(set: ReadonlySet<string>, key: string, member: boolean): Set<string> {
	const next = new Set(set);
	if (member) {
		next.add(key);
	} else {
		next.delete(key);
	}
	return next;
}

// Mirrors the pre-React defaults: production/tests buckets open, technical
// closed, files open, contexts open only when a file holds at most three.
function defaultOpenDetails(view: ViewModel): Set<string> {
	const open = new Set<string>();
	if (view.kind !== "detail") {
		return open;
	}
	const typeTarget = isTypeSymbolKind(view.payload.symbol.kind);
	for (const scope of ["incoming", "outgoing"] as const) {
		const rows = scope === "incoming" ? view.payload.incoming : view.payload.outgoing;
		for (const bucket of usageBuckets(rows, typeTarget)) {
			if (bucket.rows.length === 0) {
				continue;
			}
			if (bucket.kind !== "technical") {
				open.add(`${scope}:bucket:${bucket.kind}`);
			}
			for (const group of groupUsages(bucket.rows, bucket.kind, scope)) {
				open.add(`${scope}:file:${bucket.kind}:${group.file}`);
				if (group.contexts.length <= 3) {
					for (const context of group.contexts) {
						open.add(`${scope}:context:${bucket.kind}:${group.file}:${context.label}`);
					}
				}
			}
		}
	}
	return open;
}
