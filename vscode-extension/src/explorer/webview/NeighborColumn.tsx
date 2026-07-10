import type { SymbolGraphNeighbor } from "../../daemon/model";
import type { UnitPayload } from "../protocol";
import { postFocus, postOpenSource } from "./actions";

export function NeighborColumn({
	title,
	neighbors,
	unit,
	side,
}: {
	title: string;
	neighbors: SymbolGraphNeighbor[];
	unit: UnitPayload;
	side: "left" | "right";
}) {
	const focusUri = unit.focus.kind === "symbol" ? unit.focus.symbol.uri : null;
	return (
		<div className={`column ${side}`}>
			<div className="heading">
				{title} ({neighbors.length})
			</div>
			{neighbors.length === 0 ? (
				<div className="muted">{side === "left" ? "no callers" : "no external callees"}</div>
			) : (
				neighbors.map((neighbor) => (
					<NeighborRow key={neighbor.symbol.uri} neighbor={neighbor} focusUri={focusUri} />
				))
			)}
		</div>
	);
}

function NeighborRow({
	neighbor,
	focusUri,
}: {
	neighbor: SymbolGraphNeighbor;
	focusUri: string | null;
}) {
	const recursion = focusUri != null && neighbor.symbol.uri === focusUri;
	const count = neighbor.count > 1 ? ` ×${neighbor.count}` : "";
	return (
		<div
			className="neighbor"
			onClick={() => postFocus(neighbor.symbol.uri)}
			onContextMenu={(event) => {
				event.preventDefault();
				postOpenSource(neighbor.symbol);
			}}
		>
			<div className="name">
				{recursion ? "↺ " : ""}
				{neighbor.symbol.kind} {neighbor.symbol.name}
			</div>
			<div className="meta">
				{neighbor.symbol.file}
				{count} [{neighbor.kinds.join(",")}]
			</div>
		</div>
	);
}
