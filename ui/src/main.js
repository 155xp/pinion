import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { api, listen, convertFileSrc } from "./api.js";
import { h, $, debounce, basename, dirname, modKey, stamp, isMac } from "./dom.js";
import { icon } from "./icons.js";
import { LinkIndex } from "./links.js";
import { createEditor } from "./editor/index.js";
import { Tree } from "./tree.js";
import { openPalette } from "./palette.js";
import { openSettings } from "./settings.js";
import { openMenu, confirmDialog, toast, closeOverlays } from "./overlays.js";
import { applyTheme, themePrefs, DEFAULTS, resolvedMode } from "./theme.js";
import { VoiceView } from "./voice-view.js";

const root = document.documentElement;
const report = (m) => invoke("ui_log", { message: String(m) }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${e.reason?.stack || e.reason}`));
const appEl = $("#app");
const railEl = $("#rail");
const scrollEl = $("#scroll");
const columnEl = $("#column");
const titleEl = $("#title");
const titleErr = $("#title-error");
const backlinksEl = $("#backlinks");
const emptyEl = $("#empty");
const voiceEl = $("#voice-view");

const index = new LinkIndex();
let boot = null;
let ui = {};
let entries = [];
let current = null; // { path, mtime, dirty, crlf }
let recent = [];
let back = [];
let forward = [];
let view = "note";
let applyingOutside = false;
let tree = null;
let voice = null;
let recordingPath = null;

const saveUi = (patch) => {
  Object.assign(ui, patch);
  return api.saveUi(patch).catch(() => {});
};

// ---------- theme ----------
function themeApplied(t) {
  const railOpen = !appEl.classList.contains("rail-closed");
  api.appearance(railOpen ? t.meta.ground : t.meta.page, t.resolved === "dark").catch(() => {});
}
function setTheme(patch) {
  applyTheme(patch);
  saveUi({ theme: themePrefs() });
}

// ---------- vault ----------
async function refreshEntries() {
  try {
    entries = await api.list();
  } catch {
    return;
  }
  index.set(entries);
  tree.set(entries);
  editor.refresh();
  voice?.refreshMeetings(entries);
}

function useVault(info) {
  boot.vault = info;
  entries = info.entries;
  index.set(entries);
  $("#vault-name").textContent = info.name;
  tree.set(entries);
  voice?.refreshMeetings(entries);
}

const notes = () => index.notes;

// ---------- editor ----------
const editor = createEditor($("#editor"), {
  path: () => current?.path || "",
  resolve: (target) => index.resolve(target, current?.path || ""),
  resolveMarkdown: (dest) => resolveMarkdown(dest) != null,
  fileUrl: (target) => {
    const p = index.resolveFile(target, current?.path || "") || index.resolve(target, current?.path || "");
    return p ? assetUrl(p) : null;
  },
  imageUrl: (url) => {
    if (/^(https?:|data:)/i.test(url)) return url;
    const rel = joinRel(dirname(current?.path || ""), decodeURIComponent(url));
    return rel ? assetUrl(rel) : null;
  },
  completions: () => {
    const from = current?.path || "";
    return index.notes.map((p) => {
      const text = index.linkText(p, from);
      return { text, detail: text.includes("/") ? null : dirname(p) || null };
    });
  },
  openWiki: (target, heading, withMod) => openWiki(target, heading, withMod),
  openMarkdown: (dest) => {
    const [path, hash] = dest.split("#");
    const p = resolveMarkdown(path);
    if (p) open(p, { heading: hash ? decodeURIComponent(hash) : null });
    else toast("That note doesn't exist.");
  },
  openUrl: (url) => {
    if (/^(https?:|mailto:)/i.test(url)) api.openExternal(url).catch((e) => toast(String(e), { error: true }));
  },
  saveFile: async (file) => {
    const ext = (file.name.match(/\.[a-z0-9]+$/i) || [".png"])[0];
    const name = file.name && !/^image\.\w+$/i.test(file.name) ? file.name : `Pasted image ${stamp()}${ext}`;
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const rel = await api.saveAttachment(name, bytes);
      await refreshEntries();
      return rel.split("/").pop();
    } catch (e) {
      toast(String(e), { error: true });
      return null;
    }
  },
  onChange: (u) => {
    if (applyingOutside || !current) return;
    current.dirty = true;
    scheduleSave();
    if (u.transactions.some((t) => t.isUserEvent("input") || t.isUserEvent("delete"))) appEl.classList.add("typing");
  },
  scrollTop: () => scrollEl.scrollTop,
  scrollTo: (y) => { scrollEl.scrollTop = y; },
});

function assetUrl(rel) {
  const sep = boot.platform === "windows" ? "\\" : "/";
  return convertFileSrc(boot.vault.path + sep + rel.split("/").join(sep));
}

function joinRel(dir, dest) {
  const parts = dest.startsWith("/") ? [] : dir.split("/").filter(Boolean);
  for (const seg of dest.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") parts.pop();
    else parts.push(seg);
  }
  return parts.join("/");
}

function resolveMarkdown(dest) {
  const rel = joinRel(dirname(current?.path || ""), decodeURIComponent(dest)).toLowerCase();
  return index.notes.find((n) => n.toLowerCase() === rel) || null;
}

// ---------- saving ----------
async function save() {
  scheduleSave.cancel();
  const c = current;
  if (!c || !c.dirty) return;
  c.dirty = false;
  let text = editor.text();
  if (c.crlf) text = text.replace(/\n/g, "\r\n");
  try {
    const m = await api.write(c.path, text);
    if (current === c) c.mtime = m;
  } catch (e) {
    c.dirty = true;
    toast(String(e), { error: true });
  }
}
const scheduleSave = debounce(save, 350);

// ---------- opening notes ----------
function showView(v) {
  view = v;
  columnEl.hidden = v !== "note";
  emptyEl.hidden = v !== "empty";
  voiceEl.hidden = v !== "voice";
  $("#voice-row")?.setAttribute("aria-current", v === "voice" ? "page" : "false");
  if (v !== "note") tree.select(null);
  updateCrumb();
}

function updateCrumb() {
  const crumb = $("#crumb");
  crumb.textContent = view === "note" && current ? dirname(current.path).split("/").join(" / ") : "";
}

function setTitle(path) {
  titleEl.value = basename(path);
  titleErr.hidden = true;
  fitTitle();
}
function fitTitle() {
  titleEl.style.height = "auto";
  titleEl.style.height = `${titleEl.scrollHeight}px`;
}

async function open(path, { push = true, focus = false, line = null, heading = null } = {}) {
  if (current && current.path === path && view === "note") {
    jumpWithin({ line, heading, focus });
    return;
  }
  await save();
  let note;
  try {
    note = await api.read(path);
  } catch (e) {
    toast(String(e), { error: true });
    refreshEntries();
    return;
  }
  if (push && current && current.path !== path) {
    back.push(current.path);
    if (back.length > 100) back.shift();
    forward = [];
  }
  const crlf = note.text.includes("\r\n");
  const text = crlf ? note.text.replace(/\r\n/g, "\n") : note.text;
  current = { path, mtime: note.mtime, dirty: false, crlf };
  showView("note");
  setTitle(path);
  applyingOutside = true;
  editor.load(path, text);
  applyingOutside = false;
  tree.select(path);
  updateCrumb();
  syncRecording();
  recent = [path, ...recent.filter((p) => p !== path)].slice(0, 30);
  saveUi({ lastNote: path, recent });
  jumpWithin({ line, heading, focus: focus || line != null || heading != null || !text.trim() });
  loadBacklinks();
}

function jumpWithin({ line, heading, focus }) {
  let at = null;
  if (line != null) at = editor.lineStart(line);
  if (heading) at = editor.find(heading);
  if (at != null) editor.goto(at, true);
  else if (focus) editor.focus();
}

async function openWiki(target, heading, withMod) {
  if (!target) {
    if (heading) jumpWithin({ heading, focus: true });
    return;
  }
  const p = index.resolve(target, current?.path || "");
  if (p) return open(p, { heading });
  // Like Obsidian: following a link to a missing note creates it.
  const dir = target.includes("/") ? dirname(target) : "";
  const name = target.split("/").pop();
  try {
    const path = await api.create(dir, name, "");
    await refreshEntries();
    await open(path, { focus: true });
  } catch (e) {
    toast(String(e), { error: true });
  }
}

async function goBack() {
  const p = back.pop();
  if (!p) return;
  if (current) forward.push(current.path);
  if (index.notes.includes(p)) open(p, { push: false });
}
async function goForward() {
  const p = forward.pop();
  if (!p) return;
  if (current) back.push(current.path);
  if (index.notes.includes(p)) open(p, { push: false });
}

async function loadBacklinks() {
  const path = current?.path;
  if (!path) return;
  let list = [];
  try {
    list = await api.backlinks(path);
  } catch {
    /* best effort */
  }
  if (current?.path !== path) return;
  backlinksEl.textContent = "";
  backlinksEl.hidden = !list.length;
  if (!list.length) return;
  backlinksEl.append(h("h2", {}, list.length === 1 ? "Linked from 1 note" : `Linked from ${list.length} notes`));
  for (const b of list) {
    const line = (b.lines[0] || "").replace(/!?\[\[([^\]|]*)\|([^\]]*)\]\]/g, "$2").replace(/!?\[\[([^\]]*)\]\]/g, "$1").replace(/^[-*+]\s+(\[.\]\s+)?/, "");
    const row = h("button.backlink", {}, h("div.name", {}, basename(b.path)), h("div.line", {}, line));
    row.onclick = () => open(b.path);
    backlinksEl.append(row);
  }
}

// The note a meeting is being written into stays read-only until it's done.
function syncRecording() {
  const live = !!current && view === "note" && current.path === recordingPath;
  editor.setReadOnly(live);
  $("#note-banner").hidden = !live;
}

function showEmpty() {
  current = null;
  showView("empty");
  emptyEl.textContent = "";
  emptyEl.append(
    h("div", {},
      h("p", {}, "No note is open."),
      h("p", {}, "Pick one from the sidebar, or start a new one."),
      h("button.btn.has-icon", { onclick: () => newNote(), html: `${icon("newNote")}<span>New note</span>` }),
    ),
  );
}

// ---------- create, rename, move, delete ----------
async function newNote(dir = "") {
  await save();
  try {
    const path = await api.create(dir, "Untitled", "");
    await refreshEntries();
    await open(path);
    titleEl.focus();
    titleEl.select();
  } catch (e) {
    toast(String(e), { error: true });
  }
}

async function createNote(name) {
  const dir = name.includes("/") ? dirname(name) : "";
  try {
    const path = await api.create(dir, name.split("/").pop(), "");
    await refreshEntries();
    await open(path, { focus: true });
  } catch (e) {
    toast(String(e), { error: true });
  }
}

async function createFolder(parent) {
  try {
    const path = await api.createFolder(parent, "New folder");
    await refreshEntries();
    return path;
  } catch (e) {
    toast(String(e), { error: true });
    return null;
  }
}

const remap = (p, from, to) => (p === from ? to : p.startsWith(from + "/") ? to + p.slice(from.length) : p);

async function move(from, to) {
  await save();
  let r;
  try {
    r = await api.rename(from, to);
  } catch (e) {
    toast(String(e), { error: true });
    tree.render();
    return false;
  }
  if (current) {
    const np = remap(current.path, from, r.path);
    if (np !== current.path) {
      editor.rename(current.path, np);
      current.path = np;
    }
  }
  recent = recent.map((p) => remap(p, from, r.path));
  back = back.map((p) => remap(p, from, r.path));
  forward = forward.map((p) => remap(p, from, r.path));
  if (current && r.updated.includes(current.path)) {
    const n = await api.read(current.path);
    applyOutside(n.text, n.mtime);
  }
  await refreshEntries();
  if (current && view === "note") {
    tree.select(current.path);
    setTitle(current.path);
    updateCrumb();
  }
  saveUi({ recent, lastNote: current?.path || null });
  const others = r.updated.filter((p) => p !== current?.path).length;
  if (others) toast(others === 1 ? "Updated links in 1 other note." : `Updated links in ${others} other notes.`);
  return true;
}

function applyOutside(text, mtime) {
  const crlf = text.includes("\r\n");
  const t = crlf ? text.replace(/\r\n/g, "\n") : text;
  if (t !== editor.text()) {
    applyingOutside = true;
    editor.replace(t);
    applyingOutside = false;
  }
  current.mtime = mtime;
  current.crlf = crlf;
  current.dirty = false;
}

async function reloadIfClean(path) {
  if (!current || current.path !== path || current.dirty || view !== "note") return;
  try {
    const n = await api.read(path);
    if (current?.path === path && !current.dirty) {
      const following = editor.endVisible(scrollEl);
      applyOutside(n.text, n.mtime);
      if (following && path === recordingPath) editor.revealEnd();
    }
  } catch {
    /* it may have moved */
  }
}

async function trash(path, isDir) {
  const name = isDir ? path.split("/").pop() : basename(path);
  const ok = await confirmDialog({
    title: `Move “${name}” to the trash?`,
    body: isDir
      ? "The folder and its notes move to the .trash folder inside your vault, where you can still get them back."
      : "It moves to the .trash folder inside your vault, where you can still get it back.",
    confirm: "Move to trash",
    danger: true,
  });
  if (!ok) return;
  const inside = (p) => p === path || p.startsWith(path + "/");
  if (current && inside(current.path)) {
    scheduleSave.cancel();
    current.dirty = false;
  }
  try {
    await api.trash(path);
  } catch (e) {
    toast(String(e), { error: true });
    return;
  }
  recent = recent.filter((p) => !inside(p));
  back = back.filter((p) => !inside(p));
  forward = forward.filter((p) => !inside(p));
  await refreshEntries();
  if (current && inside(current.path)) {
    editor.forget(current.path);
    current = null;
    const next = recent.find((p) => index.notes.includes(p)) || index.notes[0];
    if (next) open(next, { push: false });
    else showEmpty();
  }
  saveUi({ recent });
}

async function commitTitle() {
  if (!current) return;
  const name = titleEl.value.replace(/\s+/g, " ").trim();
  const old = basename(current.path);
  if (!name || name === old) {
    setTitle(current.path);
    return;
  }
  const dir = dirname(current.path);
  const to = (dir ? dir + "/" : "") + name + ".md";
  try {
    await move(current.path, to);
  } catch {
    /* handled in move */
  }
}

// ---------- sidebar ----------
let railTimer = null;
function setRail(open, { instant = false } = {}) {
  clearTimeout(railTimer);
  appEl.classList.remove("peeking");
  if (instant) {
    appEl.classList.toggle("rail-closed", !open);
  } else if (open) {
    appEl.classList.add("rail-moving");
    appEl.classList.remove("rail-closed");
    railTimer = setTimeout(() => appEl.classList.remove("rail-moving"), 220);
  } else {
    appEl.classList.add("rail-moving");
    railTimer = setTimeout(() => {
      appEl.classList.add("rail-closed");
      appEl.classList.remove("rail-moving");
    }, 80);
  }
  $("#toggle-rail").setAttribute("aria-pressed", String(open));
  $("#toggle-rail").setAttribute("aria-label", open ? "Hide sidebar" : "Show sidebar");
  saveUi({ railOpen: open });
  themeApplied({ meta: applyTheme({}), resolved: resolvedMode() });
}

function setupRail() {
  const width = Math.max(200, Math.min(440, ui.railWidth || 264));
  appEl.style.setProperty("--rail-w", `${width}px`);
  setRail(ui.railOpen !== false, { instant: true });

  const resizer = $("#rail-resizer");
  resizer.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    resizer.setPointerCapture(e.pointerId);
    appEl.classList.add("resizing");
    const startX = e.clientX;
    const startW = railEl.getBoundingClientRect().width;
    let raw = startW;
    const moveFn = (ev) => {
      raw = startW + ev.clientX - startX;
      const w = Math.max(200, Math.min(440, raw));
      appEl.style.setProperty("--rail-w", `${w}px`);
    };
    const up = () => {
      resizer.removeEventListener("pointermove", moveFn);
      resizer.removeEventListener("pointerup", up);
      appEl.classList.remove("resizing");
      const w = Math.max(200, Math.min(440, raw));
      saveUi({ railWidth: Math.round(w) });
      // Dragging well past the minimum folds the sidebar away.
      if (raw < 150) setRail(false);
    };
    resizer.addEventListener("pointermove", moveFn);
    resizer.addEventListener("pointerup", up);
  });
  resizer.addEventListener("dblclick", () => setRail(false));

  // Peek: the left edge brings the closed sidebar back over the page.
  let peekTimer = null;
  const zone = $("#peek-zone");
  zone.addEventListener("mouseenter", () => {
    peekTimer = setTimeout(() => appEl.classList.add("peeking"), 90);
  });
  zone.addEventListener("mouseleave", () => clearTimeout(peekTimer));
  railEl.addEventListener("mouseleave", () => {
    if (!appEl.classList.contains("peeking")) return;
    setTimeout(() => {
      if (!railEl.matches(":hover") && !document.querySelector(".menu, .popover, .dialog")) {
        appEl.classList.remove("peeking");
      }
    }, 280);
  });
}

// ---------- window chrome ----------
function setupChrome() {
  const toggle = $("#toggle-rail");
  toggle.innerHTML = icon("sidebar");
  toggle.onclick = () => setRail(appEl.classList.contains("rail-closed"));
  $("#new-note").innerHTML = icon("newNote");
  $("#new-note").onclick = () => newNote();
  $("#open-search").innerHTML = icon("search");
  $("#open-search").onclick = () => openPalette(paletteApp);
  $("#vault-chevron").outerHTML = icon("chevronDown", 14);
  $("#vault-button").onclick = (e) => vaultMenu(e.currentTarget);
  $("#settings-row").insertAdjacentHTML("afterbegin", icon("settings"));
  $("#settings-row").onclick = (e) => openSettings(e.currentTarget, settingsApp);

  if (boot.platform !== "macos") {
    const wc = $("#window-controls");
    const mk = (name, action, cls = "") => {
      const b = h(`button.icon-button${cls}`, { "aria-label": action, html: icon(name) });
      b.onclick = async () => {
        const maximized = await api.windowAction(action);
        if (action === "maximize") max.innerHTML = icon(maximized ? "restore" : "maximize");
      };
      return b;
    };
    const max = mk("maximize", "maximize");
    wc.append(mk("minimize", "minimize"), max, mk("x", "close", ".close"));
  }

  document.addEventListener("mousemove", (() => {
    let lx = 0, ly = 0;
    return (e) => {
      if (Math.abs(e.clientX - lx) + Math.abs(e.clientY - ly) > 3) appEl.classList.remove("typing");
      lx = e.clientX; ly = e.clientY;
    };
  })());

  scrollEl.addEventListener("scroll", () => scrollEl.classList.toggle("scrolled", scrollEl.scrollTop > 2), { passive: true });

  titleEl.addEventListener("input", () => { fitTitle(); titleErr.hidden = true; });
  titleEl.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      editor.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (current) setTitle(current.path);
      editor.focus();
    }
  });
  titleEl.addEventListener("blur", commitTitle);
  window.addEventListener("resize", fitTitle);
}

function vaultMenu(anchor) {
  const items = [
    { label: "Open another folder…", icon: "folder", action: chooseVault },
    { label: "Show in file manager", icon: "reveal", action: () => api.reveal("").catch(() => {}) },
  ];
  const others = (boot.recent || []).filter((p) => p !== boot.vault.path).slice(0, 5);
  if (others.length) {
    items.push("sep");
    for (const p of others) {
      const name = p.split(/[\\/]/).pop();
      items.push({ label: name, icon: "vault", action: () => switchVault(p) });
    }
  }
  openMenu(anchor, items);
}

async function chooseVault() {
  await save();
  try {
    const info = await api.chooseVault();
    if (info) afterVaultChange(info);
  } catch (e) {
    toast(String(e), { error: true });
  }
}

async function switchVault(path) {
  await save();
  try {
    afterVaultChange(await api.switchVault(path));
  } catch (e) {
    toast(String(e), { error: true });
    boot.recent = await api.forgetVault(path);
  }
}

function afterVaultChange(info) {
  boot.recent = [info.path, ...(boot.recent || []).filter((p) => p !== info.path)];
  current = null;
  back = [];
  forward = [];
  recent = [];
  useVault(info);
  const first = index.notes.find((p) => /welcome/i.test(p)) || index.notes[0];
  if (first) open(first, { push: false });
  else showEmpty();
  saveUi({ recent: [], lastNote: first || null });
}

// ---------- keys ----------
function setupKeys() {
  document.addEventListener("keydown", (e) => {
    const mod = modKey(e);
    const k = e.key.toLowerCase();
    if (mod && !e.shiftKey && (k === "k" || k === "o" || k === "p")) { e.preventDefault(); openPalette(paletteApp); }
    else if (mod && e.shiftKey && k === "f") { e.preventDefault(); openPalette(paletteApp); }
    else if (mod && !e.shiftKey && k === "n") { e.preventDefault(); newNote(); }
    else if (mod && (e.key === "\\" || e.code === "Backslash")) { e.preventDefault(); setRail(appEl.classList.contains("rail-closed")); }
    else if (mod && e.key === ",") { e.preventDefault(); openSettings($("#settings-row"), settingsApp); }
    else if (mod && e.shiftKey && k === "l") {
      e.preventDefault();
      setTheme({ mode: resolvedMode() === "dark" ? "light" : "dark" });
    }
    else if ((mod && e.key === "[") || (e.altKey && e.key === "ArrowLeft" && !e.shiftKey)) { e.preventDefault(); goBack(); }
    else if ((mod && e.key === "]") || (e.altKey && e.key === "ArrowRight" && !e.shiftKey)) { e.preventDefault(); goForward(); }
    else if (mod && (e.key === "=" || e.key === "+")) { e.preventDefault(); stepSize(1); }
    else if (mod && e.key === "-") { e.preventDefault(); stepSize(-1); }
    else if (e.key === "F5" || (mod && k === "r")) { e.preventDefault(); }
    else if (e.key === "F2" && current) { e.preventDefault(); titleEl.focus(); titleEl.select(); }
  });
}

function stepSize(d) {
  const sizes = ["s", "m", "l"];
  const i = sizes.indexOf(themePrefs().size);
  const n = sizes[Math.max(0, Math.min(2, i + d))];
  if (n !== themePrefs().size) setTheme({ size: n });
}

// ---------- the objects other modules talk to ----------
const paletteApp = {
  notes,
  recent: () => recent.filter((p) => index.notes.includes(p)),
  open: (p, o) => open(p, o),
  createNote,
};

const settingsApp = {
  setTheme,
  vaultPath: () => boot.vault.path,
  vaultDisplay: () => boot.vault.display || boot.vault.path,
  chooseVault,
  revealVault: () => api.reveal("").catch(() => {}),
  version: () => boot.version,
  voiceEnabled: () => boot.voice,
};

const treeApp = {
  ui,
  open: (p, o) => open(p, o),
  newNote,
  createFolder,
  move,
  trash,
  reveal: (p) => api.reveal(p).catch((e) => toast(String(e), { error: true })),
  saveUi,
};

// ---------- start ----------
async function start() {
  try {
    boot = await api.boot();
  } catch (e) {
    document.body.textContent = `Pinion couldn't open your notes: ${e}`;
    api.ready().catch(() => {});
    return;
  }
  root.dataset.platform = boot.platform;
  ui = boot.ui || {};
  Object.assign(treeApp, { ui });
  recent = Array.isArray(ui.recent) ? ui.recent : [];
  applyTheme({ ...DEFAULTS, ...(ui.theme || {}) }, themeApplied);

  tree = new Tree($("#tree"), treeApp);
  useVault(boot.vault);
  setupChrome();
  setupRail();
  setupKeys();

  if (boot.voice) {
    const row = $("#voice-row");
    row.hidden = false;
    row.insertAdjacentHTML("afterbegin", icon("mic"));
    voice = new VoiceView(voiceEl, {
      open: (p) => open(p),
      onState: (s, was) => {
        recordingPath = s.meeting?.path || null;
        syncRecording();
        if (was && !recordingPath && current?.path === was) reloadIfClean(was);
      },
      show: () => { closeOverlays(); save(); showView("voice"); voice.show(); },
      row,
      entries: () => entries,
      refreshEntries,
      modLabel: isMac() ? "⌘" : "Ctrl",
    });
    row.onclick = () => (view === "voice" && current ? open(current.path, { push: false }) : voice.open());
    voice.refreshMeetings(entries);
  }

  const last = ui.lastNote && index.notes.includes(ui.lastNote) ? ui.lastNote : null;
  const first = last || recent.find((p) => index.notes.includes(p)) || index.notes.find((p) => /^welcome\.md$/i.test(p)) || index.notes[0];
  if (first) await open(first, { push: false });
  else showEmpty();

  requestAnimationFrame(() => requestAnimationFrame(() => api.ready().catch(() => {})));

  window.addEventListener("focus", async () => {
    await refreshEntries();
    if (!current || current.dirty || view !== "note") return;
    try {
      const m = await api.stat(current.path);
      if (m === 0) return;
      if (m !== current.mtime) {
        const n = await api.read(current.path);
        if (!current.dirty) applyOutside(n.text, n.mtime);
      }
    } catch {
      /* the note may have moved */
    }
    loadBacklinks();
  });
  window.addEventListener("blur", () => save());
  listen("vault-changed", () => refreshEntries());
  listen("note-updated", (e) => reloadIfClean(e.payload));
  listen("meeting-started", async (e) => {
    recordingPath = e.payload;
    await refreshEntries();
    open(e.payload);
  });
  listen("open-note", (e) => { refreshEntries().then(() => open(e.payload)); });
  listen("show-voice", () => voice?.open());
  getCurrentWindow().onCloseRequested(async () => { await save(); });
}

start();
