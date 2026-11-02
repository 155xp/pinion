import { EditorView, keymap, drawSelection, dropCursor, placeholder } from "@codemirror/view";
import { EditorState, EditorSelection, Compartment } from "@codemirror/state";
import { history, defaultKeymap, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdownLanguage, markdownKeymap } from "@codemirror/lang-markdown";
import { syntaxHighlighting, HighlightStyle, indentUnit, Language, LanguageSupport } from "@codemirror/language";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { search, searchKeymap } from "@codemirror/search";
import { tags } from "@lezer/highlight";
import { extensions as mdExtensions, mdTags } from "./markdown.js";
import { livePreview, refreshPreview } from "./preview.js";

// Built from the parser directly so the HTML, CSS and JavaScript grammars
// that markdown() pulls in for embedded code stay out of the bundle.
const markdownSupport = new LanguageSupport(
  new Language(markdownLanguage.data, markdownLanguage.parser.configure(mdExtensions), [], "markdown"),
);

const style = HighlightStyle.define([
  { tag: tags.emphasis, class: "md-em" },
  { tag: tags.strong, class: "md-strong" },
  { tag: tags.strikethrough, class: "md-strike" },
  { tag: mdTags.highlight, class: "md-highlight" },
  { tag: tags.processingInstruction, class: "md-mark" },
  { tag: tags.url, class: "md-url" },
]);

// Wraps the selection in a marker, or unwraps it if it's already wrapped.
function toggleWrap(mark) {
  return (view) => {
    const { state } = view;
    const tr = state.changeByRange((r) => {
      const before = state.sliceDoc(r.from - mark.length, r.from);
      const after = state.sliceDoc(r.to, r.to + mark.length);
      if (before === mark && after === mark) {
        return {
          changes: [{ from: r.from - mark.length, to: r.from }, { from: r.to, to: r.to + mark.length }],
          range: EditorSelection.range(r.from - mark.length, r.to - mark.length),
        };
      }
      return {
        changes: [{ from: r.from, insert: mark }, { from: r.to, insert: mark }],
        range: EditorSelection.range(r.from + mark.length, r.to + mark.length),
      };
    });
    view.dispatch(state.update(tr, { userEvent: "input" }));
    return true;
  };
}

