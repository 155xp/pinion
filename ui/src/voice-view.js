// The Voice page: set up the model, see what you've said, and tune how it listens.
import { api, listen } from "./api.js";
import { h, basename, dirname, timeAgo } from "./dom.js";
import { icon } from "./icons.js";
import { openMenu, toast, confirmDialog } from "./overlays.js";

const MB = (b) => `${Math.round(b / 1e6)} MB`;
const keysHtml = (label) => label.split(" ").map((k) => `<span class="key">${k}</span>`).join(" ");

export class VoiceView {
  constructor(el, app) {
    this.el = el;
    this.app = app;
    this.state = null;
    this.history = [];
    this.showAll = false;
    this.meetings = [];
    this.devices = null;
    this.recording = false;
    this.clock = null;

    listen("voice-state", (e) => {
      const was = this.state?.meeting?.path;
      this.state = e.payload;
      this.updateRow();
      app.onState?.(this.state, was);
      if (this.visible()) this.render();
    });
    listen("voice-history", (e) => {
      this.history.unshift(e.payload);
      if (this.visible()) this.render();
    });
    listen("voice-download", (e) => {
      if (!this.state) return;
      this.state.downloading = e.payload;
      const bar = this.el.querySelector(".progress span");
      const label = this.el.querySelector(".progress-label");
      if (bar && label) {
        const [done, total] = e.payload;
        bar.style.transform = `scaleX(${done / total})`;
        label.textContent = `Downloading… ${MB(done)} of ${MB(total)}`;
      } else if (this.visible()) this.render();
    });
    listen("voice-error", (e) => toast(String(e.payload), { error: true }));
    api.voice.state().then((s) => {
      this.state = s;
      this.updateRow();
      app.onState?.(s, null);
    }).catch(() => {});
  }

  visible() {
    return !this.el.hidden;
  }

  updateRow() {
    const row = this.app.row;
    let dot = row.querySelector(".dot");
    const live = !!this.state?.meeting;
    if (live && !dot) row.append(h("span.dot", { title: "Recording a meeting" }));
    if (!live && dot) dot.remove();
  }

  async open() {
    this.app.show();
  }

  async show() {
    try {
      [this.state, this.history] = await Promise.all([api.voice.state(), api.voice.history()]);
    } catch (e) {
      toast(String(e), { error: true });
    }
    this.render();
    this.el.scrollIntoView?.({ block: "start" });
  }

  refreshMeetings(entries) {
    const folder = (this.state?.settings?.meetingFolder || "Meetings").toLowerCase();
    this.meetings = entries
      .filter((e) => !e.dir && /\.md$/i.test(e.path) && dirname(e.path).toLowerCase() === folder)
      .sort((a, b) => b.mtime - a.mtime);
    if (this.visible() && this.state) this.render();
  }

  render() {
    const s = this.state;
    if (!s) return;
    clearInterval(this.clock);
    const col = h("div.voice-column");
    const meetingBtn = h(`button.btn${s.meeting ? ".danger-soft" : ""}.has-icon`, {
      html: `${icon(s.meeting ? "stop" : "meeting")}<span>${s.meeting ? "Stop recording" : "Record meeting"}</span>`,
      disabled: !s.ready,
      onclick: () => api.voice.meeting().catch((e) => toast(String(e), { error: true })),
    });
    col.append(
      h("header.voice-head", {},
        h("div", {},
          h("h1", {}, "Voice"),
          h("p", { html: s.ready
            ? `Hold ${keysHtml(s.shortcutLabel)} anywhere to talk, and let go to type it. Tap it once to keep listening.`
            : "Talk instead of typing, in any app. It all happens on this computer." }),
        ),
        meetingBtn,
      ),
    );

    if (!s.ready) col.append(this.setupCard(s));
    if (s.meeting) col.append(this.meetingCard(s));
    if (s.shortcutError) col.append(h("div.voice-alert", {}, s.shortcutError));
    if (s.accessibility === false && s.ready) {
      col.append(h("div.voice-alert", {},
        h("span", {}, "Allow Pinion in Accessibility so it can type for you. Until then, your words are copied."),
        h("button.btn.compact", { onclick: async () => { await api.voice.permissions(true); this.show(); } }, "Allow"),
      ));
    }

    col.append(this.historySection());
    if (this.meetings.length) col.append(this.meetingsSection());
    col.append(this.settingsSection(s));
    this.el.textContent = "";
    this.el.append(col);
  }

