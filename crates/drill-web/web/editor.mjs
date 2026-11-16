import init, { WebHandle } from "./pkg/drill_web.js";

const byId = (id) => document.getElementById(id);
const prefix = "drillforge.project.";
const activeKey = "drillforge.active";
const revisionPrefix = "drillforge.cloud.revision.";
const library = byId("library");
const status = byId("status");
let currentId;
let saved;
let draft;

function message(text) {
  status.textContent = text;
  byId("library-status").textContent = text;
}

function save() {
  if (!currentId) return;
  draft = app.snapshot(false) ?? draft;
  if (draft === undefined || draft === saved) return;
  const key = prefix + currentId;
  if (localStorage.getItem(key) !== (saved ?? null)) {
    throw new Error("別のタブで作品が更新されました。現在の編集を書き出してから開き直してください。");
  }
  localStorage.setItem(key, draft);
  saved = draft;
  message("このブラウザに保存済み");
}

function trySave() {
  try {
    save();
  } catch (error) {
    message(error instanceof Error && error.name === "Error"
      ? error.message
      : "保存できません。作品を書き出してください。");
  }
}

function open(id, json, isNew = false) {
  save();
  activate(id, json, isNew);
}

function activate(id, json, isNew) {
  app.open_project(json);
  currentId = id;
  saved = isNew ? undefined : json;
  draft = undefined;
  localStorage.setItem(activeKey, id);
  save();
  message("このブラウザに保存済み");
}

function renderList() {
  const list = byId("projects");
  list.replaceChildren();
  for (const key of Object.keys(localStorage).filter((key) => key.startsWith(prefix)).sort()) {
    const id = key.slice(prefix.length);
    const json = localStorage.getItem(key);
    if (!json) continue;
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    try {
      button.textContent = JSON.parse(json).title || "無題";
    } catch {
      button.textContent = "読み込みエラーのある作品";
    }
    button.addEventListener("click", () => {
      try {
        open(id, localStorage.getItem(key) ?? "");
        library.close();
      } catch (error) {
        message(String(error));
      }
    });
    item.append(button);
    list.append(item);
  }
}

function create(json) {
  open(crypto.randomUUID(), json, true);
  library.close();
}

let app;
try {
  await init();
  app = new WebHandle();
  await app.start(byId("drillforge"));
} catch (error) {
  message("エディターを起動できませんでした。WebGLを利用できるブラウザで再読み込みしてください。");
  throw error;
}

const template = JSON.parse(app.snapshot(true));
const fieldTools = byId("field-tools");
fieldTools.hidden = false;
for (const button of fieldTools.querySelectorAll("button")) {
  button.addEventListener("click", () => app.adjust_field_view(Number(button.dataset.view)));
}
const toggle = byId("library-toggle");
toggle.disabled = false;
toggle.addEventListener("click", () => {
  library.showModal();
  try {
    renderList();
  } catch {
    message("ブラウザの保存領域にアクセスできません。");
  }
});

byId("create").addEventListener("submit", (event) => {
  event.preventDefault();
  const title = byId("title").value.trim();
  if (!title) return;
  try {
    create(WebHandle.blank_project(title, byId("performers").valueAsNumber));
    event.target.reset();
  } catch (error) {
    message(String(error));
  }
});

byId("sample").addEventListener("click", () => {
  try {
    create(JSON.stringify({ ...template, title: "見本ドリル" }));
  } catch (error) {
    message(String(error));
  }
});

byId("import").addEventListener("change", async (event) => {
  const [file] = event.target.files;
  event.target.value = "";
  if (!file) return;
  try {
    if (file.size > 64 * 1024 * 1024) throw new Error("読み込めるファイルは64 MBまでです。");
    create(await file.text());
  } catch (error) {
    message(String(error));
  }
});

