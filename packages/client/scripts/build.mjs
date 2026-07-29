import { build } from "esbuild";

const shared = {
	bundle: true,
	target: "es2022",
	sourcemap: true,
};

await Promise.all([
	build({
		...shared,
		entryPoints: ["src/index.ts"],
		platform: "neutral",
		format: "esm",
		outfile: "dist/index.js",
	}),
	build({
		...shared,
		entryPoints: ["src/index.ts"],
		platform: "neutral",
		format: "cjs",
		outfile: "dist/index.cjs",
	}),
	build({
		...shared,
		entryPoints: ["src/node.ts"],
		platform: "node",
		packages: "external",
		format: "esm",
		outfile: "dist/node.js",
	}),
	build({
		...shared,
		entryPoints: ["src/node.ts"],
		platform: "node",
		packages: "external",
		format: "cjs",
		outfile: "dist/node.cjs",
	}),
]);
