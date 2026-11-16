import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import worker from "./worker.mjs";

const origin = "https://drillforge.test";
const id = "0d345dac-2dc7-48cc-b325-1e572d87b78b";
const raw = JSON.stringify({ title: "テスト", schema_version: 2, sets: [], performers: [] });

function fixture() {
  const database = new DatabaseSync(":memory:");
  database.exec(readFileSync(new URL("./drizzle/0000_drill_projects.sql", import.meta.url), "utf8"));
  const db = {
    prepare(sql) {
      const statement = database.prepare(sql);
      return {
        bind(...args) {
          return {
            all: () => ({ results: statement.all(...args) }),
            first: () => statement.get(...args),
            run: () => ({ meta: statement.run(...args) }),
          };
        },
      };
    },
  };
  return (owner, method = "GET", project = id, revision, requestOrigin = origin) => {
    const headers = new Headers({ Origin: requestOrigin });
    if (owner) headers.set("oai-authenticated-user-id", owner);
    if (revision !== undefined) headers.set("If-Match", '"' + revision + '"');
    const path = "/api/projects" + (project ? "/" + project : "");
    return worker.fetch(new Request(origin + path, {
      method, headers, body: method === "PUT" ? raw : undefined,
    }), { DB: db });
  };
}

test("an account can open a saved project on another device", async () => {
  const request = fixture();
  assert.equal((await request(null)).status, 401);
  assert.equal((await request("a", "PUT", id, 0)).status, 200);
  assert.deepEqual(await (await request("a")).json(), { document: raw, revision: 1 });
  assert.equal((await request("a", "GET", "")).headers.get("Cache-Control"), "no-store");
  assert.equal((await (await request("a", "GET", "")).json()).projects.length, 1);
  assert.equal((await request("b")).status, 404);
  assert.equal((await request("b", "PUT", id, 1)).status, 409);
  assert.deepEqual((await (await request("b", "GET", "")).json()).projects, []);
});

test("concurrent saves require the current revision", async () => {
  const request = fixture();
  assert.equal((await request("a", "PUT", id)).status, 428);
  assert.equal((await request("a", "PUT", id, 0, "https://elsewhere.test")).status, 403);
  assert.equal((await request("a", "PUT", id, 0)).status, 200);
  const attempts = await Promise.all([request("a", "PUT", id, 1), request("a", "PUT", id, 1)]);
  assert.deepEqual(attempts.map((attempt) => attempt.status).sort(), [200, 409]);
  assert.equal((await (await request("a")).json()).revision, 2);
});
