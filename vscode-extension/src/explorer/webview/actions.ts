import type { SymbolDto } from "../../daemon/model";
import { vscode } from "./vscodeApi";

export function postFocus(prefix: string): void {
	vscode.postMessage({ type: "focus", prefix });
}

export function postOpenSource(symbol: SymbolDto): void {
	vscode.postMessage({
		type: "openSource",
		target: {
			root: symbol.root,
			file: symbol.file,
			line: symbol.line_range ? symbol.line_range[0] : 1,
		},
	});
}
