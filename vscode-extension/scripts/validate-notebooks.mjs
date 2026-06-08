// Structural check for shipped .cmnb notebooks: valid JSON, known cell kinds,
// and every rule cell has a preceding sample cell in the same language.
// Run: npm run validate
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, "..", "notebooks");
const LANGS = new Set(["rust", "typescript", "python", "go", "java", "csharp", "sql"]);

let failures = 0;
const fail = (file, msg) => {
	failures++;
	console.error(`✗ ${file}: ${msg}`);
};

for (const name of readdirSync(dir).filter((f) => f.endsWith(".cmnb"))) {
	let doc;
	try {
		doc = JSON.parse(readFileSync(join(dir, name), "utf8"));
	} catch (err) {
		fail(name, `invalid JSON: ${err.message}`);
		continue;
	}
	if (typeof doc.version !== "number" || !Array.isArray(doc.cells)) {
		fail(name, "missing version or cells[]");
		continue;
	}
	const samplesByLang = new Set();
	doc.cells.forEach((cell, i) => {
		if (cell.kind === "markdown") {
			if (typeof cell.value !== "string") fail(name, `cell ${i}: markdown needs value`);
		} else if (cell.kind === "sample") {
			if (!LANGS.has(cell.language)) fail(name, `cell ${i}: unknown sample language ${cell.language}`);
			if (typeof cell.value !== "string") fail(name, `cell ${i}: sample needs value`);
			samplesByLang.add(cell.language);
		} else if (cell.kind === "rule") {
			if (!LANGS.has(cell.language)) fail(name, `cell ${i}: unknown rule language ${cell.language}`);
			if (typeof cell.value !== "string" || !cell.value.trim()) fail(name, `cell ${i}: rule needs a TOML value`);
			if (!/\[\[.*\.where\]\]/.test(cell.value)) fail(name, `cell ${i}: rule TOML has no [[...where]] block`);
			if (!samplesByLang.has(cell.language)) {
				fail(name, `cell ${i}: rule (${cell.language}) has no preceding sample of that language`);
			}
		} else {
			fail(name, `cell ${i}: unknown kind ${cell.kind}`);
		}
	});
	if (failures === 0) console.log(`✓ ${name}: ${doc.cells.length} cells`);
}

if (failures > 0) {
	console.error(`\n${failures} problem(s) found.`);
	process.exit(1);
}
console.log("All notebooks valid.");
