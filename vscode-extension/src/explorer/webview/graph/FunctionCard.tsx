import { Handle, Position } from "@xyflow/react";

import type { GraphNodeModel } from "./model";
import { parseCallableName } from "./parse";

const KIND_GLYPHS: Record<string, string> = {
	fn: "ƒ",
	function: "ƒ",
	method: "m",
	constructor: "c",
	macro: "!",
	test: "⚗",
};

// The canonical function card: kind as glyph, dominant name, one typed badge
// per argument, degrees instead of line numbers. Line ranges stay in the data
// to fetch code zones; they are never printed.
export function FunctionCard({ data }: { data: { node: GraphNodeModel } }) {
	const node = data.node;
	const symbol = node.symbol;
	const parsed = parseCallableName(symbol.name);
	const glyph = KIND_GLYPHS[symbol.kind] ?? symbol.kind.charAt(0);
	const classes = ["fncard", node.entry ? "entry" : "", node.test ? "test" : ""]
		.filter(Boolean)
		.join(" ");
	return (
		<div className={classes}>
			<Handle type="target" position={Position.Top} className="port" />
			<div className="fncard-head">
				<span className={`glyph ${symbol.kind === "method" ? "method" : ""}`}>{glyph}</span>
				<span className="fnname">{parsed.base}</span>
				{node.recursive && <span className="recursion">↺</span>}
				{symbol.visibility === "public" && <span className="pub">pub</span>}
			</div>
			{parsed.args.length > 0 && (
				<div className="fncard-args">
					{parsed.args.map((arg, index) => (
						<span key={index} className="argchip">
							{arg.type ? (
								<>
									<span className="argname">{arg.name}</span>
									<span className="argtype">{arg.type}</span>
								</>
							) : (
								<span className="argname">{arg.name}</span>
							)}
						</span>
					))}
				</div>
			)}
			<div className="fncard-degrees">
				{node.callsIn > 0 && <span className="deg-in">⟵ {node.callsIn}</span>}
				{node.callsOut > 0 && <span className="deg-out">⟶ {node.callsOut}</span>}
			</div>
			<Handle type="source" position={Position.Bottom} className="port" />
		</div>
	);
}
