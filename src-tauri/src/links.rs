//! Obsidian-compatible link parsing and resolution.
//!
//! The same rules live in `ui/src/links.js`; keep the two in step.

/// A link found in a note. `start..end` covers the whole `[[...]]` (or the
/// `(...)` part of a markdown link); `target` is the path or name it points at.
#[derive(Debug, Clone)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub target: String,
    /// Byte range of `target` inside the note, so it can be rewritten.
    pub target_start: usize,
    pub target_end: usize,
    pub kind: LinkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Wiki,
    Markdown,
}

/// Finds `[[wiki]]` and `[text](relative.md)` links, skipping fenced and inline code.
pub fn find_links(text: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent < 4 {
            let tb = trimmed.as_bytes();
            let ch = tb.first().copied().unwrap_or(0);
            if ch == b'`' || ch == b'~' {
                let run = tb.iter().take_while(|&&c| c == ch).count();
                if run >= 3 {
                    match fence {
                        None => fence = Some((ch, run)),
                        Some((fc, fr)) if fc == ch && run >= fr && trimmed[run..].trim().is_empty() => {
                            fence = None
                        }
                        _ => {}
                    }
                    offset += line.len();
                    continue;
                }
            }
        }
        if fence.is_none() {
            scan_line(line, offset, &mut out);
        }
        offset += line.len();
    }
    out
}

fn scan_line(line: &str, base: usize, out: &mut Vec<Link>) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                // Skip an inline code span: a run of n backticks closed by the same run.
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                let mut j = i + run;
                let mut closed = None;
                while j < b.len() {
                    if b[j] == b'`' {
                        let r = b[j..].iter().take_while(|&&c| c == b'`').count();
                        if r == run {
                            closed = Some(j + r);
                            break;
                        }
                        j += r;
                    } else {
                        j += 1;
                    }
                }
                i = closed.unwrap_or(i + run);
            }
            b'\\' => i += 2,
            b'[' if b.get(i + 1) == Some(&b'[') => {
                let start = if i > 0 && b[i - 1] == b'!' { i - 1 } else { i };
                if let Some(close) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + close];
                    if !inner.is_empty() && !inner.contains('[') {
                        let name_len = inner.find(['|', '#']).unwrap_or(inner.len());
                        let raw = &inner[..name_len];
                        let lead = raw.len() - raw.trim_start().len();
                        let target = raw.trim();
                        let ts = base + i + 2 + lead;
                        out.push(Link {
                            start: base + start,
                            end: base + i + 2 + close + 2,
                            target: target.to_string(),
                            target_start: ts,
                            target_end: ts + target.len(),
                            kind: LinkKind::Wiki,
                        });
                        i = i + 2 + close + 2;
                        continue;
                    }
                }
                i += 2;
            }
            b']' if b.get(i + 1) == Some(&b'(') => {
                // Markdown link destination: `](path.md)` or `](<path with spaces.md>)`.
                let s = i + 2;
                let (ds, de) = if b.get(s) == Some(&b'<') {
                    match line[s + 1..].find('>') {
                        Some(e) => (s + 1, s + 1 + e),
                        None => {
                            i += 1;
                            continue;
                        }
                    }
                } else {
                    let e = line[s..].find([')', ' ', '\t', '\n']).map(|e| s + e).unwrap_or(b.len());
                    (s, e)
                };
                let dest = &line[ds..de];
                let lower = dest.to_ascii_lowercase();
                let is_local = !dest.is_empty()
                    && !lower.contains("://")
                    && !lower.starts_with("mailto:")
                    && !lower.starts_with('#');
                let path_part = dest.split('#').next().unwrap_or("");
                if is_local && path_part.to_ascii_lowercase().ends_with(".md") {
                    out.push(Link {
                        start: base + s,
                        end: base + de,
                        target: path_part.to_string(),
                        target_start: base + ds,
                        target_end: base + ds + path_part.len(),
                        kind: LinkKind::Markdown,
                    });
                }
                i = de.max(i + 1);
            }
            _ => i += 1,
        }
    }
}

/// Percent-decodes a markdown link destination (`My%20Note.md`).
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn url_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            _ => out.push(ch),
        }
    }
    out
}

