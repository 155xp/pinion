import { h } from "./dom.js";
import { icon } from "./icons.js";
import { THEMES, themePrefs } from "./theme.js";
import { openPopover } from "./overlays.js";

// A segmented toggle with a sliding raised pill. The first placement snaps.
export function segmented(options, value, onChange, { fill = false, label } = {}) {
  const track = h(`div.segmented${fill ? ".fill" : ""}`, { role: "radiogroup", "aria-label": label || "" });
  const thumb = h("span.thumb.instant");
  track.append(thumb);
  const buttons = options.map((o) => {
    const b = h("button", {
      role: "radio",
      "aria-checked": String(o.value === value),
      tabindex: o.value === value ? "0" : "-1",
      html: (o.icon ? icon(o.icon, 14) : "") + `<span>${o.label}</span>`,
    });
    b.onclick = () => select(o.value, true);
    return b;
  });
  track.append(...buttons);
  const move = (instant) => {
    const i = options.findIndex((o) => o.value === value);
    const b = buttons[i];
    if (!b || !b.offsetWidth) return;
    thumb.classList.toggle("instant", instant);
    thumb.style.width = `${b.offsetWidth}px`;
    thumb.style.translate = `${b.offsetLeft}px 0`;
  };
  const select = (v, user) => {
    if (v === value) return;
    value = v;
    buttons.forEach((b, i) => {
      const on = options[i].value === v;
      b.setAttribute("aria-checked", String(on));
      b.tabIndex = on ? 0 : -1;
    });
    move(!user);
    if (user) onChange(v);
  };
  track.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    const i = options.findIndex((o) => o.value === value);
    const n = (i + (e.key === "ArrowRight" ? 1 : -1) + options.length) % options.length;
    select(options[n].value, true);
    buttons[n].focus();
  });
  requestAnimationFrame(() => move(true));
  track.set = (v) => select(v, false);
  return track;
}

export function openSettings(anchor, app) {
  const prefs = themePrefs();
  const colourName = h("span.value", {}, THEMES.find((t) => t.id === prefs.tint)?.name || "");
  const swatches = h("div.swatches", { role: "radiogroup", "aria-label": "Colour" },
    THEMES.map((t) => {
      const b = h("button.swatch", {
        role: "radio",
        "aria-checked": String(t.id === prefs.tint),
        "aria-label": t.name,
        title: t.name,
        dataset: { swatch: t.id },
      });
      b.onclick = () => {
        swatches.querySelectorAll(".swatch").forEach((s) => s.setAttribute("aria-checked", String(s === b)));
        colourName.textContent = t.name;
        app.setTheme({ tint: t.id });
      };
      b.onmouseenter = () => { colourName.textContent = t.name; };
      b.onmouseleave = () => { colourName.textContent = THEMES.find((x) => x.id === themePrefs().tint)?.name || ""; };
      return b;
    }),
  );

  const vaultPath = h("div.setting-path", { title: app.vaultPath() }, app.vaultDisplay());
  const content = h("div", {},
    h("div.setting", {},
      h("div.setting-label", {}, h("span", {}, "Mode")),
      segmented(
        [
          { value: "light", label: "Light", icon: "sun" },
          { value: "dark", label: "Dark", icon: "moon" },
          { value: "auto", label: "Auto", icon: "auto" },
        ],
        prefs.mode,
        (v) => app.setTheme({ mode: v }),
        { fill: true, label: "Mode" },
      ),
    ),
    h("div.setting", {},
      h("div.setting-label", {}, h("span", {}, "Colour"), colourName),
      swatches,
    ),
    h("div.setting", {},
      h("div.setting-label", {}, h("span", {}, "Writing")),
      segmented(
        [
          { value: "sans", label: "Sans" },
          { value: "serif", label: "Serif" },
          { value: "mono", label: "Mono" },
        ],
        prefs.font,
        (v) => app.setTheme({ font: v }),
        { fill: true, label: "Font" },
      ),
      h("div", { style: { height: "8px" } }),
      segmented(
        [
          { value: "s", label: "Small" },
          { value: "m", label: "Medium" },
          { value: "l", label: "Large" },
        ],
        prefs.size,
        (v) => app.setTheme({ size: v }),
        { fill: true, label: "Text size" },
      ),
    ),
    h("div.setting", {},
      h("div.setting-label", {}, h("span", {}, "Notes folder")),
      vaultPath,
      h("div.setting-actions", {},
        h("button.btn.compact", { onclick: () => { pop.close(); app.chooseVault(); } }, "Change…"),
        h("button.btn.compact", { onclick: () => app.revealVault() }, "Show folder"),
      ),
    ),
    h("div.setting-foot", {}, h("span", {}, `Pinion ${app.version()}`), app.voiceEnabled() ? h("span", {}, "Voice on") : null),
  );
  const pop = openPopover(anchor, content, { prefer: "above" });
  return pop;
}
