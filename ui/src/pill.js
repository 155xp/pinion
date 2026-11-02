// The little pill at the bottom of the screen.
import { listen } from "@tauri-apps/api/event";
import { api } from "./api.js";
import { icon } from "./icons.js";

const root = document.documentElement;
const pill = document.getElementById("pill");
const $ = (id) => document.getElementById(id);
const BARS = 11;
const MBARS = 5;

let state = null;
let hover = false;
let notice = null;
let noticeTimer = null;
let clockTimer = null;
const levels = new Array(BARS).fill(0);

$("bars").innerHTML = "<span></span>".repeat(BARS);
$("mbars").innerHTML = "<span></span>".repeat(MBARS);
$("cancel").innerHTML = icon("x", 14);
$("done").innerHTML = icon("check", 14);
$("mstop").innerHTML = icon("stop", 14);

const SIZES = {
  idle: [48, 8],
  idleHover: [232, 36],
  listening: [128, 36],
  handsfree: [160, 36],
  working: [80, 36],
  meeting: [204, 36],
};

function keys(label) {
  return label.split(" ").map((k) => `<span class="k">${k}</span>`).join("");
}

function face() {
  if (notice) return "notice";
  if (!state) return "idle";
  if (state.dictation === "listening" || state.dictation === "handsfree") return "listening";
  if (state.dictation === "working") return "working";
  if (state.meeting) return "meeting";
  return "idle";
}

function render() {
  const f = face();
  pill.dataset.face = f;
  pill.classList.toggle("hover", hover);
  pill.classList.toggle("hidden-idle", f === "idle" && !hover && state && !state.settings.showPill);
  const mac = state?.platform === "macos";
  let size;
  if (f === "notice") {
    const w = Math.min(330, Math.ceil(measure($("notice").textContent)) + 40);
    size = [w, 36];
  } else if (f === "idle") {
    size = hover ? SIZES.idleHover : SIZES.idle;
    if (state && !state.settings.showPill && !hover) size = [72, 8];
  } else if (f === "listening") {
    const free = state.dictation === "handsfree";
    // On macOS a click would pull focus away from the app being typed into.
    const buttons = free && !mac;
    $("cancel").hidden = !buttons;
    $("done").hidden = !buttons;
    $("tail").hidden = !(free && mac);
    $("tail").innerHTML = free && mac ? `Tap ${keys(state.shortcutLabel)}` : "";
    pill.classList.toggle("with-buttons", buttons);
    size = buttons ? SIZES.handsfree : free && mac ? [224, 36] : SIZES.listening;
  } else {
    size = SIZES[f];
  }
  pill.style.setProperty("--w", `${size[0]}px`);
  pill.style.setProperty("--h", `${size[1]}px`);
  // The hover zone is a little larger than the idle line so it's easy to find.
  const area = f === "idle" && !hover ? [96, 24] : size;
  api.voice.hover?.(area[0], area[1]);
}

function measure(text) {
  const m = $("measure");
  m.textContent = text;
  return m.getBoundingClientRect().width;
}

function setState(s) {
  const prevMeeting = state?.meeting?.startedMs;
  state = s;
  root.dataset.tint = s.tint || "paper";
  $("hint").innerHTML = s.ready
    ? `Hold ${keys(s.shortcutLabel)} to talk`
    : "Set up voice in Pinion";
  if (s.meeting && s.meeting.startedMs !== prevMeeting) startClock(s.meeting.startedMs);
  if (!s.meeting) stopClock();
  render();
}

function startClock(started) {
  stopClock();
  const tick = () => {
    const secs = Math.max(0, Math.floor((Date.now() - started) / 1000));
    const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60), s = secs % 60;
    $("clock").textContent = h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
  };
  tick();
  clockTimer = setInterval(tick, 1000);
}
function stopClock() {
  clearInterval(clockTimer);
  clockTimer = null;
}

function showNotice(text) {
  notice = text;
  $("notice").textContent = text;
  clearTimeout(noticeTimer);
  render();
  noticeTimer = setTimeout(() => {
    notice = null;
    render();
  }, 2800);
}

const scale = (l) => Math.max(0.12, Math.min(1, Math.sqrt(l || 0) * 2.6));

listen("pill-level", (e) => {
  const [dict, meet] = e.payload;
  if (dict != null) {
    levels.shift();
    levels.push(scale(dict));
    const spans = $("bars").children;
    for (let i = 0; i < BARS; i++) spans[i].style.transform = `scaleY(${levels[i]})`;
  }
  if (meet != null) {
    const spans = $("mbars").children;
    const v = scale(meet);
    for (let i = 0; i < MBARS; i++) {
      const shape = [0.55, 0.85, 1, 0.8, 0.5][i];
      spans[i].style.transform = `scaleY(${Math.max(0.12, v * shape)})`;
    }
  }
});

listen("voice-state", (e) => setState(e.payload));
listen("pill-notice", (e) => showNotice(e.payload));
listen("pill-hover", (e) => {
  hover = !!e.payload;
  render();
});
listen("ui-settings", (e) => {
  const tint = e.payload?.theme?.tint;
  if (tint) root.dataset.tint = tint;
});

$("meet").onclick = () => api.voice.meeting().catch((err) => showNotice(String(err)));
$("mstop").onclick = () => api.voice.meeting().catch(() => {});
$("done").onclick = () => api.voice.toggle();
$("cancel").onclick = () => api.voice.cancel();

api.voice.state().then(setState).catch(() => {});