byId("export").addEventListener("click", () => {
  try {
    draft = app.snapshot(true) ?? draft;
    if (draft === undefined) throw new Error("作品を取得できませんでした。");
    const blob = new Blob([draft], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "drillforge-project.json";
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (error) {
    message(String(error));
  }
});

const setDialog = byId("set-dialog");
const addSet = byId("add-set");
addSet.disabled = false;
addSet.addEventListener("click", () => setDialog.showModal());
byId("set-form").addEventListener("submit", (event) => {
  if (event.submitter?.formMethod === "dialog") return;
  event.preventDefault();
  try {
    app.add_set(byId("counts").valueAsNumber);
    setDialog.close();
    trySave();
  } catch (error) {
    setDialog.close();
    message(String(error));
  }
});

try {
  const previous = location.search ? null : localStorage.getItem(activeKey);
  const json = previous && localStorage.getItem(prefix + previous);
  if (json) {
    open(previous, json);
  } else {
    currentId = crypto.randomUUID();
    // Preserve a room joined by the original query-string entry point.
    draft = location.search ? JSON.stringify(template) : WebHandle.blank_project("新しいドリル", 8);
    if (!location.search) app.open_project(draft);
    localStorage.setItem(activeKey, currentId);
    save();
  }
} catch {
  message("作品を復元または保存できませんでした。作品一覧から開き直すか、作品を書き出してください。");
}

setInterval(trySave, 500);
window.addEventListener("pagehide", trySave);
document.addEventListener("visibilitychange", () => {
  if (document.hidden) trySave();
});

const cloud = byId("cloud");
const cloudSave = byId("cloud-save");
const cloudRefresh = byId("cloud-refresh");
const cloudList = byId("cloud-projects");

async function cloudRequest(id = "", options) {
  const result = await fetch("./api/projects" + (id ? "/" + id : ""), options);
  if (!result.headers.get("Content-Type")?.includes("application/json")) {
    throw new Error("クラウドに接続できません。ログインと接続を確認してください。");
  }
  const payload = await result.json();
  if (!result.ok) throw new Error(payload.error ?? "クラウドでの操作に失敗しました。");
  return payload;
}

async function refreshCloud(quiet = false) {
  try {
    const { projects } = await cloudRequest();
    cloud.hidden = false;
    cloudList.replaceChildren();
    for (const { id, title } of projects) {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.textContent = title;
      button.addEventListener("click", async () => {
        button.disabled = true;
        try {
          const remote = await cloudRequest(id);
          save();
          const local = localStorage.getItem(prefix + id);
          if (local && local !== remote.document) {
            // Keep the local copy available for export or a separate save.
            localStorage.setItem(prefix + crypto.randomUUID(), local);
          }
          localStorage.setItem(prefix + id, remote.document);
          localStorage.setItem(revisionPrefix + id, String(remote.revision));
          // The current version has already been saved before replacing its record.
          activate(id, remote.document, false);
          library.close();
          message("クラウドの作品を開きました");
        } catch (error) {
          message(String(error));
        } finally {
          button.disabled = false;
        }
      });
      item.append(button);
      cloudList.append(item);
    }
  } catch (error) {
    if (!quiet) message(String(error));
  }
}

cloudRefresh.addEventListener("click", () => refreshCloud());
cloudSave.addEventListener("click", async () => {
  cloudSave.disabled = true;
  try {
    const id = currentId;
    if (!id) throw new Error("保存する作品を開いてください。");
    const document = app.snapshot(true);
    if (!document) throw new Error("作品を取得できませんでした。");
    draft = document;
    save();
    const revision = localStorage.getItem(revisionPrefix + id) ?? "0";
    const result = await cloudRequest(id, {
      method: "PUT",
      headers: { "Content-Type": "application/json", "If-Match": '"' + revision + '"' },
      body: document,
    });
    localStorage.setItem(revisionPrefix + id, String(result.revision));
    message("クラウドに保存済み");
    await refreshCloud();
  } catch (error) {
    message(String(error));
  } finally {
    cloudSave.disabled = false;
  }
});
toggle.addEventListener("click", () => refreshCloud(true));

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("./offline.js").catch(console.warn);
}
