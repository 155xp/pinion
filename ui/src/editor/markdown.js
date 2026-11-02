// Obsidian flavoured additions to the markdown parser.
import { Tag, tags } from "@lezer/highlight";

export const mdTags = {
  highlight: Tag.define(),
  wiki: Tag.define(),
  hashtag: Tag.define(),
  frontmatter: Tag.define(),
};

const CHAR = (s) => s.charCodeAt(0);
const BRACKET_OPEN = CHAR("["), BRACKET_CLOSE = CHAR("]"), BANG = CHAR("!"), NL = 10;

// [[Note]], [[Note#Heading|alias]] and ![[embeds]].
export const WikiLink = {
  defineNodes: [
    { name: "WikiLink", style: mdTags.wiki },
    { name: "WikiEmbed", style: mdTags.wiki },
    { name: "WikiMark", style: tags.processingInstruction },
  ],
  parseInline: [
    {
      name: "WikiLink",
      before: "Link",
      parse(cx, next, pos) {
        const start = pos;
        let open = pos;
        let embed = false;
        if (next === BANG && cx.char(pos + 1) === BRACKET_OPEN && cx.char(pos + 2) === BRACKET_OPEN) {
          embed = true;
          open = pos + 1;
        } else if (!(next === BRACKET_OPEN && cx.char(pos + 1) === BRACKET_OPEN)) {
          return -1;
        }
        let end = open + 2;
        for (; end < cx.end; end++) {
          const ch = cx.char(end);
          if (ch === BRACKET_CLOSE && cx.char(end + 1) === BRACKET_CLOSE) break;
          if (ch === NL || ch === BRACKET_OPEN || ch === BRACKET_CLOSE) return -1;
        }
        if (end >= cx.end || end === open + 2) return -1;
        return cx.addElement(
          cx.elt(embed ? "WikiEmbed" : "WikiLink", start, end + 2, [
            cx.elt("WikiMark", start, open + 2),
            cx.elt("WikiMark", end, end + 2),
          ]),
        );
      },
    },
  ],
};

// ==highlight==
const HighlightDelim = { resolve: "Highlight", mark: "HighlightMark" };
export const Highlight = {
  defineNodes: [
    { name: "Highlight", style: mdTags.highlight },
    { name: "HighlightMark", style: tags.processingInstruction },
  ],
  parseInline: [
    {
      name: "Highlight",
      parse(cx, next, pos) {
        if (next !== 61 || cx.char(pos + 1) !== 61 || cx.char(pos + 2) === 61) return -1;
        const before = cx.slice(pos - 1, pos);
        const after = cx.slice(pos + 2, pos + 3);
        const sBefore = /\s|^$/.test(before), sAfter = /\s|^$/.test(after);
        return cx.addDelimiter(HighlightDelim, pos, pos + 2, !sAfter, !sBefore);
      },
      after: "Emphasis",
    },
  ],
};

// #tags: a hash after a space or line start, at least one non-digit.
const TAG_CHAR = /[\p{L}\p{N}_\-/]/u;
export const HashTag = {
  defineNodes: [{ name: "HashTag", style: mdTags.hashtag }],
  parseInline: [
    {
      name: "HashTag",
      parse(cx, next, pos) {
        if (next !== 35) return -1;
        const prev = cx.slice(pos - 1, pos);
        if (prev && !/\s/.test(prev)) return -1;
        let end = pos + 1;
        let letter = false;
        while (end < cx.end) {
          const ch = cx.slice(end, end + 1);
          if (!TAG_CHAR.test(ch)) break;
          if (!/[0-9]/.test(ch)) letter = true;
          end++;
        }
        if (end === pos + 1 || !letter) return -1;
        return cx.addElement(cx.elt("HashTag", pos, end));
      },
      before: "Emphasis",
    },
  ],
};

// YAML frontmatter between --- fences at the very top of a note.
const YAMLISH = /^(\s*$|\s+\S|\s*-\s|\s*#|[^\s:][^:]*:(\s|$))/;
export const Frontmatter = {
  defineNodes: [
    { name: "Frontmatter", block: true, style: mdTags.frontmatter },
    { name: "FrontmatterMark", style: tags.processingInstruction },
  ],
  parseBlock: [
    {
      name: "Frontmatter",
      before: "HorizontalRule",
      parse(cx, line) {
        if (cx.lineStart !== 0 || line.text.trimEnd() !== "---") return false;
        const first = cx.peekLine();
        if (first.trimEnd() !== "---" && !YAMLISH.test(first)) return false;
        const start = cx.lineStart;
        const marks = [cx.elt("FrontmatterMark", start, start + 3)];
        let end = start + line.text.length;
        while (cx.nextLine()) {
          end = cx.lineStart + line.text.length;
          const t = line.text.trimEnd();
          if (t === "---" || t === "...") {
            marks.push(cx.elt("FrontmatterMark", cx.lineStart, cx.lineStart + t.length));
            cx.nextLine();
            break;
          }
          const peek = cx.peekLine();
          if (peek.trimEnd() !== "---" && peek.trimEnd() !== "..." && !YAMLISH.test(peek)) {
            cx.nextLine();
            break;
          }
        }
        cx.addElement(cx.elt("Frontmatter", start, end, marks));
        return true;
      },
    },
  ],
};

export const extensions = [Frontmatter, WikiLink, Highlight, HashTag];
