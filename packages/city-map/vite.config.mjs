import { defineConfig } from "vite";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

export default defineConfig({
  server: { watch: { usePolling: true, interval: 500 } },
  build: { outDir: "preview-dist" },
  plugins: [
    {
      name: "city-map-local-capture",
      configureServer(server) {
        const path = resolve(
          process.env.CITY_MAP_DATA ??
            "../../work/research/city-map-client.json",
        );
        server.watcher.add(path);
        server.watcher.on("change", (changed) => {
          if (resolve(changed) === path)
            server.ws.send({ type: "full-reload" });
        });
        server.middlewares.use("/data.json", async (_req, res) => {
          try {
            res.setHeader("Content-Type", "application/json");
            res.end(await readFile(path));
          } catch {
            res.statusCode = 404;
            res.end(
              JSON.stringify({
                error:
                  "Capture absente. Exécuter la commande analyze avec --output avant de lancer la prévisualisation.",
              }),
            );
          }
        });
      },
    },
  ],
});
