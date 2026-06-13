// Scenario output renderer: shows the compiled rules, then the sample code with
// violating lines highlighted and each violation message. No framework, just DOM.
// Payload types are shared with the extension host (type-only import, erased at
// bundle time) so the contract lives in one place.

import type {
	CheckFile,
	CheckOutputPayload,
	RendererMessage,
	Violation,
} from "../src/cli/model";
import {
	groupVisibleViolations,
	lineRangeLabel,
	severityCounts,
	visibleViolationDetail,
} from "../src/cli/presentation";

interface OutputItem {
	json(): CheckOutputPayload;
}

interface RendererApi {
	renderOutputItem(item: OutputItem, element: HTMLElement): void;
	disposeOutputItem?(id: string): void;
}

interface RendererContext {
	postMessage?(message: RendererMessage): unknown;
}

interface RendererMessenger {
	postMessage(message: RendererMessage): void;
}

const STYLE_ID = "code-moniker-violations-style";

export function activate(context?: RendererContext): RendererApi {
	const messenger = rendererMessenger(context);
	return {
		renderOutputItem(item: OutputItem, element: HTMLElement): void {
			injectStyle();
			element.replaceChildren(renderCheck(item.json(), messenger));
		},
	};
}

function renderCheck(
	payload: CheckOutputPayload,
	messenger: RendererMessenger | undefined,
): HTMLElement {
	const root = document.createElement("div");
	root.className = "cm-root";
	root.appendChild(checkHeader(payload));
	if (payload.files.length > 0) {
		root.appendChild(checkFileList(payload.files, messenger));
	} else {
		root.appendChild(emptyState("No matching files scanned."));
	}
	if (payload.errors?.length) {
		root.appendChild(checkErrorList(payload.errors));
	}
	return root;
}

function checkHeader(payload: CheckOutputPayload): HTMLElement {
	const el = document.createElement("div");
	const failed =
		payload.summary.total_violations > 0 || payload.summary.total_errors > 0;
	el.className = failed ? "cm-header cm-fail" : "cm-header cm-pass";
	el.appendChild(textSpan("cm-badge", failed ? "FAIL" : "PASS"));
	el.appendChild(textSpan("cm-target", payload.target));
	const metrics = document.createElement("div");
	metrics.className = "cm-metrics";
	metrics.appendChild(metric("files", payload.summary.files_scanned));
	metrics.appendChild(metric("violations", payload.summary.total_violations));
	metrics.appendChild(metric("errors", payload.summary.total_errors));
	metrics.appendChild(metric("warnings", payload.summary.total_warnings));
	el.appendChild(metrics);
	return el;
}

function checkFileList(
	files: CheckFile[],
	messenger: RendererMessenger | undefined,
): HTMLElement {
	const root = document.createElement("div");
	root.className = "cm-check-files";
	for (const file of files) {
		const counts = severityCounts(file.violations);
		const section = document.createElement("section");
		section.className = file.violations.length > 0
			? "cm-check-file cm-check-file-bad"
			: "cm-check-file";
		const title = document.createElement("div");
		title.className = "cm-check-file-title";
		const path = linkButton(
			file.file,
			`Reveal ${file.file} in the notebook`,
			{ command: "revealFile", file: file.file },
			messenger,
		);
		path.classList.add("cm-file-link");
		const count = document.createElement("span");
		count.className = file.violations.length > 0 ? "cm-file-count cm-file-count-bad" : "cm-file-count";
		count.textContent = file.violations.length > 0
			? `${counts.errors} error(s), ${counts.warnings} warning(s)`
			: "clean";
		title.appendChild(path);
		title.appendChild(count);
		section.appendChild(title);
		if (file.violations.length > 0) {
			section.appendChild(checkViolationList(file.file, file.violations, messenger));
		}
		root.appendChild(section);
	}
	return root;
}

