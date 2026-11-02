const REPO = "155xp/pinion";
const root = document.documentElement;
const $ = (id) => document.getElementById(id);
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

// ---------- colours ----------
const TINTS = [
  ["paper", "Paper"],
  ["pure", "Pure"],
  ["rose", "Rose"],
  ["lilac", "Lilac"],
  ["sage", "Sage"],
  ["sky", "Sky"],
];
const SUN = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>';
const MOON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></svg>';

function saveTheme(patch) {
  let s = {};
  try { s = JSON.parse(localStorage.getItem("pinion-theme") || "{}"); } catch {}
  Object.assign(s, patch);
  try { localStorage.setItem("pinion-theme", JSON.stringify(s)); } catch {}
}

function paintMode() {
  const dark = root.dataset.mode === "dark";
  $("mode").innerHTML = dark ? SUN : MOON;
  $("mode").setAttribute("aria-label", dark ? "Switch to light" : "Switch to dark");
  document.querySelector('meta[name="theme-color"]').content = getComputedStyle(root).getPropertyValue("--page").trim() || "#fbfaf8";
}

for (const [id, name] of TINTS) {
  const b = document.createElement("button");
  b.className = "swatch";
  b.dataset.swatch = id;
  b.setAttribute("role", "radio");
  b.setAttribute("aria-label", name);
  b.title = name;
  b.setAttribute("aria-checked", String(root.dataset.tint === id));
  b.onclick = () => {
    root.dataset.tint = id;
    document.querySelectorAll(".swatch").forEach((s) => s.setAttribute("aria-checked", String(s === b)));
    saveTheme({ tint: id });
    paintMode();
  };
  $("swatches").append(b);
}
$("mode").onclick = () => {
  root.dataset.mode = root.dataset.mode === "dark" ? "light" : "dark";
  saveTheme({ mode: root.dataset.mode });
  paintMode();
};
paintMode();

// ---------- the checklist ----------
document.querySelectorAll(".check").forEach((c) => {
  c.onclick = () => {
    const done = !c.classList.contains("done");
    c.classList.toggle("done", done);
    c.setAttribute("aria-pressed", String(done));
    c.setAttribute("aria-label", done ? "Done" : "Not done");
  };
});

// ---------- the voice demo ----------
const SENTENCE = "Let’s meet on Thursday at ten, and I’ll bring the notes.";
const pill = $("demo-pill");
const typed = $("typed");
const bars = $("demo-bars");
bars.innerHTML = "<span></span>".repeat(11);
let running = false;
let timers = [];
let wave = null;
const later = (ms, fn) => timers.push(setTimeout(fn, ms));

function speak() {
  const spans = bars.children;
  const levels = new Array(11).fill(0.12);
  let t = 0;
  wave = setInterval(() => {
    t += 1;
    const v = 0.25 + Math.abs(Math.sin(t * 0.55) * Math.sin(t * 0.21)) * 0.75 * (0.6 + Math.random() * 0.4);
    levels.shift();
    levels.push(v);
    for (let i = 0; i < 11; i++) spans[i].style.transform = `scaleY(${levels[i]})`;
  }, 90);
}

function cycle() {
  if (!running) return;
  typed.textContent = "";
  pill.dataset.face = "idle";
  later(900, () => { pill.dataset.face = "listening"; speak(); });
  later(3700, () => { clearInterval(wave); pill.dataset.face = "working"; });
  later(4400, () => { pill.dataset.face = "idle"; typed.textContent = SENTENCE; });
  later(9000, cycle);
}

if (reduced) {
  typed.textContent = SENTENCE;
} else {
  new IntersectionObserver((entries) => {
    const visible = entries.some((e) => e.isIntersecting);
    if (visible && !running) { running = true; cycle(); }
    if (!visible && running) {
      running = false;
      timers.forEach(clearTimeout);
      timers = [];
      clearInterval(wave);
      pill.dataset.face = "idle";
    }
  }, { threshold: 0.6 }).observe($("demo-pill").parentElement);
}

// ---------- downloads ----------
const ua = navigator.userAgent;
const os = /Mac/i.test(ua) && !/iPhone|iPad/i.test(ua) ? "mac"
  : /Windows/i.test(ua) ? "windows"
  : /Linux|X11/i.test(ua) && !/Android/i.test(ua) ? "linux"
  : "other";
const FILES = [
  { key: "mac", label: "macOS", sub: "Apple silicon", file: "macos-arm64.dmg" },
  { key: "mac-intel", label: "macOS", sub: "Intel", file: "macos-x64.dmg" },
  { key: "windows", label: "Windows", file: "windows-x64-setup.exe" },
  { key: "linux", label: "Linux", sub: ".deb", file: "linux-amd64.deb" },
  { key: "linux-rpm", label: "Linux", sub: ".rpm", file: "linux-x86_64.rpm" },
];
let assets = {};
let variant = "plain";

const mb = (bytes) => `${(bytes / 1e6).toFixed(1)} MB`;
const name = (f) => `${variant === "voice" ? "Pinion-Voice" : "Pinion"}-${f.file}`;
const url = (f) => assets[name(f)]?.url || `https://github.com/${REPO}/releases/latest/download/${name(f)}`;
const size = (f) => (assets[name(f)] ? mb(assets[name(f)].size) : "");

function renderDownloads() {
  const main = FILES.find((f) => f.key === os);
  const primary = $("primary");
  if (main) {
    primary.href = url(main);
    $("primary-label").textContent = `Download for ${main.label}`;
    $("primary-size").textContent = size(main);
    primary.hidden = false;
  } else {
    primary.hidden = true;
  }
  const others = FILES.filter((f) => f !== main);
  const parts = others.map((f) => {
    const s = size(f);
    return `<a href="${url(f)}">${f.label}${f.sub ? ` (${f.sub})` : ""}</a>${s ? ` <span class="size">${s}</span>` : ""}`;
  });
  $("others").innerHTML = (main ? "Also for " : "For ") + parts.join(", ") + ".";
  $("variant-note").textContent = variant === "voice"
    ? "Everything in Pinion, plus voice typing and meeting notes. The speech model, 416 MB, downloads inside the app the first time you turn voice on."
    : "Notes and nothing else. You can add voice later by installing the other one.";
}

const toggle = $("variant");
const thumb = toggle.querySelector(".thumb");
const buttons = [...toggle.querySelectorAll("button")];
function placeThumb(instant) {
  const b = buttons.find((x) => x.getAttribute("aria-checked") === "true");
  thumb.classList.toggle("instant", instant);
  thumb.style.width = `${b.offsetWidth}px`;
  thumb.style.translate = `${b.offsetLeft}px 0`;
}
buttons.forEach((b) => {
  b.onclick = () => {
    variant = b.dataset.variant;
    buttons.forEach((x) => x.setAttribute("aria-checked", String(x === b)));
    placeThumb(false);
    renderDownloads();
  };
});
toggle.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  const i = buttons.findIndex((x) => x.getAttribute("aria-checked") === "true");
  const next = buttons[(i + 1) % 2];
  next.click();
  next.focus();
});
document.fonts.ready.then(() => placeThumb(true));
window.addEventListener("resize", () => placeThumb(true));
renderDownloads();

fetch(`https://api.github.com/repos/${REPO}/releases/latest`)
  .then((r) => (r.ok ? r.json() : null))
  .then((rel) => {
    if (!rel) return;
    for (const a of rel.assets || []) assets[a.name] = { url: a.browser_download_url, size: a.size };
    if (rel.tag_name) {
      $("version").textContent = rel.tag_name;
      $("version").hidden = false;
    }
    renderDownloads();
  })
  .catch(() => {});
