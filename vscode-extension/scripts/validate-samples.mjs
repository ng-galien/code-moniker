import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..");
const catalogDir = join(repoRoot, "samples", "catalog");
const packsSource = join(here, "..", "src", "catalog", "packs.ts");

const catalog = readdirSync(catalogDir)
	.filter((name) => name.endsWith(".cm.md"))
	.sort();
const source = readFileSync(packsSource, "utf8");
const imported = [...source.matchAll(/samples\/catalog\/([^"]+\.cm\.md)"/g)]
	.map((match) => match[1])
	.sort();

if (JSON.stringify(imported) !== JSON.stringify(catalog)) {
	console.error("Catalog sample imports do not match samples/catalog/*.cm.md");
	console.error(`on disk:  ${catalog.join(", ")}`);
	console.error(`imported: ${imported.join(", ")}`);
	process.exit(1);
}

for (const name of catalog) {
	const document = readFileSync(join(catalogDir, name), "utf8");
	for (const token of ["cm:rules", "cm:file=", "cm:expect"]) {
		if (!document.includes(token)) {
			console.error(`${basename(name)} is missing ${token}`);
			process.exit(1);
		}
	}
}

console.log(`All ${catalog.length} catalog samples are imported by the extension.`);