function checkViolationList(
	file: string,
	violations: Violation[],
	messenger: RendererMessenger | undefined,
): HTMLElement {
	const ul = document.createElement("ul");
	ul.className = "cm-check-violations";
	for (const group of groupVisibleViolations(violations)) {
		const violation = group.violation;
		const li = document.createElement("li");
		li.className = violation.severity === "warn"
			? "cm-check-violation cm-check-violation-warn"
			: "cm-check-violation cm-check-violation-error";
		const where = linkButton(
			lineRangeLabel(violation.lines),
			`Reveal ${file}:${lineRangeLabel(violation.lines)} in the notebook`,
			{ command: "revealLine", file, line: violation.lines[0] },
			messenger,
		);
		where.classList.add("cm-where");
		const sev = document.createElement("span");
		sev.className = violation.severity === "warn" ? "cm-sev cm-warn" : "cm-sev cm-error";
		sev.textContent = violation.severity;
		const rule = linkButton(
			violation.rule_id,
			`Reveal rule ${violation.rule_id} in the notebook`,
			{ command: "revealRule", ruleId: violation.rule_id },
			messenger,
		);
		rule.classList.add("cm-rid");
		const kind = document.createElement("span");
		kind.className = "cm-kind";
		kind.textContent = visibleViolationDetail(group);
		const msg = document.createElement("span");
		msg.className = "cm-msg";
		msg.textContent = violation.explanation ?? violation.message;
		li.appendChild(where);
		li.appendChild(sev);
		li.appendChild(rule);
		li.appendChild(kind);
		li.appendChild(msg);
		ul.appendChild(li);
	}
	return ul;
}

function linkButton(
	text: string,
	title: string,
	message: RendererMessage,
	messenger: RendererMessenger | undefined,
): HTMLButtonElement {
	const button = document.createElement("button");
	button.type = "button";
	button.className = "cm-link";
	button.textContent = text;
	button.title = title;
	button.setAttribute("aria-label", title);
	if (!messenger) {
		button.disabled = true;
		button.title = "Notebook navigation is unavailable because renderer messaging is not initialized.";
		return button;
	}
	button.addEventListener("click", (event) => {
		event.preventDefault();
		event.stopPropagation();
		messenger.postMessage(message);
	});
	return button;
}

function rendererMessenger(
	context: RendererContext | undefined,
): RendererMessenger | undefined {
	if (typeof context?.postMessage === "function") {
		return {
			postMessage(message): void {
				void context.postMessage?.(message);
			},
		};
	}

	const webviewApi = (globalThis as {
		acquireVsCodeApi?: () => { postMessage(message: RendererMessage): void };
	}).acquireVsCodeApi?.();
	return webviewApi
		? { postMessage: (message) => webviewApi.postMessage(message) }
		: undefined;
}

function checkErrorList(errors: { file: string; error: string }[]): HTMLElement {
	const ul = document.createElement("ul");
	ul.className = "cm-check-errors";
	for (const error of errors) {
		const li = document.createElement("li");
		const path = document.createElement("code");
		path.textContent = error.file;
		const msg = document.createElement("span");
		msg.textContent = error.error;
		li.appendChild(path);
		li.appendChild(msg);
		ul.appendChild(li);
	}
	return ul;
}

function emptyState(text: string): HTMLElement {
	const el = document.createElement("div");
	el.className = "cm-empty";
	el.textContent = text;
	return el;
}

function textSpan(className: string, text: string): HTMLElement {
	const span = document.createElement("span");
	span.className = className;
	span.textContent = text;
	return span;
}

function metric(label: string, value: number): HTMLElement {
	const item = document.createElement("span");
	item.className = "cm-metric";
	item.appendChild(textSpan("cm-metric-value", String(value)));
	item.appendChild(textSpan("cm-metric-label", label));
	return item;
}

