<img src="assets/icon.svg" width="72" height="72" alt="">

# Pinion

A small, quiet place to write. Notes are plain markdown files in a folder you choose, linked with `[[double brackets]]`, so Obsidian (or anything else) can open the same vault. Hide the sidebar and nothing is left but the page.

**pinion.ericomg.com**

## Two builds

| | What you get | Download |
|---|---|---|
| **Pinion** | The notes app | about 2.2 MB (Linux .deb) |
| **Pinion Voice** | Notes, plus voice typing into any app and meeting notes | about 3.5 MB, plus a one-time 416 MB speech model |

Both are the same app: installing one over the other switches between them, and your notes and settings stay put.

## Notes

- Live preview: markdown symbols step aside when your cursor leaves them.
- `[[Links]]`, `[[Note#Heading]]`, `[[Note|alias]]`, `![[image.png]]` embeds, `#tags`, `==highlights==`, tasks and frontmatter, all Obsidian-compatible.
- Following a link to a missing note creates it; renaming a note rewrites every link to it.
- Backlinks at the foot of each note, quick open and full-text search (`Ctrl/⌘ K`).
- Paste or drop images straight into a note.
- Six colours (Paper, Pure, Rose, Lilac, Sage, Sky), each in light and dark. Pure is true white or true black.
- `Ctrl/⌘ \` hides the sidebar. With it hidden, the window is just the page; the left edge brings it back.

## Voice

- Hold the shortcut (`⌥ Space` on macOS, `Ctrl Shift Space` elsewhere) to talk, let go to type it into whatever app has focus. Tap it instead to keep listening; tap again to finish, `Esc` to cancel.
- A small pill sits at the bottom of the screen and shows when it's listening. Hover it to record a meeting.
- Meetings record your microphone and the other side of the call (system audio), transcribe as they go, and land in `Meetings/` as a note, with you and them on separate lines.
- Everything runs on your computer with NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) through [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (4-bit, 416 MB, 25 European languages). On 40 LibriSpeech clips it scored 3.4% word error rate, transcribing 11 seconds of speech in about 0.3 s on a desktop CPU. The model loads when you start talking and is freed after a few idle minutes.
- History of everything you've dictated lives on the Voice page.

Platform notes:

- **macOS** asks for Microphone and Accessibility (to press ⌘V for you) the first time. System audio in meetings needs macOS 14.6 or later.
- **Windows** needs nothing extra.
- **Linux** works on X11. On Wayland, global shortcuts don't reach apps: bind `pinion --dictate` and `pinion --meeting` to keys in your desktop's settings instead. Typing into other apps uses `wtype` or `ydotool` when present.

## Build from source

You need Rust, Node 20+, CMake and a C++ compiler. On Linux also: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev libxdo-dev libssl-dev`.

```sh
git submodule update --init --depth 1   # whisper.cpp, for the voice build
npm ci
npx tauri dev                           # notes only
npx tauri dev --features voice          # with voice
npx tauri build                         # release build of Pinion
npx tauri build --features voice --config src-tauri/tauri.voice.conf.json
```

Tests: `cd src-tauri && cargo test --features voice`.

## Layout

```
ui/                 the app's interface: plain JS, CodeMirror 6, bundled by esbuild
  src/editor/       live preview, wiki links, markdown extensions
  src/pill.js       the voice pill
src-tauri/          the Rust side
  src/vault.rs      files, renames that rewrite links, search, backlinks
  src/links.rs      Obsidian's link resolution (mirrored in ui/src/links.js)
  src/voice/        audio capture, the speech engine, paste, meetings, history
crates/parakeet-sys static build of whisper.cpp's Parakeet, with a small C shim
scripts/themes.mjs  generates every colour theme from OKLCH recipes
web/                pinion.ericomg.com
```

## Releasing

Push a tag like `v0.1.0`. GitHub Actions builds both variants for macOS (Apple silicon and Intel), Windows and Linux, uploads them to the release with stable names (`Pinion-macos-arm64.dmg`, `Pinion-Voice-windows-x64-setup.exe`, ...), and publishes it once every build is in. The website reads sizes and links from the latest release.

The apps are not code-signed yet: macOS builds are ad-hoc signed, Windows builds unsigned.
