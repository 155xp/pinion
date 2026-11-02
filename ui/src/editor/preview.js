// Live preview: markdown symbols step aside when the cursor isn't on them.
import { Decoration, ViewPlugin, WidgetType, EditorView } from "@codemirror/view";
import { StateEffect } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import { parseWiki, isImage } from "../links.js";
import { icon } from "../icons.js";

export const refreshPreview = StateEffect.define();

const hide = Decoration.replace({});
const lineDeco = (cls) => Decoration.line({ class: cls });

class Bullet extends WidgetType {
  eq() { return true; }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-bullet";
    s.textContent = "•";
    return s;
  }
}

// Text widths in the editor's current font, for hanging list indents.
let fontKey = "";
let canvas = null;
function measure(text, view) {
  const cs = getComputedStyle(view.contentDOM);
  const key = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  if (!canvas) canvas = document.createElement("canvas").getContext("2d");
  if (key !== fontKey) { canvas.font = key; fontKey = key; }
  return canvas.measureText(text).width;
}
function leadingWidth(ws, sw) {
  let n = 0;
  for (const ch of ws) n += ch === "\t" ? 4 : 1;
  return n * sw;
}
const bullet = Decoration.replace({ widget: new Bullet() });

class Check extends WidgetType {
  constructor(done) { super(); this.done = done; }
  eq(o) { return o.done === this.done; }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-check" + (this.done ? " done" : "");
    s.setAttribute("role", "checkbox");
    s.setAttribute("aria-checked", String(this.done));
    s.innerHTML = icon("check", 12);
    return s;
  }
  ignoreEvent() { return false; }
}

class Rule extends WidgetType {
  eq() { return true; }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-hr";
    return s;
  }
}
const rule = Decoration.replace({ widget: new Rule() });

class Embed extends WidgetType {
  constructor(src, alt, width) { super(); this.src = src; this.alt = alt; this.width = width; }
  eq(o) { return o.src === this.src && o.width === this.width && o.alt === this.alt; }
  toDOM() {
    if (!this.src) {
      const s = document.createElement("span");
      s.className = "md-embed missing";
      s.textContent = `Can't find ${this.alt}`;
      return s;
    }
    const img = document.createElement("img");
    img.className = "md-embed";
    img.src = this.src;
    img.alt = this.alt || "";
    img.draggable = false;
    if (this.width) img.style.width = `${this.width}px`;
    return img;
  }
  ignoreEvent() { return false; }
}

