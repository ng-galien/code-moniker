import type { UsageDto } from "../../../daemon/model";
import type { SourceTarget } from "../panel";

export interface PersistedViewState {
	key?: string;
	openDetails?: string[];
	openPreviews?: string[];
	scrollY?: number;
}

export type DetailOutboundMessage =
	| { type: "ready" }
	| { type: "openSource"; target: SourceTarget }
	| { type: "loadUsageSnippet"; requestId: string; target: UsageDto };

interface WebviewApi {
	getState(): PersistedViewState | undefined;
	postMessage(message: DetailOutboundMessage): void;
	setState(state: PersistedViewState): void;
}

declare function acquireVsCodeApi(): WebviewApi;

export const vscode = acquireVsCodeApi();
