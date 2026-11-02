//! The notes a brand new vault starts with.

#[cfg(target_os = "macos")]
const MOD: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

pub fn notes() -> Vec<(&'static str, String)> {
    vec![
        (
            "Welcome.md",
            format!(
                "Pinion is a quiet place to write. Every note is a plain markdown file in this folder, \
so other apps, like Obsidian, can open the same notes.\n\n\
## A few things to try\n\n\
- [ ] Type `[[` to link to another note, like [[Linking notes]]\n\
- [ ] Press {MOD} K to jump to any note\n\
- [ ] Press {MOD} \\ to hide the sidebar and just write\n\
- [ ] Pick a colour from the settings button at the foot of the sidebar\n\n\
Notes save as you type. There is no save button.\n"
            ),
        ),
        (
            "Linking notes.md",
            "Wrap a note's name in double brackets to link to it: [[Welcome]].\n\n\
- Show different words with a bar: [[Welcome|back to the start]]\n\
- Point at a heading with a hash: [[Welcome#A few things to try]]\n\
- Link to a note that doesn't exist yet, like [[Ideas]], and Pinion makes it when you click\n\n\
Renaming a note updates every link that points to it. Notes that link here are listed at the foot of the page.\n"
                .to_string(),
        ),
        (
            "Writing.md",
            "Pinion shows markdown as you write it. The symbols step aside once your cursor leaves them.\n\n\
**Bold**, *italic*, ~~struck through~~, ==highlighted== and `code`.\n\n\
> A quote sits on its own.\n\n\
1. Numbered lists\n\
2. keep counting\n\n\
---\n\n\
Add #tags anywhere, and paste images straight into a note.\n"
                .to_string(),
        ),
    ]
}