  setupCard(s) {
    const dl = s.downloading;
    const card = h("section.card.voice-setup.surface-raised", {},
      h("h2", {}, "Download the speech model"),
      h("p", {}, `Pinion listens with ${s.model}, a speech model from NVIDIA that runs on this computer, so nothing you say leaves it. It's a one-time ${MB(s.modelSize)} download.`),
    );
    if (dl) {
      const [done, total] = dl;
      card.append(
        h("div.progress", {}, h("span", { style: { transform: `scaleX(${done / total})` } })),
        h("div.progress-row", {},
          h("span.progress-label", {}, `Downloading… ${MB(done)} of ${MB(total)}`),
          h("button.btn.compact", { onclick: () => api.voice.cancelDownload() }, "Cancel"),
        ),
      );
    } else {
      card.append(h("div.card-actions", {},
        h("button.btn.primary.has-icon", { html: `${icon("download")}<span>Download ${MB(s.modelSize)}</span>`, onclick: () => api.voice.download() }),
      ));
    }
    return card;
  }

  meetingCard(s) {
    const clock = h("span.meeting-clock", {}, "0:00");
    const tick = () => {
      const secs = Math.max(0, Math.floor((Date.now() - s.meeting.startedMs) / 1000));
      const m = Math.floor(secs / 60), sec = secs % 60;
      clock.textContent = `${m}:${String(sec).padStart(2, "0")}`;
    };
    tick();
    this.clock = setInterval(tick, 1000);
    return h("section.card.meeting-card.surface-raised", {},
      h("span.live"),
      h("div.grow", {},
        h("div", {}, "Recording ", clock),
        h("div.sub", {}, s.meeting.systemAudio ? "Your microphone and the other side of the call." : "Your microphone. Wear headphones on calls so it doesn't hear the speakers."),
      ),
      h("button.btn.compact", { onclick: () => this.app.open(s.meeting.path) }, "Open note"),
    );
  }

  historySection() {
    const list = this.showAll ? this.history : this.history.slice(0, 6);
    const sec = h("section.voice-section", {},
      h("div.section-head", {},
        h("h2", {}, "History"),
        this.history.length
          ? h("button.ghost", {
              onclick: async () => {
                const ok = await confirmDialog({ title: "Clear your voice history?", body: "Everything you've dictated is removed from this list. Text you already typed into apps stays where it is.", confirm: "Clear history", danger: true });
                if (!ok) return;
                await api.voice.clearHistory();
                this.history = [];
                this.render();
              },
            }, "Clear")
          : null,
      ),
    );
    if (!this.history.length) {
      sec.append(h("div.voice-empty", {},
        h("p", {}, "Nothing yet."),
        h("p", {}, this.state.ready ? "What you dictate shows up here, newest first." : "Once voice is set up, what you dictate shows up here."),
      ));
      return sec;
    }
    const rows = h("div.history");
    for (const e of list) {
      const copy = h("button.icon-button", { "aria-label": "Copy", title: "Copy", html: icon("copy") });
      copy.onclick = () => {
        copy.innerHTML = icon("check");
        const restore = () => setTimeout(() => { copy.innerHTML = icon("copy"); }, 1000);
        navigator.clipboard?.writeText(e.text).then(restore, () => api.voice.copy(e.text).then(restore));
      };
      const del = h("button.icon-button", { "aria-label": "Delete", title: "Delete", html: icon("trash") });
      del.onclick = async () => {
        await api.voice.deleteHistory(e.id);
        this.history = this.history.filter((x) => x.id !== e.id);
        this.render();
      };
      rows.append(h("div.history-row", {},
        h("div.grow", {},
          h("div.history-text", {}, e.text),
          h("div.meta", {}, `${timeAgo(e.at)} · ${Math.max(1, Math.round(e.ms / 1000))} s`),
        ),
        h("div.row-actions", {}, copy, del),
      ));
    }
    sec.append(rows);
    if (this.history.length > 6 && !this.showAll) {
      sec.append(h("button.ghost.more", { onclick: () => { this.showAll = true; this.render(); } }, `Show all ${this.history.length}`));
    }
    return sec;
  }

