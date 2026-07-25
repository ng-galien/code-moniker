import type { MouseEvent, ReactNode } from "react";

import { parseCallableName } from "./parse";
import { glyphClass, symbolGlyph } from "./symbolGlyph";

// One symbol row, shared by every webview that lists members: kind glyph then
// the dominant name. Panels style `.member-row` themselves and add trailing
// badges as children; the markup stays in one place so a graph card and the
// detail pane cannot drift into showing different things.
export function MemberRow({
	kind,
	name,
	title,
	onClick,
	children,
}: {
	kind: string;
	name: string;
	title: string;
	onClick: (event: MouseEvent<HTMLButtonElement>) => void;
	children?: ReactNode;
}) {
	return (
		<button type="button" className="member-row" title={title} onClick={onClick}>
			<span className={glyphClass(kind)}>{symbolGlyph(kind)}</span>
			<span className="member-name">{parseCallableName(name).base}</span>
			{children}
		</button>
	);
}
