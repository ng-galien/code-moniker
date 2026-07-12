import { bag } from "./helpers/bag";
import * as util from "./helpers/util";

export function kinds(): Record<string, string> {
	return util.arrayToEnum(["a", "b"]);
}

export function first(): string {
	return bag.pickFirst(["x", "y"]);
}
