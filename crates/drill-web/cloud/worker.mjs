const maxBytes = 768 * 1024;
const projectId = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i;

function response(value, status = 200) {
  return Response.json(value, { status, headers: { "Cache-Control": "no-store" } });
}

async function readDocument(request) {
  const reader = request.body?.getReader();
  if (!reader) return null;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let bytes = 0;
  let text = "";
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      if (bytes > maxBytes) {
        await reader.cancel();
        return null;
      }
      text += decoder.decode(chunk.value, { stream: true });
    }
    text += decoder.decode();
    const document = JSON.parse(text);
    const title = document?.title;
    if (typeof title !== "string" || !title.trim() || title.length > 120 ||
        !Number.isInteger(document.schema_version) ||
        !Array.isArray(document.sets) || !Array.isArray(document.performers)) return null;
    return { text, title };
  } catch {
    return null;
  }
}

async function projects(request, db, url) {
  // Sites dispatch owns this header. Keep the Worker reachable only through Sites.
  const owner = request.headers.get("oai-authenticated-user-id");
  if (!owner) return response({ error: "ログインしてください。" }, 401);

  if (url.pathname === "/api/projects" && request.method === "GET") {
    const { results } = await db.prepare(
      "SELECT id, title, revision FROM drill_projects WHERE owner = ? ORDER BY updated_at DESC, id LIMIT 100"
    ).bind(owner).all();
    return response({ projects: results });
  }

  const id = url.pathname.slice("/api/projects/".length);
  if (!url.pathname.startsWith("/api/projects/") || !projectId.test(id)) {
    return response({ error: "作品が見つかりません。" }, 404);
  }

  if (request.method === "GET") {
    const project = await db.prepare(
      "SELECT document, revision FROM drill_projects WHERE owner = ? AND id = ?"
    ).bind(owner, id).first();
    return project ? response(project) : response({ error: "作品が見つかりません。" }, 404);
  }

  if (request.method !== "PUT") return response({ error: "操作できません。" }, 405);
  if (request.headers.get("Origin") !== url.origin) {
    return response({ error: "このページから保存してください。" }, 403);
  }
  const tag = request.headers.get("If-Match");
  const revision = tag === null || !/^"\d+"$/.test(tag) ? NaN : Number(tag.slice(1, -1));
  if (!Number.isSafeInteger(revision)) return response({ error: "保存情報が不足しています。" }, 428);
  const document = await readDocument(request);
  if (!document) return response({ error: "クラウドには768 KiB以内の作品を保存できます。" }, 400);

  const result = revision === 0
    ? await db.prepare(
      "INSERT INTO drill_projects (owner, id, title, document) SELECT ?, ?, ?, ? WHERE (SELECT count(*) FROM drill_projects WHERE owner = ?) < 100 ON CONFLICT (owner, id) DO NOTHING"
    ).bind(owner, id, document.title, document.text, owner).run()
    : await db.prepare(
      "UPDATE drill_projects SET title = ?, document = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE owner = ? AND id = ? AND revision = ?"
    ).bind(document.title, document.text, owner, id, revision).run();

  if (!result.meta.changes) {
    return response({ error: "別の端末で更新されたか、保存上限に達しました。編集を書き出し、クラウドの作品を確認してください。" }, 409);
  }
  return response({ revision: revision + 1 });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (!url.pathname.startsWith("/api/")) return env.ASSETS.fetch(request);
    try {
      return await projects(request, env.DB, url);
    } catch (error) {
      console.error("DrillForge cloud database request failed", error?.name);
      return response({ error: "クラウドに接続できません。端末内の作品を確認してください。" }, 503);
    }
  },
};
