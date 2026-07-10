import type { SymbolDto } from "../../daemon/model";
import { CodeBlock } from "../../webview-lib/CodeBlock";
import type { UnitPayload } from "../protocol";
import { postOpenSource } from "./actions";
import { CALLABLE_KINDS } from "./graph/model";
import { UnitGraph } from "./graph/UnitGraph";

// The unit's center: a callable focus reads as header + code; any container
// focus (file, type, module) reads as the structural call DAG of its members.
export function Center({ unit }: { unit: UnitPayload }) {
	if (unit.focus.kind === "symbol" && CALLABLE_KINDS.has(unit.focus.symbol.kind)) {
		return (
			<div className="column center">
				<UnitHeader symbol={unit.focus.symbol} />
				{unit.source && <CodeBlock source={unit.source} active={unit.focus.symbol.line_range} />}
			</div>
		);
	}
	return (
		<div className="column center">
			{unit.focus.kind === "symbol" ? (
				<UnitHeader symbol={unit.focus.symbol} />
			) : (
				<div className="heading">members ({unit.members.length})</div>
			)}
			<UnitGraph unit={unit} />
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
				{symbol.file}
			</div>
		</div>
	);
}
