import { parseArgs } from "node:util";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { performance } from "node:perf_hooks";
import { analyze, layoutCity } from "./index.js";
import { captureProject, type Capture } from "./node.js";

const { values } = parseArgs({
  options: {
    root: { type: "string" },
    path: { type: "string", multiple: true },
    input: { type: "string" },
    output: { type: "string" },
    binary: { type: "string" },
    registry: { type: "string" },
    "max-symbols": { type: "string" },
    threshold: { type: "string" },
    resolution: { type: "string" },
  },
});
const start = performance.now();
const capture: Capture = values.input
  ? JSON.parse(await readFile(values.input, "utf8"))
  : await captureProject(values.root ?? process.cwd(), {
      path: values.path,
      binary: values.binary,
      registryDirectory: values.registry,
      maxSymbols: values["max-symbols"]
        ? Number(values["max-symbols"])
        : undefined,
      onProgress: (done, total) => {
        if (done % 100 === 0 || done === total)
          console.error(`Captured ${done}/${total} symbols`);
      },
    });
const captureMs = performance.now() - start,
  analyzeStart = performance.now();
const analysis = analyze(capture.graph, {
  jaccardThreshold: values.threshold ? Number(values.threshold) : undefined,
  resolution: values.resolution ? Number(values.resolution) : undefined,
});
const analysisMs = performance.now() - analyzeStart;
const layouts = Object.fromEntries(
  analysis.partitions.map((p) => [
    p.algorithm,
    layoutCity(analysis, p.algorithm),
  ]),
);
const output = {
  graph: capture.graph,
  provenance: capture.provenance,
  analysis,
  layouts,
  timings: {
    captureMs,
    analysisMs,
    layoutMs: performance.now() - analyzeStart - analysisMs,
  },
};
if (values.output) {
  await mkdir(dirname(values.output), { recursive: true });
  await writeFile(values.output, JSON.stringify(output, null, 2) + "\n");
}
console.log(
  JSON.stringify(
    {
      provenance: capture.provenance,
      timings: output.timings,
      partitions: analysis.partitions.map((p) => ({
        algorithm: p.algorithm,
        communities: p.communities.length,
        ...p.metrics,
      })),
      cycles: analysis.stronglyConnectedComponents.filter((c) => c.length > 1)
        .length,
    },
    null,
    2,
  ),
);
