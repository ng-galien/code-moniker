import * as util from "./helpers/util";

export function kinds(): Record<string, string> {
	return util.arrayToEnum(["a", "b"]);
}
