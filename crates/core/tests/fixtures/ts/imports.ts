// Wiring module — exercises the import surface: default, namespace,
// scoped, side-effect, aliases, type-only, external `new`.

// cm: ref side effect local import
import "./polyfills";
import defaultLogger from "./logger";
import * as fs from "node:fs";
import * as Util from "./util";
// cm: ref scoped package aliased import
import { format as fmt, parse } from "@scope/datelib/iso";
// cm: ref zod type import
import { z, type Schema } from "zod";
import { Buffer } from "node:buffer";
import { helper } from "./helper";
import { Widget } from "./widget";

export { helper };
// cm: ref renamed widget reexport
export { Widget as RenamedWidget };

// cm: def schema const
const schema: Schema = z.string();
// cm: def widget const
const widget = new Widget();

// cm: def boot function
export function boot() {
	const log = defaultLogger;
	// cm: ref boot calls aliased format
	const cfg = fmt(new Date());
	const parsed = parse(cfg);
	// cm: ref boot calls fs stat
	const stat = fs.statSync("/tmp");
	// cm: ref boot instantiates buffer
	const buf = new Buffer(0);
	// cm: ref boot calls helper
	const ok = helper();
	// cm: ref boot calls util init
	Util.init();
	return { log, cfg, parsed, stat, schema, buf, widget, ok };
}
