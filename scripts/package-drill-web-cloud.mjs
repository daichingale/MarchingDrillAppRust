import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const site = resolve(process.argv[2] ?? ".");
const manifestPath = join(site, ".openai/hosting.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
if (!manifest.project_id) throw new Error("A provisioned Sites project is required");
delete manifest.static;
manifest.d1 = "DB";
await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + "\n");

const cloud = join(site, "crates/drill-web/cloud");
await cp(join(cloud, "drizzle"), join(site, "drizzle"), { recursive: true });
await mkdir(join(site, "dist/server"), { recursive: true });
await cp(join(root, "target/drill-web-site"), join(site, "dist/client"), { recursive: true });
await cp(join(cloud, "worker.mjs"), join(site, "dist/server/index.js"));
await writeFile(join(site, "dist/server/wrangler.json"), JSON.stringify({
  main: "index.js",
  compatibility_date: "2026-11-16",
  assets: { directory: "../client", binding: "ASSETS", run_worker_first: true },
}, null, 2) + "\n");
