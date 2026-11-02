//! Light clean-up of what the model heard.

const FILLERS: [&str; 8] = ["um", "umm", "uh", "uhh", "uhm", "erm", "hmm", "mmm"];

pub fn clean(raw: &str, remove_fillers: bool) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut cap_next = false;
    for w in raw.split_whitespace() {
        let core = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if remove_fillers && FILLERS.contains(&core.as_str()) {
            // "Um, so" -> "So": carry sentence-ending punctuation back, capitalise what follows.
            if words.is_empty() {
                cap_next = true;
            } else if let Some(p) = w.chars().last().filter(|c| matches!(c, '.' | '?' | '!')) {
                if let Some(prev) = words.last_mut() {
                    if !prev.ends_with(['.', '?', '!', ',']) {
                        prev.push(p);
                    }
                }
                cap_next = true;
            }
            continue;
        }
        let mut w = w.to_string();
        if cap_next {
            w = capitalise(&w);
            cap_next = false;
        }
        words.push(w);
    }
    let mut s = words.join(" ");
    s = commands(&s);
    s.trim().to_string()
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// "New line" and "new paragraph", said as their own phrase, become line breaks.
fn commands(s: &str) -> String {
    let mut out = s.to_string();
    for (phrase, rep) in [("new paragraph", "\n\n"), ("new line", "\n")] {
        let mut from = 0;
        loop {
            let lower = out.to_lowercase();
            let Some(rel) = lower[from..].find(phrase) else { break };
            let i = from + rel;
            let before = lower[..i].trim_end();
            let starts_clause = before.is_empty() || before.ends_with(['.', ',', '!', '?', '\n']);
            let mut end = i + phrase.len();
            let after = &lower[end..];
            let trailing = after.chars().next();
            let ends_clause = matches!(trailing, None | Some('.') | Some(',') | Some('!') | Some('?'));
            if !(starts_clause && ends_clause) {
                from = end;
                continue;
            }
            if matches!(trailing, Some('.') | Some(',') | Some('!') | Some('?')) {
                end += 1;
            }
            let rest = out[end..].trim_start().to_string();
            let head = out[..i].trim_end().trim_end_matches(',').to_string();
            out = format!("{head}{rep}{}", capitalise(&rest));
            from = head.len() + rep.len();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_fillers() {
        assert_eq!(clean("Um, so I think we should, uh, ship it.", true), "So I think we should, ship it.");
        assert_eq!(clean("Um, so I think", false), "Um, so I think");
    }

    #[test]
    fn line_commands() {
        assert_eq!(clean("Hello there. New line. How are you?", true), "Hello there.\nHow are you?");
        assert_eq!(clean("First point, new paragraph, second point.", true), "First point\n\nSecond point.");
        assert_eq!(clean("We need a new line of products.", true), "We need a new line of products.");
    }
}
