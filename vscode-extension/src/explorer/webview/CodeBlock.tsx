import type { CSSProperties } from "react";

import type { HighlightedSourceSnippet } from "../../symbols/detail/highlight";

// Pre-highlighted source lines from the extension host: each token carries a
// dark and a light colour, resolved by CSS custom properties so the block
// follows the active editor theme without re-highlighting.
export function CodeBlock({ source }: { source: HighlightedSourceSnippet }) {
	return (
		<pre className="code">
			{source.lines.map((line) => (
				<div key={line.number} className="code-line">
					<span className="line-number">{line.number}</span>
					<span className="line-content">
						{line.tokens.map((token, index) => (
							<span
								key={index}
								className="tok"
								style={
									{
										"--dark": token.darkColor,
										"--light": token.lightColor,
									} as CSSProperties
								}
							>
								{token.text}
							</span>
						))}
					</span>
				</div>
			))}
		</pre>
	);
}