fn without_md(p: &str) -> &str {
    if p.len() > 3 && p[p.len() - 3..].eq_ignore_ascii_case(".md") {
        &p[..p.len() - 3]
    } else {
        p
    }
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn dirname(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

/// Resolves a wiki link target the way Obsidian does: a bare name matches any
/// note with that file name (closest to the linking note wins); a name with a
/// slash matches by path suffix.
pub fn resolve_wiki<'a>(target: &str, from: &str, notes: &'a [String]) -> Option<&'a String> {
    let t = without_md(target.trim().trim_start_matches('/')).to_lowercase();
    if t.is_empty() {
        return None;
    }
    let from_dir = dirname(from).to_lowercase();
    let mut best: Option<(&String, (u8, usize, usize))> = None;
    for n in notes {
        let p = without_md(n).to_lowercase();
        let rank = if t.contains('/') {
            if p == t {
                (0, 0, 0)
            } else if p.ends_with(&format!("/{t}")) {
                (1, p.len(), 0)
            } else {
                continue;
            }
        } else {
            if basename(&p) != t {
                continue;
            }
            let dir = dirname(&p);
            let same = if dir == from_dir { 0 } else { 1 };
            (same, p.matches('/').count(), p.len())
        };
        if best.as_ref().map_or(true, |(_, r)| rank < *r) {
            best = Some((n, rank));
        }
    }
    best.map(|(n, _)| n)
}

/// Resolves a relative markdown link destination against the linking note's folder.
pub fn resolve_markdown(dest: &str, from: &str, notes: &[String]) -> Option<String> {
    let dest = url_decode(dest);
    let mut parts: Vec<&str> = if dest.starts_with('/') {
        Vec::new()
    } else {
        dirname(from).split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in dest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/").to_lowercase();
    notes.iter().find(|n| n.to_lowercase() == joined).cloned()
}

pub fn resolve(link: &Link, from: &str, notes: &[String]) -> Option<String> {
    match link.kind {
        LinkKind::Wiki => resolve_wiki(&link.target, from, notes).cloned(),
        LinkKind::Markdown => resolve_markdown(&link.target, from, notes),
    }
}

/// The shortest wiki target that still resolves to `path` from `from`:
/// the bare name when it is unambiguous, otherwise the full path.
pub fn wiki_target_for(path: &str, from: &str, notes: &[String]) -> String {
    let name = without_md(basename(path)).to_string();
    match resolve_wiki(&name, from, notes) {
        Some(p) if p == path => name,
        _ => without_md(path).to_string(),
    }
}

/// A relative markdown destination from `from`'s folder to `to`.
pub fn relative_path(from: &str, to: &str) -> String {
    let fd: Vec<&str> = dirname(from).split('/').filter(|s| !s.is_empty()).collect();
    let td: Vec<&str> = to.split('/').collect();
    let common = fd.iter().zip(td.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..fd.len() {
        parts.push("..".into());
    }
    for s in &td[common..] {
        parts.push((*s).to_string());
    }
    url_encode_path(&parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes() -> Vec<String> {
        ["Inbox.md", "Projects/Pinion.md", "Archive/Pinion.md", "Projects/Ideas.md", "Daily/2026-11-02.md"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn finds_wiki_links_and_skips_code() {
        let t = "See [[Pinion]] and ![[pic.png]] and [[Ideas|my ideas]].\n```\n[[Not]]\n```\nInline `[[no]]` [[Inbox#Top]]";
        let l: Vec<_> = find_links(t).into_iter().map(|l| l.target).collect();
        assert_eq!(l, vec!["Pinion", "pic.png", "Ideas", "Inbox"]);
    }

    #[test]
    fn finds_markdown_links() {
        let t = "A [link](Projects/Ideas.md) and [web](https://x.com/a.md) and [sp](<My Note.md>)";
        let l: Vec<_> = find_links(t).into_iter().map(|l| l.target).collect();
        assert_eq!(l, vec!["Projects/Ideas.md", "My Note.md"]);
    }

    #[test]
    fn resolves_like_obsidian() {
        let n = notes();
        assert_eq!(resolve_wiki("Pinion", "Projects/Ideas.md", &n).unwrap(), "Projects/Pinion.md");
        assert_eq!(resolve_wiki("Pinion", "Archive/Other.md", &n).unwrap(), "Archive/Pinion.md");
        assert_eq!(resolve_wiki("archive/pinion", "Inbox.md", &n).unwrap(), "Archive/Pinion.md");
        assert_eq!(resolve_wiki("inbox", "Projects/Ideas.md", &n).unwrap(), "Inbox.md");
        assert!(resolve_wiki("Missing", "Inbox.md", &n).is_none());
        assert_eq!(resolve_markdown("../Inbox.md", "Projects/Ideas.md", &n).unwrap(), "Inbox.md");
        assert_eq!(resolve_markdown("Ideas.md", "Projects/Pinion.md", &n).unwrap(), "Projects/Ideas.md");
    }

    #[test]
    fn shortest_targets() {
        let n = notes();
        assert_eq!(wiki_target_for("Inbox.md", "Projects/Ideas.md", &n), "Inbox");
        assert_eq!(wiki_target_for("Archive/Pinion.md", "Projects/Ideas.md", &n), "Archive/Pinion");
        assert_eq!(relative_path("Projects/Ideas.md", "Daily/2026-11-02.md"), "../Daily/2026-11-02.md");
        assert_eq!(relative_path("Inbox.md", "My Note.md"), "My%20Note.md");
    }
}
