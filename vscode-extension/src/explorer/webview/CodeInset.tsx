import { useEffect, useRef } from "react";

import type { SymbolDto } from "../../daemon/model";
import type { HighlightedSourceSnippet } from "../../symbols/detail/highlight";
import { CodeBlock } from "../../webview-lib/CodeBlock";
import { parseCallableName } from "../../webview-lib/parse";
import { glyphClass, symbolGlyph } from "../../webview-lib/symbolGlyph";
import { postFocus, postOpenSource } from "./actions";

export interface InsetState {
	uri: string;
	symbol: SymbolDto | null;
	source: HighlightedSourceSnippet | null;
	loading: boolean;
}

// The graph and one definition's source remain visible side by side.
export function CodeInset({
	inset,
	onClose,
	width,
	onResize,
}: {
	inset: InsetState;
	onClose: () => void;
	width: number;
	onResize: (width: number) => void;
}) {
	const symbol = inset.symbol;
	const drag = useRef<{ x: number; width: number } | null>(null);
	const resize = (next: number) => onResize(Math.max(300, Math.min(window.innerWidth * 0.58, next)));
	useEffect(() => {
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key !== "Escape") return;
			event.preventDefault();
			onClose();
		};
		window.addEventListener("keydown", closeOnEscape);
		return () => window.removeEventListener("keydown", closeOnEscape);
	}, [onClose]);
	return (
		<aside className="code-inset" aria-label="Contextual code inspector" data-code-inspector={inset.uri}>
			<hr
				className="code-inset-resize"
				aria-label="Resize source inspector"
				aria-orientation="vertical"
				aria-valuemin={300}
				aria-valuemax={Math.round(window.innerWidth * 0.58)}
				aria-valuenow={Math.round(width)}
				tabIndex={0}
				onPointerDown={(event) => {
					drag.current = { x: event.clientX, width };
					event.currentTarget.setPointerCapture(event.pointerId);
				}}
				onPointerMove={(event) => {
					if (drag.current) resize(drag.current.width + drag.current.x - event.clientX);
				}}
				onPointerUp={(event) => {
					drag.current = null;
					event.currentTarget.releasePointerCapture(event.pointerId);
				}}
				onPointerCancel={() => {
					drag.current = null;
				}}
				onKeyDown={(event) => {
					if (event.key === "ArrowLeft" || event.key === "ArrowRight") event.preventDefault();
					if (event.key === "ArrowLeft") resize(width + 20);
					if (event.key === "ArrowRight") resize(width - 20);
				}}
			/>
			<div className="code-inset-bar">
				<div className="code-inset-heading">
					<span className="code-inset-kicker">Selected symbol · code</span>
					{symbol ? (
						<>
							<div className="code-inset-title">
								<span className={glyphClass(symbol.kind)}>{symbolGlyph(symbol.kind)}</span>
								<strong className="code-inset-name">{parseCallableName(symbol.name).base}</strong>
								<span className="code-inset-kind">{symbol.kind}</span>
							</div>
							<div className="code-inset-location" title={symbol.file}>
								<span className="code-inset-file">{symbol.file}</span>
								{symbol.line_range && <span>lines {symbol.line_range[0]}–{symbol.line_range[1]}</span>}
							</div>
						</>
					) : (
						<strong className="code-inset-name">Loading definition…</strong>
					)}
				</div>
				<span className="code-inset-actions">
					{symbol && (
						<>
							<button type="button" title="Make this symbol the cockpit focus" onClick={() => postFocus(inset.uri)}>
								Refocus graph
							</button>
							<button type="button" title="Open in the editor" onClick={() => postOpenSource(symbol)}>
								Open editor ↗
							</button>
						</>
					)}
					<button type="button" className="code-inset-close" aria-label="Close code inspector" title="Close code inspector" onClick={onClose}>
						✕
					</button>
				</span>
			</div>
			{inset.loading ? (
				<div className="code-inset-empty">Loading the indexed definition…</div>
			) : inset.source ? (
				<div className="code-inset-body">
					<CodeBlock source={inset.source} active={symbol?.line_range} />
				</div>
			) : (
				<div className="code-inset-empty">No source zone for this symbol.</div>
			)}
		</aside>
	);
}
