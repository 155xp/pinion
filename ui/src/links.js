// Obsidian-style link resolution. Mirrors src-tauri/src/links.rs.
import { basename, dirname } from "./dom.js";

const IMAGE = /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/i;
const stripMd = (p) => p.replace(/\.md$/i, "");

export class LinkIndex {
  constructor() {
    this.notes = [];
    this.files = [];
    this.byName = new Map();
    this.byFileName = new Map();
    this.version = 0;
  }

  set(entries) {
    this.notes = [];
    this.files = [];
    this.byName = new Map();
    this.byFileName = new Map();
    for (const e of entries) {
      if (e.dir) continue;
      if (/\.md$/i.test(e.path)) {
        this.notes.push(e.path);
        const k = basename(e.path).toLowerCase();
        if (!this.byName.has(k)) this.byName.set(k, []);
        this.byName.get(k).push(e.path);
      } else {
        this.files.push(e.path);
        const k = e.path.split("/").pop().toLowerCase();
        if (!this.byFileName.has(k)) this.byFileName.set(k, []);
        this.byFileName.get(k).push(e.path);
      }
    }
    this.version++;
  }

  // A bare name matches any note with that name, closest to `from` first;
  // a name with a slash matches by path suffix.
  resolve(target, from = "") {
    let t = stripMd(target.trim().replace(/^\/+/, "")).toLowerCase();
    if (!t) return null;
    if (t.includes("/")) {
      let best = null;
      for (const n of this.notes) {
        const p = stripMd(n).toLowerCase();
        if (p === t) return n;
        if (p.endsWith("/" + t) && (!best || p.length < best.length)) best = n;
      }
      return best;
    }
    const list = this.byName.get(t);
    if (!list) return null;
    return pickClosest(list, from);
  }

  resolveFile(target, from = "") {
    const t = target.trim().replace(/^\/+/, "");
    const lower = t.toLowerCase();
    if (lower.includes("/")) {
      return this.files.find((f) => f.toLowerCase() === lower || f.toLowerCase().endsWith("/" + lower)) || null;
    }
    const list = this.byFileName.get(lower);
    return list ? pickClosest(list, from) : null;
  }

  // The shortest link text that still points at `path` from `from`.
  linkText(path, from = "") {
    const name = basename(path);
    return this.resolve(name, from) === path ? name : stripMd(path);
  }
}

function pickClosest(list, from) {
  if (list.length === 1) return list[0];
  const fd = dirname(from).toLowerCase();
  return [...list].sort((a, b) => {
    const sa = dirname(a).toLowerCase() === fd ? 0 : 1;
    const sb = dirname(b).toLowerCase() === fd ? 0 : 1;
    if (sa !== sb) return sa - sb;
    const da = a.split("/").length, db = b.split("/").length;
    if (da !== db) return da - db;
    return a.length - b.length;
  })[0];
}

export const isImage = (name) => IMAGE.test(name);

// Splits the inside of [[...]] into target, heading and alias.
export function parseWiki(inner) {
  const bar = inner.indexOf("|");
  const main = bar < 0 ? inner : inner.slice(0, bar);
  const alias = bar < 0 ? null : inner.slice(bar + 1);
  const hash = main.indexOf("#");
  return {
    target: (hash < 0 ? main : main.slice(0, hash)).trim(),
    heading: hash < 0 ? null : main.slice(hash + 1).trim(),
    alias: alias && alias.trim() ? alias.trim() : null,
    aliasOffset: bar < 0 ? -1 : bar + 1,
  };
}