function buildDecorations(view, ctx) {
  const { state } = view;
  const doc = state.doc;
  const ranges = state.selection.ranges;
  const decos = [];
  const add = (from, to, d) => { if (from <= to) decos.push(d.range(from, to)); };

  const activeLines = new Set();
  for (const r of ranges) {
    const a = doc.lineAt(r.from).number, b = doc.lineAt(r.to).number;
    for (let n = a; n <= Math.min(b, a + 2000); n++) activeLines.add(n);
  }
  const lineActive = (pos) => activeLines.has(doc.lineAt(pos).number);
  const touches = (from, to) => ranges.some((r) => r.from <= to && r.to >= from);
  const from = ctx.path();
  const sw = measure("    ", view) / 4;

  for (const vr of view.visibleRanges) {
    syntaxTree(state).iterate({
      from: vr.from,
      to: vr.to,
      enter(node) {
        const name = node.name;
        switch (name) {
          case "ATXHeading1": case "ATXHeading2": case "ATXHeading3":
          case "ATXHeading4": case "ATXHeading5": case "ATXHeading6": {
            const level = name.slice(-1);
            add(doc.lineAt(node.from).from, doc.lineAt(node.from).from, lineDeco(`h${level}`));
            if (!touches(node.from, node.to)) {
              for (let c = node.node.firstChild; c; c = c.nextSibling) {
                if (c.name !== "HeaderMark") continue;
                if (c.from === node.from) {
                  const end = doc.sliceString(c.to, c.to + 1) === " " ? c.to + 1 : c.to;
                  add(c.from, end, hide);
                } else {
                  add(c.from - (doc.sliceString(c.from - 1, c.from) === " " ? 1 : 0), c.to, hide);
                }
              }
            }
            break;
          }
          case "SetextHeading1": case "SetextHeading2": {
            const lvl = name.slice(-1);
            const first = doc.lineAt(node.from).number;
            const markLine = doc.lineAt(node.to).number;
            for (let n = first; n < markLine; n++) add(doc.line(n).from, doc.line(n).from, lineDeco(`h${lvl}`));
            break;
          }
          case "Emphasis": case "StrongEmphasis": case "Strikethrough": case "Highlight": {
            if (!touches(node.from, node.to)) {
              for (let c = node.node.firstChild; c; c = c.nextSibling) {
                if (c.name === "EmphasisMark" || c.name === "StrikethroughMark" || c.name === "HighlightMark") {
                  add(c.from, c.to, hide);
                }
              }
            }
            break;
          }
          case "InlineCode": {
            add(node.from, node.to, Decoration.mark({ class: "md-code" }));
            if (!touches(node.from, node.to)) {
              for (let c = node.node.firstChild; c; c = c.nextSibling) {
                if (c.name === "CodeMark") add(c.from, c.to, hide);
              }
            }
            return false;
          }
          case "Link": {
            const marks = [];
            let url = null;
            for (let c = node.node.firstChild; c; c = c.nextSibling) {
              if (c.name === "LinkMark") marks.push(c);
              if (c.name === "URL") url = doc.sliceString(c.from, c.to);
            }
            if (marks.length < 2) break;
            const textFrom = marks[0].to, textTo = marks[1].from;
            const isNote = url && !/^[a-z]+:/i.test(url) && /\.md(#.*)?$/i.test(url);
            const attrs = isNote
              ? { "data-md": url }
              : { "data-href": url || "" };
            const resolved = isNote ? ctx.resolveMarkdown(url) : true;
            if (textTo > textFrom) {
              add(textFrom, textTo, Decoration.mark({
                class: "md-link" + (resolved ? "" : " unresolved"),
                attributes: attrs,
              }));
            }
            if (!touches(node.from, node.to) && url) {
              add(marks[0].from, marks[0].to, hide);
              add(marks[1].from, node.to, hide);
            }
            return false;
          }
          case "Image": {
            let url = null, alt = "";
            const marks = [];
            for (let c = node.node.firstChild; c; c = c.nextSibling) {
              if (c.name === "URL") url = doc.sliceString(c.from, c.to);
              if (c.name === "LinkMark") marks.push(c);
            }
            if (marks.length >= 2) alt = doc.sliceString(marks[0].to, marks[1].from);
            if (url == null) break;
            const widget = new Embed(ctx.imageUrl(url), alt || url, null);
            if (!touches(node.from, node.to)) {
              add(node.from, node.to, Decoration.replace({ widget }));
            } else {
              add(node.to, node.to, Decoration.widget({ widget, side: 1 }));
            }
            return false;
          }
          case "WikiLink": case "WikiEmbed": {
            const embed = name === "WikiEmbed";
            const openEnd = node.from + (embed ? 3 : 2);
            const closeStart = node.to - 2;
            const inner = doc.sliceString(openEnd, closeStart);
            const w = parseWiki(inner);
            const active = touches(node.from, node.to);
            if (embed && isImage(w.target)) {
              const width = w.alias && /^\d+(x\d+)?$/.test(w.alias) ? parseInt(w.alias, 10) : null;
              const widget = new Embed(ctx.fileUrl(w.target), w.target, width);
              if (!active) add(node.from, node.to, Decoration.replace({ widget }));
              else add(node.to, node.to, Decoration.widget({ widget, side: 1 }));
              return false;
            }
            const resolved = w.target === "" ? from : ctx.resolve(w.target);
            const attrs = { "data-wiki": w.target, "data-heading": w.heading || "" };
            const cls = "md-link" + (resolved ? "" : " unresolved");
            if (active) {
              add(openEnd, closeStart, Decoration.mark({ class: cls, attributes: attrs }));
            } else {
              const shownFrom = w.alias ? openEnd + w.aliasOffset : openEnd;
              add(node.from, shownFrom, hide);
              add(shownFrom, closeStart, Decoration.mark({ class: cls, attributes: attrs }));
              add(closeStart, node.to, hide);
            }
            return false;
          }
          case "HashTag":
            add(node.from, node.to, Decoration.mark({ class: "md-tag" }));
            return false;
          case "Blockquote": {
            const a = doc.lineAt(node.from).number, b = doc.lineAt(node.to).number;
            for (let n = a; n <= b; n++) add(doc.line(n).from, doc.line(n).from, lineDeco("quote"));
            break;
          }
          case "QuoteMark": {
            if (!lineActive(node.from)) {
              const end = doc.sliceString(node.to, node.to + 1) === " " ? node.to + 1 : node.to;
              add(node.from, end, hide);
            }
            break;
          }
          case "ListMark": {
            const parent = node.node.parent;
            const listType = parent && parent.parent ? parent.parent.name : "";
            const task = parent && parent.getChild("Task");
            const line = doc.lineAt(node.from);
            const lead = leadingWidth(doc.sliceString(line.from, node.from), sw);
            const spaced = doc.sliceString(node.to, node.to + 1) === " ";
            const end = spaced ? node.to + 1 : node.to;
            let hang;
            if (task) {
              const marker = task.getChild("TaskMarker");
              if (!(marker && touches(node.from, marker.to))) add(node.from, end, hide);
              hang = `${lead + sw}px + 1.44em`;
            } else if (listType === "BulletList") {
              if (!touches(node.from, end)) add(node.from, end, bullet);
              hang = `${lead}px + 1.3em`;
            } else {
              add(node.from, node.to, Decoration.mark({ class: "md-ordinal" }));
              hang = `${lead + measure(doc.sliceString(node.from, end), view)}px`;
            }
            add(line.from, line.from, Decoration.line({
              attributes: { style: `padding-left: calc(${hang}); text-indent: calc(-1 * (${hang}))` },
            }));
            break;
          }
          case "TaskMarker": {
            const text = doc.sliceString(node.from, node.to);
            const done = /\[[xX]\]/.test(text);
            const line = doc.lineAt(node.from);
            if (done) add(line.from, line.from, lineDeco("task-done"));
            if (node.to < line.to) add(node.to, line.to, Decoration.mark({ class: "md-task-text" }));
            if (!touches(node.from, node.to)) add(node.from, node.to, Decoration.replace({ widget: new Check(done) }));
            break;
          }
          case "HorizontalRule": {
            if (!lineActive(node.from)) add(node.from, node.to, rule);
            break;
          }
          case "FencedCode": case "CodeBlock": {
            const a = doc.lineAt(node.from).number, b = doc.lineAt(node.to).number;
            for (let n = a; n <= b; n++) {
              let cls = "codeblock";
              if (n === a) cls += " first";
              if (n === b) cls += " last";
              if (name === "FencedCode" && (n === a || (n === b && /^\s*(```|~~~)/.test(doc.line(n).text)))) cls += " fence";
              add(doc.line(n).from, doc.line(n).from, lineDeco(cls));
            }
            return false;
          }
          case "Frontmatter": {
            const a = doc.lineAt(node.from).number, b = doc.lineAt(node.to).number;
            for (let n = a; n <= b; n++) {
              const fence = /^(---|\.\.\.)\s*$/.test(doc.line(n).text) && (n === a || n === b);
              add(doc.line(n).from, doc.line(n).from, lineDeco("frontmatter" + (fence ? " fence" : "")));
            }
            return false;
          }
          case "Table": {
            const a = doc.lineAt(node.from).number, b = doc.lineAt(node.to).number;
            for (let n = a; n <= b; n++) add(doc.line(n).from, doc.line(n).from, lineDeco("table"));
            break;
          }
        }
      },
    });
  }
  return Decoration.set(decos, true);
}

export function livePreview(ctx) {
  const plugin = ViewPlugin.fromClass(
    class {
      constructor(view) { this.decorations = buildDecorations(view, ctx); }
      update(u) {
        const forced = u.transactions.some((t) => t.effects.some((e) => e.is(refreshPreview)));
        if (u.docChanged || u.viewportChanged || u.selectionSet || forced || syntaxTree(u.startState) !== syntaxTree(u.state)) {
          this.decorations = buildDecorations(u.view, ctx);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

  const clicks = EditorView.domEventHandlers({
    mousedown(e, view) {
      if (e.button !== 0) return false;
      const check = e.target.closest(".md-check");
      if (check) {
        const pos = view.posAtDOM(check);
        const ch = view.state.sliceDoc(pos + 1, pos + 2);
        view.dispatch({ changes: { from: pos + 1, to: pos + 2, insert: ch === " " ? "x" : " " } });
        e.preventDefault();
        return true;
      }
      const link = e.target.closest(".md-link");
      if (!link) return false;
      const pos = view.posAtDOM(link);
      const active = view.state.selection.ranges.some((r) => r.from <= pos + link.textContent.length && r.to >= pos - 2);
      const withMod = e.metaKey || e.ctrlKey;
      if (active && !withMod) return false;
      e.preventDefault();
      const d = link.dataset;
      if (d.wiki !== undefined) ctx.openWiki(d.wiki, d.heading || null, withMod);
      else if (d.md) ctx.openMarkdown(d.md, withMod);
      else if (d.href) ctx.openUrl(d.href);
      return true;
    },
  });

  return [plugin, clicks];
}
