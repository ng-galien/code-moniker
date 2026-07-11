import type { CSSProperties } from "react";

import "./code.css";

import type {
	HighlightedSourceLine,
	HighlightedSourceSnippet,
	HighlightedSourceToken,
} from "../symbols/detail/highlight";

// Shared code renderer for every webview: pre-highlighted lines from the
// extension host, theme-resolved through --tok-dark/--tok-light custom
// properties. Optional active range highlighting and compact density.
export function CodeBlock({
	source,
	active,
	compact,
}: {
	source: HighlightedSourceSnippet;
	active?: [number, number] | null;
	compact?: boolean;
}) {
	return (
		<div className={compact ? "code code-compact" : "code"}>
			{source.lines.map((line) => (
				<div
					key={line.number}
					className={
						active && line.number >= active[0] && line.number <= active[1]
							? "code-line active"
							: "code-line"
					}
				>
					<span className="gutter">{line.number}</span>
					<code className="src">
						{lineTokens(line).map((token, index) => (
							<span key={index} className="tok" style={tokenStyle(token)}>
								{token.text}
							</span>
						))}
					</code>
				</div>
			))}
		</div>
	);
}

function lineTokens(line: HighlightedSourceLine): HighlightedSourceToken[] {
	return line.tokens && line.tokens.length > 0 ? line.tokens : [{ text: line.text || " " }];
}

function tokenStyle(token: HighlightedSourceToken): CSSProperties {
	const style: Record<string, string> = {};
	if (isHexColor(token.lightColor)) {
		style["--tok-light"] = token.lightColor;
	}
	if (isHexColor(token.darkColor)) {
		style["--tok-dark"] = token.darkColor;
	}
	if (token.fontStyle !== undefined) {
		if ((token.fontStyle & 1) !== 0) {
			style.fontStyle = "italic";
		}
		if ((token.fontStyle & 2) !== 0) {
			style.fontWeight = "600";
		}
		if ((token.fontStyle & 4) !== 0) {
			style.textDecoration = "underline";
		}
	}
	return style as CSSProperties;
}

function isHexColor(value: unknown): value is string {
	return typeof value === "string" && /^#[0-9a-fA-F]{3,8}$/.test(value);
}
