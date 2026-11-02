import { THEME_META, THEMES } from "./theme-meta.js";

export { THEMES };
export const DEFAULTS = { mode: "auto", tint: "paper", font: "sans", size: "m" };

const mq = window.matchMedia("(prefers-color-scheme: dark)");
let current = { ...DEFAULTS };
let listener = null;

export function resolvedMode(mode = current.mode) {
  return mode === "auto" ? (mq.matches ? "dark" : "light") : mode;
}

export function applyTheme(prefs, onApplied) {
  current = { ...current, ...prefs };
  if (onApplied) listener = onApplied;
  const root = document.documentElement;
  const mode = resolvedMode();
  root.dataset.mode = mode;
  root.dataset.tint = THEME_META[`${current.tint}-${mode}`] ? current.tint : "paper";
  root.dataset.font = current.font;
  root.dataset.size = current.size;
  const meta = THEME_META[`${root.dataset.tint}-${mode}`];
  listener?.({ ...current, mode, resolved: mode, meta });
  return meta;
}

export const themePrefs = () => ({ ...current });

mq.addEventListener("change", () => {
  if (current.mode === "auto") applyTheme({});
});
