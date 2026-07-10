import type { ReactNode } from "react";

import { vscode } from "./vscodeApi";
import { useViewActions } from "./viewContext";

export function Section({ title, children }: { title: string; children: ReactNode }) {
	return (
		<div className="section">
			<div className="section-title">{title}</div>
			<div className="section-body">{children}</div>
		</div>
	);
}

export function MetaRow({ label, value }: { label: string; value: string }) {
	return (
		<div className="meta-row">
			<span className="meta-label">{label}</span>
			<span className="meta-value">{value || "—"}</span>
		</div>
	);
}

export function DetailRow({ label, value }: { label: string; value: string }) {
	return (
		<div className="detail-row">
			<span className="detail-label">{label}</span>
			<span className="detail-value">{value || "—"}</span>
		</div>
	);
}

export interface OpenableSource {
	file: string;
	line_range?: [number, number] | null;
	root: string;
}

export function OpenSourceButton({ source, text }: { source: OpenableSource; text: string }) {
	return (
		<button
			type="button"
			className="source-link"
			onClick={() =>
				vscode.postMessage({
					type: "openSource",
					target: {
						root: source.root,
						file: source.file,
						line: source.line_range ? source.line_range[0] : 1,
					},
				})
			}
		>
			{text}
		</button>
	);
}

// Controlled <details>: the open set lives in App state so it can be
// persisted and restored across webview reloads.
export function Details({
	className,
	stateKey,
	summary,
	title,
	children,
}: {
	className: string;
	stateKey: string;
	summary: ReactNode;
	title?: string;
	children: ReactNode;
}) {
	const view = useViewActions();
	const open = view.openDetails.has(stateKey);
	return (
		<details
			className={className}
			open={open}
			title={title}
			onToggle={(event) => {
				const now = (event.currentTarget as HTMLDetailsElement).open;
				if (now !== open) {
					view.setDetailOpen(stateKey, now);
				}
			}}
		>
			<summary>{summary}</summary>
			<div className="details-body">{children}</div>
		</details>
	);
}
