import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");
const catalogDir = join(repoRoot, "samples", "catalog");
const learnDir = join(repoRoot, "samples", "learn");
const packsSource = join(here, "..", "src", "catalog", "packs.ts");

const catalog = readdirSync(catalogDir)
	.filter((name) => name.endsWith(".cm.md"))
	.sort();
const learn = readdirSync(learnDir)
	.filter((name) => name.endsWith(".cm.md"))
	.sort();
const source = readFileSync(packsSource, "utf8");

if (!source.includes('from "code-moniker-sample-packs"')) {
	console.error("Catalog packs must import the generated sample pack module.");
	process.exit(1);
}

if (/samples\/(catalog|learn)\//.test(source)) {
	console.error("Catalog packs must not manually import individual sample files.");
	process.exit(1);
}

for (const [dir, names] of [[catalogDir, catalog], [learnDir, learn]]) {
	for (const name of names) {
		const document = readFileSync(join(dir, name), "utf8");
		validateExecutableScenario(name, document);
	}
}

function validateExecutableScenario(name, document) {
	for (const token of ["cm:rules", "cm:file=", "cm:expect"]) {
		if (!document.includes(token)) {
			console.error(`${basename(name)} is missing ${token}`);
			process.exit(1);
		}
	}
}

console.log(
	`All ${catalog.length} catalog samples and ${learn.length} learn samples are imported by the extension.`,
);
