// Notebook output renderer: shows the compiled rules, then the sample code with
// violating lines highlighted and each violation message. No framework, just DOM.
// Payload types are shared with the extension host (type-only import, erased at
// bundle time) so the contract lives in one place.

import type { RuleSpec, Violation, ViolationsPayload } from "../src/cli/model";

interface OutputItem {
	json(): ViolationsPayload;
}

interface RendererApi {
	renderOutputItem(item: OutputItem, element: HTMLElement): void;
	disposeOutputItem?(id: string): void;
}

const STYLE_ID = "code-moniker-violations-style";

export function activate(): RendererApi {
	return {
		renderOutputItem(item: OutputItem, element: HTMLElement): void {
			injectStyle();
			element.replaceChildren(render(item.json()));
		},
	};
}

function render(payload: ViolationsPayload): HTMLElement {
	const root = document.createElement("div");
	root.className = "cm-root";
	root.appendChild(header(payload));
	root.appendChild(ruleList(payload.rules));
	root.appendChild(codeBlock(payload.sample, flaggedLines(payload.violations)));
	if (payload.violations.length > 0) {
		root.appendChild(messageList(payload.violations));
	}
	return root;
}

function header(payload: ViolationsPayload): HTMLElement {
	const el = document.createElement("div");
	el.className = payload.total === 0 ? "cm-header cm-pass" : "cm-header cm-fail";
	const badge =
		payload.total === 0
			? "✓ PASS"
			: `✗ ${payload.total} violation(s)`;
	el.innerHTML =
		`<span class="cm-badge">${escapeHtml(badge)}</span>` +
		`<span class="cm-scope">${escapeHtml(String(payload.rules.length))} rule(s) · ${escapeHtml(payload.language)}</span>`;
	return el;
}

function ruleList(rules: RuleSpec[]): HTMLElement {
	const ul = document.createElement("ul");
	ul.className = "cm-rules";
	for (const rule of rules) {
		const li = document.createElement("li");

		const sev = document.createElement("span");
		sev.className = rule.severity === "warn" ? "cm-sev cm-warn" : "cm-sev cm-error";
		sev.textContent = rule.severity;

		const id = document.createElement("code");
		id.className = "cm-rid";
		id.textContent = rule.rule_id;

		const expr = document.createElement("code");
		expr.className = "cm-rexpr";
		expr.textContent = rule.expr;

		li.appendChild(sev);
		li.appendChild(id);
		li.appendChild(expr);
		if (rule.rationale) {
			const why = document.createElement("div");
			why.className = "cm-why";
			why.textContent = rule.rationale;
			li.appendChild(why);
		}
		ul.appendChild(li);
	}
	return ul;
}

function flaggedLines(violations: Violation[]): Map<number, Violation> {
	const map = new Map<number, Violation>();
	for (const v of violations) {
		const [start, end] = v.lines;
		for (let line = start; line <= end; line++) {
			if (!map.has(line)) {
				map.set(line, v);
			}
		}
	}
	return map;
}

function codeBlock(source: string, flagged: Map<number, Violation>): HTMLElement {
	const pre = document.createElement("pre");
	pre.className = "cm-code";
	const lines = source.replace(/\n$/, "").split("\n");
	lines.forEach((text, idx) => {
		const lineNo = idx + 1;
		const row = document.createElement("div");
		row.className = flagged.has(lineNo) ? "cm-line cm-line-bad" : "cm-line";

		const gutter = document.createElement("span");
		gutter.className = "cm-gutter";
		gutter.textContent = String(lineNo);

		const code = document.createElement("span");
		code.className = "cm-src";
		code.textContent = text.length ? text : " ";

		row.appendChild(gutter);
		row.appendChild(code);
		pre.appendChild(row);
	});
	return pre;
}

function messageList(violations: Violation[]): HTMLElement {
	const ul = document.createElement("ul");
	ul.className = "cm-messages";
	for (const v of violations) {
		const li = document.createElement("li");
		const where = document.createElement("span");
		where.className = "cm-where";
		where.textContent = `L${v.lines[0]}-L${v.lines[1]}`;
		const msg = document.createElement("span");
		msg.className = "cm-msg";
		msg.textContent = v.explanation ? `${v.explanation}  (${v.message})` : v.message;
		li.appendChild(where);
		li.appendChild(msg);
		ul.appendChild(li);
	}
	return ul;
}

function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

function injectStyle(): void {
	if (document.getElementById(STYLE_ID)) {
		return;
	}
	const style = document.createElement("style");
	style.id = STYLE_ID;
	style.textContent = `
.cm-root { font-family: var(--vscode-editor-font-family, monospace); font-size: 13px; margin: 4px 0; }
.cm-header { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 4px 4px 0 0; }
.cm-pass { background: rgba(45, 164, 78, 0.15); }
.cm-fail { background: rgba(248, 81, 73, 0.15); }
.cm-badge { font-weight: 700; }
.cm-pass .cm-badge { color: var(--vscode-testing-iconPassed, #2da44e); }
.cm-fail .cm-badge { color: var(--vscode-testing-iconFailed, #f85149); }
.cm-scope { margin-left: auto; opacity: 0.7; font-size: 12px; }
.cm-rules { list-style: none; margin: 0; padding: 6px 8px; background: var(--vscode-textCodeBlock-background, rgba(127,127,127,0.06)); }
.cm-rules li { padding: 2px 0; }
.cm-sev { font-size: 10px; font-weight: 700; text-transform: uppercase; padding: 1px 5px; border-radius: 3px; margin-right: 8px; }
.cm-error { background: rgba(248, 81, 73, 0.25); color: var(--vscode-testing-iconFailed, #f85149); }
.cm-warn { background: rgba(210, 153, 34, 0.25); color: var(--vscode-editorWarning-foreground, #d29922); }
.cm-rid { opacity: 0.8; margin-right: 10px; }
.cm-rexpr { background: var(--vscode-textCodeBlock-background, rgba(127,127,127,0.1)); padding: 1px 6px; border-radius: 3px; }
.cm-why { opacity: 0.65; font-size: 12px; margin: 1px 0 4px 44px; }
.cm-code { margin: 0; padding: 6px 0; background: var(--vscode-textCodeBlock-background, rgba(127,127,127,0.06)); border-radius: 0 0 4px 4px; overflow-x: auto; }
.cm-line { display: flex; white-space: pre; }
.cm-line-bad { background: rgba(248, 81, 73, 0.18); box-shadow: inset 2px 0 0 var(--vscode-testing-iconFailed, #f85149); }
.cm-gutter { display: inline-block; width: 3em; text-align: right; padding-right: 1em; opacity: 0.5; user-select: none; }
.cm-src { flex: 1; }
.cm-messages { list-style: none; margin: 8px 0 0; padding: 0; }
.cm-messages li { display: flex; gap: 10px; padding: 3px 8px; }
.cm-where { color: var(--vscode-testing-iconFailed, #f85149); font-variant-numeric: tabular-nums; min-width: 5.5em; }
.cm-msg { flex: 1; }
`;
	document.head.appendChild(style);
}
