//! The vault: a plain folder of markdown files, readable by Obsidian and anything else.

use crate::links::{self, LinkKind};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_DEPTH: usize = 24;
const MAX_ENTRIES: usize = 40_000;

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub dir: bool,
    pub mtime: u64,
}

#[derive(Serialize)]
pub struct NoteText {
    pub text: String,
    pub mtime: u64,
}

#[derive(Serialize)]
pub struct Renamed {
    pub path: String,
    /// Notes whose links were rewritten to follow the rename (new paths).
    pub updated: Vec<String>,
}

#[derive(Serialize)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Serialize)]
pub struct Backlink {
    pub path: String,
    pub lines: Vec<String>,
}

pub fn is_note(p: &str) -> bool {
    p.len() > 3 && p[p.len() - 3..].eq_ignore_ascii_case(".md")
}

fn mtime_of(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn mtime(path: &Path) -> u64 {
    fs::metadata(path).map(|m| mtime_of(&m)).unwrap_or(0)
}

/// Joins a `/`-separated vault path onto the root, refusing anything that could leave it.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut out = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        let comp = Path::new(part).components().next();
        if part.contains('\\') || !matches!(comp, Some(Component::Normal(_))) || part.contains(':') {
            return Err(format!("Invalid path: {rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn hidden(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

/// Every folder and file in the vault, skipping hidden folders such as `.obsidian` and `.trash`.
pub fn list(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Entry>) {
    if depth > MAX_DEPTH || out.len() > MAX_ENTRIES {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if hidden(&name) {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_dir() {
            let meta = e.metadata().ok();
            out.push(Entry {
                path: rel_of(root, &path),
                dir: true,
                mtime: meta.as_ref().map(mtime_of).unwrap_or(0),
            });
            walk(root, &path, depth + 1, out);
        } else if ft.is_file() || ft.is_symlink() {
            // Symlinked files are fine; symlinked folders are skipped to avoid loops.
            let Ok(meta) = fs::metadata(&path) else { continue };
            if meta.is_file() {
                out.push(Entry { path: rel_of(root, &path), dir: false, mtime: mtime_of(&meta) });
            }
        }
    }
}

pub fn note_paths(root: &Path) -> Vec<String> {
    list(root).into_iter().filter(|e| !e.dir && is_note(&e.path)).map(|e| e.path).collect()
}

pub fn read(root: &Path, rel: &str) -> Result<NoteText, String> {
    let p = resolve(root, rel)?;
    let bytes = fs::read(&p).map_err(|e| format!("Couldn't open {rel}: {e}"))?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(NoteText { text: String::from_utf8_lossy(bytes).into_owned(), mtime: mtime(&p) })
}

/// Writes through a temporary file and a rename, so a crash never leaves half a note.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.pinion-tmp"));
    let res = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        // Some sync tools hold files open on Windows; fall back to a plain write.
        let _ = fs::remove_file(&tmp);
        fs::write(path, bytes)?;
    }
    Ok(())
}

pub fn write(root: &Path, rel: &str, text: &str) -> Result<u64, String> {
    let p = resolve(root, rel)?;
    write_atomic(&p, text.as_bytes()).map_err(|e| format!("Couldn't save {rel}: {e}"))?;
    Ok(mtime(&p))
}

/// Checks a note or folder name: no path separators and nothing that breaks `[[links]]`.
pub fn clean_name(name: &str) -> Result<String, String> {
    let n = name.trim().trim_end_matches('.').trim().to_string();
    if n.is_empty() {
        return Err("Give it a name.".into());
    }
    if let Some(c) = n.chars().find(|c| r#"\/:*?"<>|#^[]"#.contains(*c) || c.is_control()) {
        return Err(format!("Names can't contain {c}"));
    }
    let stem = n.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit();
    if reserved.contains(&stem.as_str()) || numbered {
        return Err(format!("{n} is a reserved name on Windows."));
    }
    if n.starts_with('.') {
        return Err("Names can't start with a dot.".into());
    }
    Ok(n)
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

/// `dir/name.md`, or `dir/name 1.md` and so on when that's taken.
pub fn unique(root: &Path, dir: &str, stem: &str, ext: &str) -> Result<String, String> {
    for i in 0..10_000 {
        let name = if i == 0 { format!("{stem}{ext}") } else { format!("{stem} {i}{ext}") };
        let rel = join_rel(dir, &name);
        if !resolve(root, &rel)?.exists() {
            return Ok(rel);
        }
    }
    Err("Too many notes with that name.".into())
}

pub fn create_note(root: &Path, dir: &str, name: &str, text: &str) -> Result<String, String> {
    let name = clean_name(name)?;
    let rel = unique(root, dir, &name, ".md")?;
    write_atomic(&resolve(root, &rel)?, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(rel)
}

pub fn create_folder(root: &Path, dir: &str, name: &str) -> Result<String, String> {
    let name = clean_name(name)?;
    let rel = unique(root, dir, &name, "")?;
    fs::create_dir_all(resolve(root, &rel)?).map_err(|e| e.to_string())?;
    Ok(rel)
}

/// Renames or moves a note or folder, then rewrites every link that pointed into it.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<Renamed, String> {
    if from == to {
        return Ok(Renamed { path: to.into(), updated: vec![] });
    }
    if to.starts_with(&format!("{from}/")) {
        return Err("A folder can't move into itself.".into());
    }
    let src = resolve(root, from)?;
    let dst = resolve(root, to)?;
    let case_only = from.to_lowercase() == to.to_lowercase();
    if dst.exists() && !case_only {
        return Err("Something with that name is already there.".into());
    }
    let before = note_paths(root);
    let moved: Vec<(String, String)> = if src.is_dir() {
        let prefix = format!("{from}/");
        before
            .iter()
            .filter(|n| n.starts_with(&prefix))
            .map(|n| (n.clone(), format!("{to}/{}", &n[prefix.len()..])))
            .collect()
    } else {
        vec![(from.to_string(), to.to_string())]
    };
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if case_only {
        // Case-insensitive file systems need a detour to change only the case.
        let tmp = src.with_file_name(format!(".pinion-rename-{}", std::process::id()));
        fs::rename(&src, &tmp).and_then(|_| fs::rename(&tmp, &dst)).map_err(|e| e.to_string())?;
    } else {
        fs::rename(&src, &dst).map_err(|e| format!("Couldn't rename: {e}"))?;
    }

    let new_of = |p: &str| moved.iter().find(|(o, _)| o == p).map(|(_, n)| n.clone());
    let after: Vec<String> = before.iter().map(|n| new_of(n).unwrap_or_else(|| n.clone())).collect();
    let mut updated = Vec::new();
    for (old_note, new_note) in before.iter().zip(after.iter()) {
        let Ok(p) = resolve(root, new_note) else { continue };
        let Ok(text) = fs::read_to_string(&p) else { continue };
        let note_moved = old_note != new_note;
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for l in links::find_links(&text) {
            let Some(target) = links::resolve(&l, old_note, &before) else { continue };
            let target_moved = new_of(&target);
            if target_moved.is_none() && !(note_moved && l.kind == LinkKind::Markdown) {
                continue;
            }
            let now = target_moved.unwrap_or(target);
            let rep = match l.kind {
                LinkKind::Wiki => {
                    let keep_path = l.target.contains('/');
                    let short = links::wiki_target_for(&now, new_note, &after);
                    if keep_path && !short.contains('/') {
                        now.trim_end_matches(".md").to_string()
                    } else {
                        short
                    }
                }
                LinkKind::Markdown => links::relative_path(new_note, &now),
            };
            if rep != l.target {
                edits.push((l.target_start, l.target_end, rep));
            }
        }
        if edits.is_empty() {
            continue;
        }
        let mut out = text.clone();
        for (s, e, r) in edits.into_iter().rev() {
            out.replace_range(s..e, &r);
        }
        if write_atomic(&p, out.as_bytes()).is_ok() {
            updated.push(new_note.clone());
        }
    }
    Ok(Renamed { path: to.to_string(), updated })
}

/// Moves a note or folder into the vault's `.trash` folder, as Obsidian does.
pub fn trash(root: &Path, rel: &str) -> Result<(), String> {
    let src = resolve(root, rel)?;
    if !src.exists() {
        return Ok(());
    }
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let (stem, ext) = match name.rfind('.') {
        Some(i) if !src.is_dir() && i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let dest = unique(root, ".trash", stem, ext)?;
    let dst = resolve(root, &dest)?;
    fs::create_dir_all(dst.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
    fs::rename(&src, &dst).map_err(|e| format!("Couldn't move to trash: {e}"))
}

fn snippet(line: &str, max: usize) -> String {
    let t = line.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let s: String = t.chars().take(max).collect();
        format!("{s}…")
    }
}

/// Plain case-insensitive search over every note, newest notes first.
pub fn search(root: &Path, query: &str, limit: usize) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut notes: Vec<Entry> = list(root).into_iter().filter(|e| !e.dir && is_note(&e.path)).collect();
    notes.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    let mut out = Vec::new();
    for n in notes {
        let Ok(text) = fs::read_to_string(resolve(root, &n.path).unwrap_or_default()) else { continue };
        let mut per = 0;
        for (i, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            if let Some(pos) = lower.find(&q) {
                // Centre long lines on the match.
                let chars_before = lower[..pos].chars().count();
                let start = chars_before.saturating_sub(40);
                let s: String = line.chars().skip(start).take(160).collect();
                out.push(Hit {
                    path: n.path.clone(),
                    line: i,
                    text: if start > 0 { format!("…{}", s.trim()) } else { s.trim().to_string() },
                });
                per += 1;
                if per >= 3 || out.len() >= limit {
                    break;
                }
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Notes that link to `rel`, with the lines that hold the links.
pub fn backlinks(root: &Path, rel: &str) -> Vec<Backlink> {
    let notes = note_paths(root);
    let mut out = Vec::new();
    for n in &notes {
        if n == rel {
            continue;
        }
        let Ok(text) = fs::read_to_string(resolve(root, n).unwrap_or_default()) else { continue };
        if !text.contains("[[") && !text.contains("](") {
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        for l in links::find_links(&text) {
            if links::resolve(&l, n, &notes).as_deref() == Some(rel) {
                let ls = text[..l.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let le = text[l.end..].find('\n').map(|i| l.end + i).unwrap_or(text.len());
                let s = snippet(&text[ls..le], 200);
                if !lines.contains(&s) {
                    lines.push(s);
                }
            }
        }
        if !lines.is_empty() {
            lines.truncate(3);
            out.push(Backlink { path: n.clone(), lines });
        }
    }
    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    out
}

/// Saves pasted or dropped files next to the vault root, like Obsidian's default.
pub fn save_attachment(root: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    let name = clean_name(name).unwrap_or_else(|_| "Pasted file".into());
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.clone(), String::new()),
    };
    let rel = unique(root, "", &stem, &ext)?;
    write_atomic(&resolve(root, &rel)?, bytes).map_err(|e| e.to_string())?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_vault(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pinion-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("Projects")).unwrap();
        fs::write(d.join("Inbox.md"), "See [[Pinion]] and [[Projects/Pinion|it]] and [md](Projects/Pinion.md).\n").unwrap();
        fs::write(d.join("Projects/Pinion.md"), "Back to [[Inbox]] and [up](../Inbox.md)\n").unwrap();
        fs::write(d.join("Projects/Other.md"), "[[Pinion#Heading]] here\n").unwrap();
        d
    }

    #[test]
    fn rename_rewrites_links() {
        let d = tmp_vault("rename");
        let r = rename(&d, "Projects/Pinion.md", "Projects/Quill.md").unwrap();
        assert_eq!(r.updated.len(), 2);
        let inbox = fs::read_to_string(d.join("Inbox.md")).unwrap();
        assert_eq!(inbox, "See [[Quill]] and [[Projects/Quill|it]] and [md](Projects/Quill.md).\n");
        let other = fs::read_to_string(d.join("Projects/Other.md")).unwrap();
        assert_eq!(other, "[[Quill#Heading]] here\n");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn folder_move_fixes_relative_links() {
        let d = tmp_vault("folder");
        rename(&d, "Projects", "Work/Projects").unwrap();
        let p = fs::read_to_string(d.join("Work/Projects/Pinion.md")).unwrap();
        assert_eq!(p, "Back to [[Inbox]] and [up](../../Inbox.md)\n");
        let inbox = fs::read_to_string(d.join("Inbox.md")).unwrap();
        assert!(inbox.contains("[[Work/Projects/Pinion|it]]"), "{inbox}");
        assert!(inbox.contains("](Work/Projects/Pinion.md)"), "{inbox}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn backlinks_and_search() {
        let d = tmp_vault("back");
        let b = backlinks(&d, "Projects/Pinion.md");
        let paths: Vec<_> = b.iter().map(|b| b.path.as_str()).collect();
        assert_eq!(paths, vec!["Inbox.md", "Projects/Other.md"]);
        let hits = search(&d, "heading", 10);
        assert_eq!(hits.len(), 1);
        trash(&d, "Inbox.md").unwrap();
        assert!(d.join(".trash/Inbox.md").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn names_and_paths() {
        assert!(clean_name("a/b").is_err());
        assert!(clean_name("Note #1").is_err());
        assert!(clean_name("con").is_err());
        assert_eq!(clean_name("  Ideas. ").unwrap(), "Ideas");
        assert!(resolve(Path::new("/v"), "../etc/passwd").is_err());
        assert!(resolve(Path::new("/v"), "a/../../b").is_err());
    }
}