// Completes [[links]] from the vault's notes.
function wikiCompletion(ctx) {
  return (cx) => {
    const m = cx.matchBefore(/\[\[[^[\]|#\n]*$/);
    if (!m) return null;
    const from = m.from + 2;
    return {
      from,
      validFor: /^[^[\]|#\n]*$/,
      options: ctx.completions().map((c) => ({
        label: c.text,
        detail: c.detail || undefined,
        apply(view, _c, a, b) {
          const closed = view.state.sliceDoc(b, b + 2) === "]]";
          view.dispatch({
            changes: { from: a, to: b, insert: closed ? c.text : c.text + "]]" },
            selection: { anchor: a + c.text.length + 2 },
            userEvent: "input.complete",
          });
        },
      })),
    };
  };
}

export function createEditor(parent, ctx) {
  const states = new Map();
  let currentPath = null;
  const readOnly = new Compartment();

  const pasteFiles = async (view, files, pos) => {
    const images = [...files].filter((f) => f.type.startsWith("image/") || /\.(png|jpe?g|gif|webp|svg|avif|pdf)$/i.test(f.name));
    if (!images.length) return false;
    for (const f of images) {
      const name = await ctx.saveFile(f);
      if (!name) continue;
      const text = `![[${name}]]`;
      const at = pos ?? view.state.selection.main.head;
      view.dispatch({ changes: { from: at, insert: text }, selection: { anchor: at + text.length }, userEvent: "input.paste" });
      pos = at + text.length;
    }
    return true;
  };

  const extensions = [
    readOnly.of(EditorState.readOnly.of(false)),
    history(),
    drawSelection({ cursorBlinkRate: 1100 }),
    dropCursor(),
    EditorView.lineWrapping,
    EditorState.allowMultipleSelections.of(true),
    indentUnit.of("    "),
    EditorState.tabSize.of(4),
    markdownSupport,
    syntaxHighlighting(style),
    closeBrackets(),
    autocompletion({ override: [wikiCompletion(ctx)], icons: false, maxRenderedOptions: 40, closeOnBlur: true }),
    search({ top: true }),
    placeholder("Start writing…"),
    EditorView.contentAttributes.of({ spellcheck: "true", autocorrect: "on", autocapitalize: "sentences", "aria-label": "Note" }),
    keymap.of([
      { key: "Mod-b", run: toggleWrap("**") },
      { key: "Mod-i", run: toggleWrap("*") },
      { key: "Mod-Shift-x", run: toggleWrap("~~") },
      { key: "Mod-Shift-h", run: toggleWrap("==") },
      { key: "Mod-e", run: toggleWrap("`") },
      { key: "Escape", run: (v) => { v.contentDOM.blur(); return true; } },
      ...closeBracketsKeymap,
      ...completionKeymap,
      ...markdownKeymap,
      ...searchKeymap,
      ...historyKeymap,
      indentWithTab,
      ...defaultKeymap,
    ]),
    livePreview(ctx),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) ctx.onChange(u);
      if (u.selectionSet && u.view.hasFocus) ctx.onCaret?.();
    }),
    EditorView.domEventHandlers({
      paste(e, view) {
        const files = e.clipboardData?.files;
        if (files && files.length) {
          e.preventDefault();
          pasteFiles(view, files);
          return true;
        }
        return false;
      },
      drop(e, view) {
        const files = e.dataTransfer?.files;
        if (files && files.length) {
          e.preventDefault();
          const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
          pasteFiles(view, files, pos);
          return true;
        }
        return false;
      },
    }),
  ];

  const make = (doc, selection) => EditorState.create({ doc, extensions, selection });
  const view = new EditorView({ parent, state: make("") });

  return {
    view,
    path: () => currentPath,
    load(path, text, { focus = false, at = null } = {}) {
      if (currentPath) states.set(currentPath, { state: view.state, scroll: ctx.scrollTop() });
      currentPath = path;
      const cached = states.get(path);
      let restoredScroll = null;
      if (cached && cached.state.doc.toString() === text) {
        view.setState(cached.state);
        restoredScroll = cached.scroll;
      } else {
        view.setState(make(text));
      }
      if (states.size > 24) states.delete(states.keys().next().value);
      if (at != null) {
        const pos = Math.min(at, view.state.doc.length);
        view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
      } else {
        ctx.scrollTo(restoredScroll ?? 0);
      }
      if (focus) view.focus();
    },
    // Replaces the text after an outside change, keeping the cursor where it was.
    replace(text) {
      const sel = view.state.selection.main;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
        selection: { anchor: Math.min(sel.anchor, text.length) },
        annotations: [],
      });
    },
    forget(path) { states.delete(path); },
    // Is the end of the note on screen? Used to follow a live transcript.
    endVisible(scroller) {
      const c = view.coordsAtPos(view.state.doc.length);
      if (!c) return false;
      const r = scroller.getBoundingClientRect();
      return c.bottom <= r.bottom + 4;
    },
    revealEnd() {
      view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.length, { y: "nearest", yMargin: 96 }) });
    },
    setReadOnly(on) {
      if (view.state.readOnly === on) return;
      view.dispatch({ effects: readOnly.reconfigure(EditorState.readOnly.of(on)) });
    },
    rename(from, to) {
      if (states.has(from)) { states.set(to, states.get(from)); states.delete(from); }
      if (currentPath === from) currentPath = to;
    },
    text: () => view.state.doc.toString(),
    focus: () => view.focus(),
    hasFocus: () => view.hasFocus,
    refresh: () => view.dispatch({ effects: refreshPreview.of(null) }),
    lineStart(line) {
      const n = Math.max(1, Math.min(line + 1, view.state.doc.lines));
      return view.state.doc.line(n).from;
    },
    find(heading) {
      const doc = view.state.doc;
      const want = heading.toLowerCase().trim();
      if (want.startsWith("^")) {
        for (let i = 1; i <= doc.lines; i++) if (doc.line(i).text.trimEnd().endsWith(" " + want)) return doc.line(i).from;
        return null;
      }
      for (let i = 1; i <= doc.lines; i++) {
        const m = /^#{1,6}\s+(.*?)\s*#*\s*$/.exec(doc.line(i).text);
        if (m && m[1].toLowerCase() === want) return doc.line(i).from;
      }
      return null;
    },
    goto(pos, focus = true) {
      view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "start", yMargin: 80 }) });
      if (focus) view.focus();
    },
  };
}
