// Menus, popovers, the confirm dialog and toasts.
import { h } from "./dom.js";
import { icon } from "./icons.js";

let openClose = null;

function place(el, anchor, { gap = 6, prefer = "below", align = "start" } = {}) {
  const r = el.getBoundingClientRect();
  const vw = window.innerWidth, vh = window.innerHeight;
  let x, y, originY;
  if (anchor instanceof Element) {
    const a = anchor.getBoundingClientRect();
    x = align === "end" ? a.right - r.width : a.left;
    const below = a.bottom + gap, above = a.top - gap - r.height;
    const fitsBelow = below + r.height <= vh - 8;
    if (prefer === "above" ? above >= 8 || !fitsBelow : !fitsBelow && above >= 8) {
      y = above; originY = "bottom";
    } else {
      y = below; originY = "top";
    }
  } else {
    x = anchor.x; y = anchor.y; originY = "top";
    if (y + r.height > vh - 8) { y = Math.max(8, anchor.y - r.height); originY = "bottom"; }
  }
  x = Math.max(8, Math.min(x, vw - r.width - 8));
  y = Math.max(8, Math.min(y, vh - r.height - 8));
  el.style.left = `${Math.round(x)}px`;
  el.style.top = `${Math.round(y)}px`;
  el.style.setProperty("--origin", `${originY} ${align === "end" ? "right" : "left"}`);
}

function dismissable(el, close, { trigger } = {}) {
  const onDown = (e) => {
    if (el.contains(e.target) || (trigger && trigger.contains(e.target))) return;
    close();
  };
  const onKey = (e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } };
  const onResize = () => close();
  setTimeout(() => {
    document.addEventListener("mousedown", onDown, true);
    window.addEventListener("resize", onResize);
    window.addEventListener("blur", onResize);
  });
  document.addEventListener("keydown", onKey, true);
  return () => {
    document.removeEventListener("mousedown", onDown, true);
    document.removeEventListener("keydown", onKey, true);
    window.removeEventListener("resize", onResize);
    window.removeEventListener("blur", onResize);
  };
}

function fadeOut(el, ms = 120) {
  el.classList.add("closing");
  el.style.pointerEvents = "none";
  setTimeout(() => el.remove(), ms);
}

export function closeOverlays() {
  if (openClose) openClose();
}

/** items: { label, icon, hint, danger, chosen, action } | "sep" | { note } */
export function openMenu(anchor, items, opts = {}) {
  closeOverlays();
  const menu = h("div.menu.surface-overlay", { role: "menu" });
  const buttons = [];
  for (const it of items) {
    if (it === "sep") { menu.append(h("div.menu-sep")); continue; }
    if (it.note) { menu.append(h("div.menu-note", {}, it.note)); continue; }
    const b = h(`button.menu-item${it.danger ? ".danger" : ""}${it.chosen ? ".chosen" : ""}`, {
      role: "menuitem",
      html: (it.icon ? icon(it.icon) : "") + `<span class="label"></span>` + (it.hint ? `<span class="hint">${it.hint}</span>` : ""),
    });
    b.querySelector(".label").textContent = it.label;
    b.addEventListener("click", () => { close(); it.action?.(); });
    b.addEventListener("mousemove", () => setActive(buttons.indexOf(b)));
    buttons.push(b);
    menu.append(b);
  }
  document.body.append(menu);
  place(menu, anchor, opts);
  let active = -1;
  const setActive = (i) => {
    buttons.forEach((b, j) => b.classList.toggle("active", j === i));
    active = i;
  };
  const onKey = (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = buttons.length;
      setActive((active + (e.key === "ArrowDown" ? 1 : -1) + n) % n);
    } else if (e.key === "Enter" && active >= 0) {
      e.preventDefault();
      buttons[active].click();
    }
  };
  document.addEventListener("keydown", onKey, true);
  const trigger = anchor instanceof Element ? anchor : null;
  trigger?.setAttribute("aria-expanded", "true");
  const undo = dismissable(menu, () => close(), { trigger });
  let closed = false;
  function close() {
    if (closed) return;
    closed = true;
    undo();
    document.removeEventListener("keydown", onKey, true);
    trigger?.setAttribute("aria-expanded", "false");
    fadeOut(menu);
    if (openClose === close) openClose = null;
    opts.onClose?.();
  }
  openClose = close;
  return close;
}

export function openPopover(anchor, content, opts = {}) {
  closeOverlays();
  const pop = h("div.popover.surface-overlay", { role: "dialog" }, content);
  document.body.append(pop);
  place(pop, anchor, { prefer: opts.prefer || "above", gap: 8, align: opts.align });
  const trigger = anchor instanceof Element ? anchor : null;
  trigger?.setAttribute("aria-expanded", "true");
  const undo = dismissable(pop, () => close(), { trigger });
  let closed = false;
  function close() {
    if (closed) return;
    closed = true;
    undo();
    trigger?.setAttribute("aria-expanded", "false");
    fadeOut(pop);
    if (openClose === close) openClose = null;
    opts.onClose?.();
  }
  openClose = close;
  return { el: pop, close, reposition: () => place(pop, anchor, { prefer: opts.prefer || "above", gap: 8, align: opts.align }) };
}

export function confirmDialog({ title, body, confirm = "Confirm", danger = false }) {
  return new Promise((resolve) => {
    closeOverlays();
    const prev = document.activeElement;
    const backdrop = h("div.backdrop");
    const ok = h(`button.btn${danger ? ".danger" : ".primary"}`, {}, confirm);
    const cancel = h("button.btn", {}, "Cancel");
    const titleEl = h(`h2${danger ? ".danger" : ""}`, { id: "dialog-title" }, title);
    const dlg = h("div.dialog.surface-overlay", { role: "dialog", "aria-modal": "true", "aria-labelledby": "dialog-title" },
      h("div.dialog-head", {}, titleEl, body ? h("p", {}, body) : null),
      h("div.dialog-foot", {}, cancel, ok),
    );
    document.body.append(backdrop, dlg);
    ok.focus();
    const done = (v) => {
      document.removeEventListener("keydown", onKey, true);
      fadeOut(backdrop, 200);
      fadeOut(dlg, 200);
      prev?.focus?.();
      resolve(v);
    };
    const onKey = (e) => {
      if (e.key === "Escape") { e.preventDefault(); done(false); }
      if (e.key === "Tab") {
        e.preventDefault();
        (document.activeElement === ok ? cancel : ok).focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    ok.onclick = () => done(true);
    cancel.onclick = () => done(false);
    backdrop.onclick = () => done(false);
  });
}

let toastEl = null;
let toastTimer = null;
export function toast(message, { error = false, action = null, ms = 6000 } = {}) {
  if (toastEl) { fadeOut(toastEl, 200); toastEl = null; }
  clearTimeout(toastTimer);
  const el = h(`div.toast${error ? ".error" : ""}${action ? "" : ".only-text"}`, { role: "status" }, h("span", {}, message));
  if (action) {
    const b = h("button.btn.pill", {}, action.label);
    b.onclick = () => { action.run(); dismiss(); };
    el.append(b);
  }
  document.body.append(el);
  toastEl = el;
  const dismiss = () => { if (toastEl === el) { fadeOut(el, 200); toastEl = null; } };
  toastTimer = setTimeout(dismiss, ms);
  return dismiss;
}
