import type { SymbolDto } from "../../daemon/model";
import type { UnitPayload } from "../protocol";
import { postFocus, postOpenSource } from "./actions";
import { CodeBlock } from "./CodeBlock";
import { internalCounts, MemberNode, nestMembers } from "./nest";

export function Center({ unit }: { unit: UnitPayload }) {
	if (unit.focus.kind === "symbol") {
		return (
			<div className="column center">
				<UnitHeader symbol={unit.focus.symbol} />
				{unit.source && <CodeBlock source={unit.source} />}
			</div>
		);
	}
	const counts = internalCounts(unit.internalEdges);
	return (
		<div className="column center">
			<div className="heading">members ({unit.members.length})</div>
			{nestMembers(unit.members, null).map((node) => (
				<SurfaceBox key={node.member.uri} node={node} counts={counts} />
			))}
		</div>
	);
}

// Containment surface: nested boxes mirror the unit's structure; each row
// zooms the focus onto that member.
function SurfaceBox({ node, counts }: { node: MemberNode; counts: Map<string, number> }) {
	const line = node.member.line_range ? `L${node.member.line_range[0]}` : "";
	const internal = counts.get(node.member.id) ?? 0;
	const meta = internal > 0 ? `${line} · ${internal} edge(s)` : line;
	return (
		<div className="surface">
			<div
				className="surface-row"
				onClick={(event) => {
					event.stopPropagation();
					postFocus(node.member.uri);
				}}
			>
				<span className="name">
					{node.member.kind} {node.member.name}
				</span>
				<span className="meta">{meta}</span>
			</div>
			{node.children.map((child) => (
				<SurfaceBox key={child.member.uri} node={child} counts={counts} />
			))}
		</div>
	);
}

function UnitHeader({ symbol }: { symbol: SymbolDto }) {
	return (
		<div className="unit-header">
			<div className="title">
				{symbol.kind} {symbol.name}
			</div>
			{symbol.signature && <div className="signature">{symbol.signature}</div>}
			<div className="meta link" onClick={() => postOpenSource(symbol)}>
				{symbol.line_range
					? `${symbol.file} · L${symbol.line_range[0]}-${symbol.line_range[1]}`
					: symbol.file}
			</div>
		</div>
	);
}
