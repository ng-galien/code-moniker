import type { ExplorerMessage } from "../protocol";

// Typed handle on the VS Code webview bridge; acquireVsCodeApi is injected by
// the webview runtime and can only be called once per document.
interface WebviewApi {
	postMessage(message: ExplorerMessage): void;
	getState(): unknown;
	setState(state: unknown): void;
}

declare function acquireVsCodeApi(): WebviewApi;

export const vscode = acquireVsCodeApi();
