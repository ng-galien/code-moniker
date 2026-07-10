import { createContext, useContext } from "react";

import type { HighlightedSourceSnippet } from "../highlight";
import type { UsageOccurrence } from "../usageModel";

// View-state surface shared down the usage tree: which <details> and code
// previews are open (persisted across webview reloads), plus lazily loaded
// snippets keyed by occurrence.
export interface ViewActions {
	openDetails: ReadonlySet<string>;
	openPreviews: ReadonlySet<string>;
	snippets: ReadonlyMap<string, HighlightedSourceSnippet | null>;
	setDetailOpen(key: string, open: boolean): void;
	setPreviewOpen(key: string, open: boolean): void;
	ensureSnippet(occurrence: UsageOccurrence): void;
}

export const ViewContext = createContext<ViewActions>({
	openDetails: new Set(),
	openPreviews: new Set(),
	snippets: new Map(),
	setDetailOpen: () => {},
	setPreviewOpen: () => {},
	ensureSnippet: () => {},
});

export function useViewActions(): ViewActions {
	return useContext(ViewContext);
}
