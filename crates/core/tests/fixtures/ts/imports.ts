// Wiring module — exercises the import surface: default, namespace,
// scoped, side-effect, aliases, type-only, external `new`.

import "./polyfills";
import defaultLogger from "./logger";
import * as fs from "node:fs";
import * as Util from "./util";
import { format as fmt, parse } from "@scope/datelib/iso";
import { z, type Schema } from "zod";
import { Buffer } from "node:buffer";
import { helper } from "./helper";
import { Widget } from "./widget";

export { helper };
export { Widget as RenamedWidget };

const schema: Schema = z.string();
const widget = new Widget();

export function boot() {
	const log = defaultLogger;
	const cfg = fmt(new Date());
	const parsed = parse(cfg);
	const stat = fs.statSync("/tmp");
	const buf = new Buffer(0);
	const ok = helper();
	Util.init();
	return { log, cfg, parsed, stat, schema, buf, widget, ok };
}
