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
const importedCatalog = [...source.matchAll(/samples\/catalog\/([^"]+\.cm\.md)"/g)]
	.map((match) => match[1])
	.sort();
const importedLearn = [...source.matchAll(/samples\/learn\/([^"]+\.cm\.md)"/g)]
	.map((match) => match[1])
	.sort();

if (JSON.stringify(importedCatalog) !== JSON.stringify(catalog)) {
	console.error("Catalog sample imports do not match samples/catalog/*.cm.md");
	console.error(`on disk:  ${catalog.join(", ")}`);
	console.error(`imported: ${importedCatalog.join(", ")}`);
	process.exit(1);
}

if (JSON.stringify(importedLearn) !== JSON.stringify(learn)) {
	console.error("Learn sample imports do not match samples/learn/*.cm.md");
	console.error(`on disk:  ${learn.join(", ")}`);
	console.error(`imported: ${importedLearn.join(", ")}`);
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
