import { build } from "esbuild";

const shared = {
	entryPoints: ["src/index.ts"],
	bundle: true,
	platform: "neutral",
	target: "es2022",
	sourcemap: true,
};

await Promise.all([
	build({
		...shared,
		format: "esm",
		outfile: "dist/index.js",
	}),
	build({
		...shared,
		format: "cjs",
		outfile: "dist/index.cjs",
	}),
]);
