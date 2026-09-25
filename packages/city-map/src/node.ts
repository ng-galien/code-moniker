import { setTimeout as delay } from "node:timers/promises";
import { resolve } from "node:path";
import type {
  CodeMonikerClient,
  QueryCursor,
  SymbolDto,
} from "@code-moniker/client";
import { NodeDaemonRuntime } from "@code-moniker/client/node";
import type { CityGraph } from "./model.js";

export interface CaptureOptions {
  path?: string[];
  maxSymbols?: number;
  maxReferences?: number;
  onProgress?: (done: number, total: number) => void;
}
export interface Capture {
  graph: CityGraph;
  provenance: {
    root: string;
    generation: unknown;
    path: string[];
    capturedAt: string;
    symbols: number;
    resolvedReferences: number;
    outsideScope: number;
    indirectReferencesExcluded: number;
    unmappedSources: number;
    semantics: string;
  };
}

/** Uses only the existing public Node client; callers can supply their connected client. */
export async function captureIndex(
  client: CodeMonikerClient,
  options: CaptureOptions = {},
): Promise<Capture> {
  const maxSymbols = options.maxSymbols ?? 20_000,
    maxReferences = options.maxReferences ?? 1_000_000;
  if (
    !Number.isSafeInteger(maxSymbols) ||
    maxSymbols < 1 ||
    !Number.isSafeInteger(maxReferences) ||
    maxReferences < 1
  )
    throw new Error("Capture budgets must be positive integers");
  const status = await client.workspace.status({ consistency: "stale_ok" });
  if (status.phase !== "ready" && status.phase !== "refreshing")
    throw new Error(`Index is ${status.phase}`);
  const generation = status.generation;
  if (generation == null) throw new Error("Index has no published generation");
  const check = (g: unknown) => {
    if (JSON.stringify(g) !== JSON.stringify(generation))
      throw new Error("Index changed during capture; retry the capture");
  };
  const symbols: SymbolDto[] = [];
  let cursor: QueryCursor | null = null;
  do {
    const page = await client.symbols.search(
      { path: options.path, includeNonNavigable: true },
      { consistency: "stale_ok", limit: 500, cursor },
    );
    check(page.generation);
    if (page.data.total > maxSymbols)
      throw new Error(
        `Symbol budget exceeded (${page.data.total} > ${maxSymbols}); choose a smaller scope or increase maxSymbols`,
      );
    symbols.push(...page.data.rows);
    cursor = page.nextCursor;
    if (symbols.length > maxSymbols) throw new Error("Symbol budget exceeded");
  } while (cursor);
  // Keep IDs for RPC, URIs for stable scene identities. Duplicate identities get a source discriminator.
  const byIdentity = new Map<string, SymbolDto[]>();
  for (const s of symbols) {
    const key = JSON.stringify([s.root, s.file, s.uri]);
    const list = byIdentity.get(key) ?? [];
    list.push(s);
    byIdentity.set(key, list);
  }
  const stable = (s: SymbolDto) =>
    JSON.stringify([
      s.root,
      s.file,
      s.uri,
      ...(byIdentity.get(JSON.stringify([s.root, s.file, s.uri]))!.length > 1
        ? [s.line_range, s.id]
        : []),
    ]);
  const graph: CityGraph = {
    nodes: symbols.map((s) => ({
      id: stable(s),
      uri: s.uri,
      file: s.file,
      label: s.name,
      kind: s.kind,
    })),
    edges: [],
  };
  let outsideScope = 0,
    unmappedSources = 0,
    indirectReferencesExcluded = 0,
    rows = 0,
    resolvedReferences = 0;
  const seen = new Set<string>();
  for (let i = 0; i < symbols.length; i++) {
    const target = symbols[i]!;
    cursor = null;
    do {
      const page = await client.symbols.usages(
        target.id,
        { direction: "incoming", includeDescendants: false },
        { consistency: "stale_ok", limit: 500, cursor },
      );
      check(page.generation);
      for (const row of page.data.rows) {
        if (++rows > maxReferences)
          throw new Error("Reference budget exceeded");
        if (row.via) {
          indirectReferencesExcluded++;
          continue;
        }
        const key = `${row.reference}\0${target.id}`;
        if (seen.has(key)) continue;
        seen.add(key);
        const sources = byIdentity.get(
          JSON.stringify([row.root, row.file, row.context]),
        );
        if (!sources) {
          outsideScope++;
          continue;
        }
        if (sources.length !== 1) {
          unmappedSources++;
          continue;
        }
        graph.edges.push({
          source: stable(sources[0]!),
          target: stable(target),
          kind: row.kind,
          weight: 1,
        });
        resolvedReferences++;
      }
      cursor = page.nextCursor;
    } while (cursor);
    options.onProgress?.(i + 1, symbols.length);
  }
  check(
    (await client.workspace.status({ consistency: "stale_ok" })).generation,
  );
  return {
    graph,
    provenance: {
      root: status.root,
      generation,
      path: options.path ?? [],
      capturedAt: new Date().toISOString(),
      symbols: symbols.length,
      resolvedReferences,
      outsideScope,
      indirectReferencesExcluded,
      unmappedSources,
      semantics:
        "Direct resolved incoming references among selected indexed symbols. Unresolved/external outgoing references are not represented. Isolation means no observed selected resolved edge, not dead code.",
    },
  };
}

export async function captureProject(
  root: string,
  options: CaptureOptions & {
    binary?: string;
    registryDirectory?: string;
  } = {},
): Promise<Capture> {
  const roots: [string] = [resolve(root)];
  const runtime = new NodeDaemonRuntime({
    registryDirectory: options.registryDirectory,
    timeoutMs: 120_000,
  });
  const existing = runtime.findDaemon(roots);
  const owned = existing
    ? undefined
    : await runtime.launch({
        workspaceRoots: roots,
        binaryCandidates: options.binary
          ? [resolve(options.binary)]
          : undefined,
        supervisorPid: process.pid,
        registrationTimeoutMs: 15_000,
      });
  let client: CodeMonikerClient | undefined;
  try {
    client = await runtime.connect(existing ?? owned!.entry, {
      clientName: "city-map",
      expectedWorkspaceRoots: roots,
    });
    const deadline = Date.now() + 120_000;
    while (true) {
      const status = await client.workspace.status({ consistency: "stale_ok" });
      if (status.phase === "ready" || status.phase === "refreshing") break;
      if (status.failure || Date.now() > deadline)
        throw new Error(`Index unavailable: ${status.phase}`);
      await delay(250);
    }
    return await captureIndex(client, options);
  } finally {
    client?.close();
    if (owned) await runtime.stopOwned(owned);
  }
}