function injectStyle(): void {
	if (document.getElementById(STYLE_ID)) {
		return;
	}
	const style = document.createElement("style");
	style.id = STYLE_ID;
	style.textContent = `
.cm-root { color: var(--vscode-foreground); font-family: var(--vscode-font-family, system-ui, sans-serif); font-size: 13px; line-height: 1.42; margin: 4px 0; border: 1px solid var(--vscode-panel-border, rgba(127,127,127,0.22)); border-radius: 6px; overflow: hidden; background: var(--vscode-editor-background); }
.cm-root code { font-family: var(--vscode-editor-font-family, monospace); }
.cm-header { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 10px; padding: 8px 10px; border-bottom: 1px solid var(--vscode-panel-border, rgba(127,127,127,0.22)); }
.cm-target { font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cm-pass { background: color-mix(in srgb, var(--vscode-testing-iconPassed, #2da44e) 12%, transparent); }
.cm-fail { background: color-mix(in srgb, var(--vscode-testing-iconFailed, #f85149) 12%, transparent); }
.cm-badge { border-radius: 999px; font-size: 11px; font-weight: 700; letter-spacing: 0; padding: 2px 8px; }
.cm-pass .cm-badge { color: var(--vscode-testing-iconPassed, #2da44e); }
.cm-fail .cm-badge { color: var(--vscode-testing-iconFailed, #f85149); }
.cm-metrics { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 6px; }
.cm-metric { display: inline-flex; align-items: baseline; gap: 4px; border: 1px solid var(--vscode-panel-border, rgba(127,127,127,0.22)); border-radius: 999px; padding: 2px 7px; background: var(--vscode-editor-background); }
.cm-metric-value { font-weight: 700; font-variant-numeric: tabular-nums; }
.cm-metric-label { opacity: 0.72; font-size: 11px; }
.cm-sev { align-self: flex-start; font-size: 10px; font-weight: 700; text-transform: uppercase; padding: 1px 5px; border-radius: 3px; }
.cm-error { background: rgba(248, 81, 73, 0.25); color: var(--vscode-testing-iconFailed, #f85149); }
.cm-warn { background: rgba(210, 153, 34, 0.25); color: var(--vscode-editorWarning-foreground, #d29922); }
.cm-rid { font-family: var(--vscode-editor-font-family, monospace); color: var(--vscode-textLink-foreground, #3794ff); }
.cm-where { font-variant-numeric: tabular-nums; min-width: 5.5em; }
.cm-msg { flex: 1; }
.cm-check-files { background: var(--vscode-editor-background); }
.cm-check-file { border-top: 1px solid var(--vscode-panel-border, rgba(127,127,127,0.18)); padding: 8px 10px; }
.cm-check-file:first-child { border-top: 0; }
.cm-check-file-bad { box-shadow: inset 3px 0 0 var(--vscode-testing-iconFailed, #f85149); }
.cm-check-file-title { display: flex; align-items: center; gap: 10px; }
.cm-file-link { font-family: var(--vscode-editor-font-family, monospace); font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cm-check-file-title span { margin-left: auto; font-size: 12px; }
.cm-file-count { color: var(--vscode-testing-iconPassed, #2da44e); opacity: 0.9; }
.cm-file-count-bad { color: var(--vscode-testing-iconFailed, #f85149); opacity: 1; }
.cm-check-violations, .cm-check-errors { list-style: none; margin: 6px 0 0; padding: 0; }
.cm-check-violations li { display: grid; grid-template-columns: 5.5em 4.5em minmax(14em, auto) minmax(10em, auto) minmax(0, 1fr); gap: 8px; padding: 4px 0; align-items: start; }
.cm-check-errors li { display: grid; grid-template-columns: 5.5em 4.5em minmax(14em, auto) minmax(0, 1fr); gap: 8px; padding: 4px 0; align-items: start; }
.cm-check-violation-error { color: var(--vscode-foreground); }
.cm-kind { color: var(--vscode-descriptionForeground, var(--vscode-foreground)); font-size: 12px; }
.cm-link { appearance: none; border: 0; padding: 0; margin: 0; background: transparent; color: var(--vscode-textLink-foreground, #3794ff); cursor: pointer; font: inherit; text-align: left; }
.cm-link:hover { color: var(--vscode-textLink-activeForeground, var(--vscode-textLink-foreground, #3794ff)); text-decoration: underline; }
.cm-link:focus-visible { outline: 1px solid var(--vscode-focusBorder, #007fd4); outline-offset: 2px; border-radius: 2px; }
.cm-link:disabled { color: var(--vscode-disabledForeground, rgba(127,127,127,0.55)); cursor: not-allowed; text-decoration: none; }
.cm-check-errors { color: var(--vscode-errorForeground, #f85149); }
.cm-check-errors code { min-width: 10em; }
.cm-empty { padding: 10px; opacity: 0.7; background: var(--vscode-editor-background); }
@media (max-width: 640px) {
  .cm-header { grid-template-columns: 1fr; align-items: start; }
  .cm-metrics { justify-content: flex-start; }
  .cm-check-violations li, .cm-check-errors li { grid-template-columns: 4.5em 4em 1fr; }
  .cm-check-violations .cm-kind { grid-column: 1 / -1; }
  .cm-check-violations .cm-msg { grid-column: 1 / -1; padding-left: 0; }
}
`;
	document.head.appendChild(style);
}
