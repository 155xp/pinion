import { h, basename, dirname } from "./dom.js";
import { icon } from "./icons.js";
import { openMenu } from "./overlays.js";

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export class Tree {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.entries = [];
    this.current = null;
    this.expanded = new Set(app.ui.expanded || []);
    this.dragPath = null;

    el.addEventListener("click", (e) => {
      const row = e.target.closest(".row");
      if (!row || row.querySelector("input")) return;
      const path = row.dataset.path;
      if (row.dataset.dir) this.toggle(path);
      else app.open(path, { focus: false });
    });
    el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const row = e.target.closest(".row");
      this.menu(row ? row.dataset.path : "", !!row?.dataset.dir || !row, { x: e.clientX, y: e.clientY });
    });
    el.addEventListener("dragstart", (e) => {
      const row = e.target.closest(".row");
      if (!row) return;
      this.dragPath = row.dataset.path;
      row.classList.add("dragging");
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", row.dataset.path);
    });
    el.addEventListener("dragend", () => {
      this.dragPath = null;
      el.querySelectorAll(".dragging, .drop-target").forEach((r) => r.classList.remove("dragging", "drop-target"));
    });
    el.addEventListener("dragover", (e) => {
      if (!this.dragPath) return;
      const target = this.dropFolder(e);
      if (target == null) return;
      e.preventDefault();
      el.querySelectorAll(".drop-target").forEach((r) => r.classList.remove("drop-target"));
      const row = target ? el.querySelector(`.row[data-path="${CSS.escape(target)}"]`) : null;
      row?.classList.add("drop-target");
    });
    el.addEventListener("drop", (e) => {
      if (!this.dragPath) return;
      e.preventDefault();
      const target = this.dropFolder(e);
      const from = this.dragPath;
      this.dragPath = null;
      if (target == null) return;
      const name = from.split("/").pop();
      const to = target ? `${target}/${name}` : name;
      if (to !== from) app.move(from, to);
    });
  }

  dropFolder(e) {
    const row = e.target.closest(".row");
    let folder = "";
    if (row) folder = row.dataset.dir ? row.dataset.path : dirname(row.dataset.path);
    const from = this.dragPath;
    if (!from) return null;
    if (folder === dirname(from)) return null;
    if (folder === from || folder.startsWith(from + "/")) return null;
    return folder;
  }

  set(entries) {
    this.entries = entries;
    this.render();
  }

  select(path) {
    this.current = path;
    if (path) {
      // Open the folders above the current note.
      let d = dirname(path);
      let changed = false;
      while (d) {
        if (!this.expanded.has(d)) { this.expanded.add(d); changed = true; }
        d = dirname(d);
      }
      if (changed) this.persist();
    }
    this.render();
    const row = this.el.querySelector(`.row[aria-current="page"]`);
    row?.scrollIntoView({ block: "nearest" });
  }

  toggle(path) {
    if (this.expanded.has(path)) this.expanded.delete(path);
    else this.expanded.add(path);
    this.persist();
    this.render();
  }

  persist() {
    this.app.saveUi({ expanded: [...this.expanded] });
  }

  render() {
    const children = new Map();
    for (const e of this.entries) {
      if (!e.dir && !/\.md$/i.test(e.path)) continue;
      const d = dirname(e.path);
      if (!children.has(d)) children.set(d, []);
      children.get(d).push(e);
    }
    for (const list of children.values()) {
      list.sort((a, b) => (a.dir !== b.dir ? (a.dir ? -1 : 1) : collator.compare(basename(a.path), basename(b.path))));
    }
    const frag = document.createDocumentFragment();
    const walk = (dir, depth) => {
      for (const e of children.get(dir) || []) {
        const open = e.dir && this.expanded.has(e.path);
        const row = h("div.row", {
          role: "treeitem",
          draggable: "true",
          dataset: e.dir ? { path: e.path, dir: "1" } : { path: e.path },
          style: { "--depth": depth },
          "aria-expanded": e.dir ? String(open) : null,
          "aria-current": !e.dir && e.path === this.current ? "page" : null,
          html: `<span class="slot">${e.dir ? icon("chevronRight", 14) : ""}</span>`,
        }, h("span.name", {}, e.dir ? e.path.split("/").pop() : basename(e.path)));
        frag.append(row);
        if (open) walk(e.path, depth + 1);
      }
    };
    walk("", 0);
    this.el.textContent = "";
    if (!frag.childNodes.length) {
      this.el.append(h("div.tree-empty", {}, "No notes yet."));
      return;
    }
    this.el.append(frag);
  }

  menu(path, isDir, at) {
    const app = this.app;
    const items = [];
    if (isDir) {
      items.push(
        { label: "New note", icon: "newNote", action: () => app.newNote(path) },
        { label: "New folder", icon: "folderPlus", action: () => this.newFolder(path) },
      );
      if (path) {
        items.push("sep",
          { label: "Rename", icon: "rename", action: () => this.rename(path) },
          { label: "Show in folder", icon: "reveal", action: () => app.reveal(path) },
          "sep",
          { label: "Move to trash", icon: "trash", danger: true, action: () => app.trash(path, true) },
        );
      }
    } else {
      items.push(
        { label: "Rename", icon: "rename", action: () => this.rename(path) },
        { label: "Show in folder", icon: "reveal", action: () => app.reveal(path) },
        "sep",
        { label: "Move to trash", icon: "trash", danger: true, action: () => app.trash(path, false) },
      );
    }
    openMenu(at, items);
  }

  async newFolder(parent) {
    const path = await this.app.createFolder(parent);
    if (!path) return;
    this.expanded.add(path);
    if (parent) this.expanded.add(parent);
    this.persist();
    this.render();
    this.rename(path);
  }

  rename(path) {
    const row = this.el.querySelector(`.row[data-path="${CSS.escape(path)}"]`);
    if (!row) return;
    const isDir = !!row.dataset.dir;
    const nameEl = row.querySelector(".name");
    const old = isDir ? path.split("/").pop() : basename(path);
    const input = h("input", { value: old, spellcheck: false, "aria-label": "Name" });
    nameEl.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = async (commit) => {
      if (done) return;
      done = true;
      const v = input.value.trim();
      if (commit && v && v !== old) {
        const dir = dirname(path);
        const to = (dir ? dir + "/" : "") + v + (isDir ? "" : ".md");
        await this.app.move(path, to);
      } else {
        this.render();
      }
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); finish(true); }
      if (e.key === "Escape") { e.preventDefault(); finish(false); }
      e.stopPropagation();
    });
    input.addEventListener("blur", () => finish(true));
  }
}
