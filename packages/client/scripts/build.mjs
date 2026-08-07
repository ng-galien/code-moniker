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
		define: { __CODE_MONIKER_MODULE_URL__: "import.meta.url" },
		outfile: "dist/node.js",
	}),
	build({
		...shared,
		entryPoints: ["src/node.ts"],
		platform: "node",
		packages: "external",
		format: "cjs",
		define: { __CODE_MONIKER_MODULE_URL__: "__filename" },
		outfile: "dist/node.cjs",
	}),
]);
