import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, join } from "node:path";

const source = fileURLToPath(new URL("../crates/drill-web/web/", import.meta.url));
const output = resolve(process.argv[2] ?? "target/drill-web-site");
const hash = createHash("sha256");
await mkdir(join(output, "pkg"), { recursive: true });

for (const file of ["index.html", "editor.css", "editor.mjs"]) {
  hash.update(await readFile(join(source, file)));
  await copyFile(join(source, file), join(output, file));
}
for (const file of ["drill_web.js", "drill_web_bg.wasm"]) {
  const path = join(output, "pkg", file);
  hash.update(await readFile(path));
}
for (const file of ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md",
  "THIRD_PARTY_LICENSES.md", "assets/OFL-NotoSansJP.txt"]) {
  await copyFile(new URL("../" + file, import.meta.url), join(output, file.split("/").at(-1)));
}
const worker = await readFile(join(source, "offline.js"), "utf8");
hash.update(worker);
await writeFile(join(output, "offline.js"), worker.replace("__BUILD_HASH__", hash.digest("hex")));