  meetingsSection() {
    const sec = h("section.voice-section", {}, h("div.section-head", {}, h("h2", {}, "Meetings")));
    const rows = h("div.meetings");
    for (const m of this.meetings.slice(0, 6)) {
      const b = h("button.meeting-row", {}, h("span.grow", {}, basename(m.path)), h("span.meta", {}, timeAgo(m.mtime)));
      b.onclick = () => this.app.open(m.path);
      rows.append(b);
    }
    sec.append(rows);
    return sec;
  }

  settingsSection(s) {
    const set = async (patch) => {
      this.state = await api.voice.settings(patch);
      this.render();
    };
    const sw = (on, onchange, label) => {
      const b = h("button.switch.instant", { role: "switch", "aria-checked": String(on), "aria-label": label });
      requestAnimationFrame(() => b.classList.remove("instant"));
      b.onclick = () => {
        const v = b.getAttribute("aria-checked") !== "true";
        b.setAttribute("aria-checked", String(v));
        onchange(v);
      };
      return b;
    };
    const row = (title, desc, control) =>
      h("div.setting-row", {}, h("div.grow", {}, h("div", {}, title), desc ? h("div.sub", {}, desc) : null), control);

    const shortcut = h("button.select", { html: `<span>${s.shortcutLabel}</span>` });
    shortcut.onclick = () => this.recordShortcut(shortcut, set);

    const micName = s.settings.microphone
      ? (this.devices?.find((d) => d.id === s.settings.microphone)?.name || "Chosen microphone")
      : "System default";
    const mic = h("button.select", { html: `<span>${micName}</span>${icon("chevronDown", 14)}`, "aria-haspopup": "menu" });
    mic.onclick = async () => {
      try {
        this.devices = await api.voice.devices();
      } catch {
        this.devices = [];
      }
      openMenu(mic, [
        { label: "System default", chosen: !s.settings.microphone, action: () => set({ microphone: null }) },
        ...this.devices.map((d) => ({ label: d.name, chosen: d.id === s.settings.microphone, action: () => set({ microphone: d.id }) })),
      ], { align: "end" });
    };

    const card = h("div.card.settings-card.surface-raised", {},
      row("Shortcut", "Hold to talk, tap to keep listening. Esc cancels.", shortcut),
      row("Microphone", null, mic),
      row("Record the other side of calls", "In meetings, also listen to what your computer plays.", sw(s.settings.systemAudio, (v) => set({ systemAudio: v }), "Record the other side of calls")),
      row("Leave out filler words", "Drops um and uh.", sw(s.settings.removeFillers, (v) => set({ removeFillers: v }), "Leave out filler words")),
      row("Show the pill while idle", "A thin line at the bottom of the screen. Hover it to record a meeting.", sw(s.settings.showPill, (v) => set({ showPill: v }), "Show the pill while idle")),
      row("Open at login", "So the shortcut always works.", sw(s.settings.launchAtLogin, (v) => set({ launchAtLogin: v }), "Open at login")),
    );
    if (s.ready) {
      const remove = h("button.ghost", {
        onclick: async () => {
          const ok = await confirmDialog({ title: "Remove the speech model?", body: `This frees ${MB(s.modelSize)}. You can download it again any time.`, confirm: "Remove model", danger: true });
          if (ok) { await api.voice.deleteModel(); this.show(); }
        },
      }, "Remove");
      card.append(row("Speech model", `${s.model}, ${MB(s.modelSize)}, on this computer.`, remove));
    }
    return h("section.voice-section", {}, h("div.section-head", {}, h("h2", {}, "Settings")), card);
  }

  recordShortcut(button, set) {
    if (this.recording) return;
    this.recording = true;
    const label = button.querySelector("span");
    const old = label.textContent;
    label.textContent = "Press keys…";
    button.setAttribute("aria-pressed", "true");
    const done = () => {
      this.recording = false;
      document.removeEventListener("keydown", onKey, true);
      button.removeAttribute("aria-pressed");
    };
    const onKey = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") { label.textContent = old; done(); return; }
      if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
      const mods = [];
      if (e.ctrlKey) mods.push("Ctrl");
      if (e.altKey) mods.push("Alt");
      if (e.shiftKey) mods.push("Shift");
      if (e.metaKey) mods.push("Super");
      const fkey = /^F\d+$/.test(e.code);
      if (!mods.length && !fkey) { label.textContent = "Add a modifier key"; return; }
      done();
      set({ shortcut: [...mods, e.code].join("+") });
    };
    document.addEventListener("keydown", onKey, true);
  }
}
