import { h, basename, dirname, debounce, modLabel } from "./dom.js";
import { icon } from "./icons.js";
import { api } from "./api.js";
import { closeOverlays } from "./overlays.js";

const SEP = /[\s\-_/.]/;

function score(q, s) {
  const t = s.toLowerCase();
  const i = t.indexOf(q);
  if (i === 0) return 1000 - t.length;
  if (i > 0) return 700 - i + (SEP.test(t[i - 1]) ? 150 : 0) - t.length * 0.1;
  let from = 0, sc = 0, last = -2;
  for (const c of q) {
    const f = t.indexOf(c, from);
    if (f < 0) return -1;
    sc += f === last + 1 ? 8 : f === 0 || SEP.test(t[f - 1]) ? 6 : 1;
    last = f;
    from = f + 1;
  }
  return sc;
}

function highlight(name, q) {
  const i = q ? name.toLowerCase().indexOf(q) : -1;
  const span = document.createElement("span");
  span.className = "name";
  if (i < 0) { span.textContent = name; return span; }
  span.append(name.slice(0, i), h("mark", {}, name.slice(i, i + q.length)), name.slice(i + q.length));
  return span;
}

export function openPalette(app, { query = "" } = {}) {
  closeOverlays();
  const prev = document.activeElement;
  const backdrop = h("div.backdrop");
  const input = h("input.palette-input", {
    type: "text",
    placeholder: "Find or create a note…",
    spellcheck: false,
    autocomplete: "off",
    "aria-label": "Find a note",
  });
  const list = h("div.palette-list", { role: "listbox" });
  const foot = h("div.palette-foot", {},
    h("span", { html: `${icon("arrows", 14)} Move` }),
    h("span", { html: `${icon("enter", 14)} Open` }),
    h("span", {}, h("span.key", {}, modLabel()), h("span.key", {}, "↵"), " New note"),
  );
  const box = h("div.palette.surface-overlay", { role: "dialog", "aria-label": "Find a note" }, input, list, foot);
  document.body.append(backdrop, box);
  input.value = query;
  input.focus();
  input.select();

  let rows = [];
  let active = 0;
  let hits = [];
  let token = 0;

  const close = () => {
    document.removeEventListener("keydown", onKey, true);
    backdrop.classList.add("closing");
    box.classList.add("closing");
    setTimeout(() => { backdrop.remove(); box.remove(); }, 160);
    if (prev && document.contains(prev)) prev.focus?.();
  };

  const setActive = (i, scroll = true) => {
    active = Math.max(0, Math.min(i, rows.length - 1));
    rows.forEach((r, j) => r.el.classList.toggle("active", j === active));
    if (scroll) rows[active]?.el.scrollIntoView({ block: "nearest" });
  };

  const run = (row, withMod) => {
    close();
    row.run(withMod);
  };

  function render() {
    const q = input.value.trim().toLowerCase();
    list.textContent = "";
    rows = [];
    const addRow = (el, runFn) => {
      const idx = rows.length;
      el.addEventListener("mousemove", () => { if (active !== idx) setActive(idx, false); });
      el.addEventListener("click", (e) => run(rows[idx], e.metaKey || e.ctrlKey));
      rows.push({ el, run: runFn });
      list.append(el);
    };
    const noteRow = (path) => {
      const name = basename(path);
      const where = dirname(path);
      const el = h("button.palette-row", { role: "option", html: icon("note") },
        h("span.text", {}, highlight(name, q)),
        where ? h("span.where", {}, where) : null,
      );
      addRow(el, () => app.open(path));
    };

    if (!q) {
      const recent = app.recent().slice(0, 8);
      if (recent.length) list.append(h("div.palette-group", {}, "Recent"));
      recent.forEach(noteRow);
      if (!recent.length) list.append(h("div.palette-empty", {}, "Type to find a note."));
    } else {
      const scored = app.notes()
        .map((p) => ({ p, s: Math.max(score(q, basename(p)), score(q, p.replace(/\.md$/i, "")) - 200) }))
        .filter((x) => x.s >= 0)
        .sort((a, b) => b.s - a.s)
        .slice(0, 8);
      scored.forEach((x) => noteRow(x.p));
      const exact = app.notes().some((p) => basename(p).toLowerCase() === q);
      if (!exact) {
        const name = input.value.trim();
        const el = h("button.palette-row", { role: "option", html: icon("plus") },
          h("span.text", {}, h("span.name", {}, `Create “${name}”`)),
        );
        addRow(el, () => app.createNote(name));
      }
      const shown = new Set(scored.map((x) => x.p));
      const textHits = hits.filter((x) => !shown.has(x.path) || true).slice(0, 12);
      if (textHits.length) {
        list.append(h("div.palette-group", {}, "In notes"));
        for (const hit of textHits) {
          const el = h("button.palette-row", { role: "option", html: icon("search") },
            h("span.text", {}, h("span.name", {}, basename(hit.path)), h("span.sub", {}, hit.text)),
          );
          addRow(el, () => app.open(hit.path, { line: hit.line }));
        }
      }
    }
    setActive(0, false);
  }

  const searchText = debounce(async () => {
    const q = input.value.trim();
    const t = ++token;
    if (q.length < 2) { hits = []; render(); return; }
    try {
      const res = await api.search(q);
      if (t === token) { hits = res; render(); }
    } catch {
      /* searching is best effort */
    }
  }, 140);

  input.addEventListener("input", () => {
    hits = [];
    render();
    searchText();
  });

  const onKey = (e) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setActive(active + 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive(active - 1); }
    else if (e.key === "Enter") {
      e.preventDefault();
      const withMod = e.metaKey || e.ctrlKey;
      if (withMod && input.value.trim()) { close(); app.createNote(input.value.trim()); return; }
      if (rows[active]) run(rows[active], withMod);
    }
  };
  document.addEventListener("keydown", onKey, true);
  backdrop.onclick = close;
  render();
  if (query) searchText();
}
