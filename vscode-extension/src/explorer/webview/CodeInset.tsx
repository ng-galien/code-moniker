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

// The code inset: one definition's zone floating over the canvas — the
// graph stays in place, the code comes to it.
export function CodeInset({ inset, onClose }: { inset: InsetState; onClose: () => void }) {
	const symbol = inset.symbol;
	return (
		<aside className="code-inset" aria-label="Code inset">
			<div className="code-inset-bar">
				{symbol ? (
					<>
						<span className={glyphClass(symbol.kind)}>{symbolGlyph(symbol.kind)}</span>
						<span className="code-inset-name">{parseCallableName(symbol.name).base}</span>
						<span className="code-inset-file">{symbol.file.split("/").pop()}</span>
					</>
				) : (
					<span className="code-inset-name">code</span>
				)}
				<span className="code-inset-actions">
					{symbol && (
						<>
							<button type="button" title="Dive into this symbol" onClick={() => postFocus(inset.uri)}>
								⤵
							</button>
							<button type="button" title="Open in the editor" onClick={() => postOpenSource(symbol)}>
								↗
							</button>
						</>
					)}
					<button type="button" title="Close" onClick={onClose}>
						✕
					</button>
				</span>
			</div>
			{inset.loading ? (
				<div className="code-inset-empty">Fetching the zone…</div>
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
