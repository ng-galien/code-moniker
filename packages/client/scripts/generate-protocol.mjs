import { readFileSync, writeFileSync } from "node:fs";

const schemaPath = new URL("../../../docs/schema/daemon.schema.json", import.meta.url);
const outputPath = new URL("../src/protocol.ts", import.meta.url);
const schema = JSON.parse(readFileSync(schemaPath, "utf8"));
const version = schema["x-code-moniker-protocol-version"];

if (!Number.isSafeInteger(version) || version < 1) {
	throw new Error(
		"daemon schema is missing a positive x-code-moniker-protocol-version",
	);
}

writeFileSync(
	outputPath,
	`// Generated from docs/schema/daemon.schema.json. Do not edit by hand.\n` +
		`export const PROTOCOL_VERSION = ${version} as const;\n`,
);
