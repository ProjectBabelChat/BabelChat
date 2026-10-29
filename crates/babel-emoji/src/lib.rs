//! A guild's own emoji in message text.
//!
//! A guild's emoji is written into text (and reactions) as a code:
//!
//! ```text
//! <:name:guild_id.emoji_id>        <:party:3f9c…e1.media-0.8a2b…>
//! ```
//!
//! The code says everything a reader needs, whichever guild the text is
//! in: the name (shown as `:name:` when the image cannot be), the guild that
//! owns the emoji, and its ID, which names the Media service holding the
//! image. Edges check that the writer may use each code, Message Cells
//! hand translators the text with `:name:` in place of codes ([`plain`]),
//! and clients put the codes back into translations ([`restore`]).
//!
//! See docs/architecture/media.md.

use std::{collections::HashMap, ops::Range};

/// Longest emoji name.
pub const MAX_NAME_CHARS: usize = 32;
const MAX_PART_CHARS: usize = 64;

/// One emoji code found in text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomEmoji<'a> {
    pub name: &'a str,
    pub guild_id: &'a str,
    /// The emoji's ID in its guild (`media-0.8a2b…`).
    pub id: &'a str,
}

impl CustomEmoji<'_> {
    pub fn code(&self) -> String {
        code(self.name, self.guild_id, self.id)
    }

    /// `guild_id.emoji_id`: the same emoji whatever name a code gives it.
    pub fn key(&self) -> String {
        format!("{}.{}", self.guild_id, self.id)
    }
}

/// The code of a guild's emoji.
pub fn code(name: &str, guild_id: &str, id: &str) -> String {
    format!("<:{name}:{guild_id}.{id}>")
}

/// Emoji names: 2 to 32 ASCII letters, digits and underscores.
pub fn valid_name(name: &str) -> bool {
    (2..=MAX_NAME_CHARS).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// IDs and service names: ASCII letters, digits, `-` and `_`.
fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PART_CHARS
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `text` if it is exactly one emoji code.
pub fn parse(text: &str) -> Option<CustomEmoji<'_>> {
    let inner = text.strip_prefix("<:")?.strip_suffix('>')?;
    let (name, reference) = inner.split_once(':')?;
    let (guild_id, id) = reference.split_once('.')?;
    let (service, random) = id.split_once('.')?;
    let ok = valid_name(name) && [guild_id, service, random].into_iter().all(valid_part);
    ok.then_some(CustomEmoji { name, guild_id, id })
}

/// A run of text, or one emoji code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    Text(&'a str),
    Emoji(CustomEmoji<'a>),
}

/// Every emoji code in `text`, with where it is.
pub fn find(text: &str) -> Vec<(Range<usize>, CustomEmoji<'_>)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("<:").map(|at| from + at) {
        let end = text[at..].find('>').map(|end| at + end + 1);
        match end.and_then(|end| parse(&text[at..end]).map(|emoji| (end, emoji))) {
            Some((end, emoji)) => {
                found.push((at..end, emoji));
                from = end;
            }
            None => from = at + 2,
        }
    }
    found
}

/// `text` cut into runs of text and emoji codes, in order.
pub fn segments(text: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut from = 0;
    for (range, emoji) in find(text) {
        if from < range.start {
            segments.push(Segment::Text(&text[from..range.start]));
        }
        segments.push(Segment::Emoji(emoji));
        from = range.end;
    }
    if from < text.len() {
        segments.push(Segment::Text(&text[from..]));
    }
    segments
}

/// `text` with `:name:` in place of every code: what translators and
/// plain-text readers get.
pub fn plain(text: &str) -> String {
    segments(text)
        .into_iter()
        .map(|segment| match segment {
            Segment::Text(text) => text.to_string(),
            Segment::Emoji(emoji) => format!(":{}:", emoji.name),
        })
        .collect()
}

/// True if `text` holds emoji codes and nothing else but whitespace:
/// nothing to translate.
pub fn only_emoji(text: &str) -> bool {
    let segments = segments(text);
    segments
        .iter()
        .any(|segment| matches!(segment, Segment::Emoji(_)))
        && segments.iter().all(|segment| match segment {
            Segment::Text(text) => text.trim().is_empty(),
            Segment::Emoji(_) => true,
        })
}

/// Replaces each `:name:` outside codes for which `code_for` gives a code.
pub fn fill(text: &str, mut code_for: impl FnMut(&str) -> Option<String>) -> String {
    let mut filled = String::with_capacity(text.len());
    for segment in segments(text) {
        match segment {
            Segment::Text(text) => fill_run(text, &mut code_for, &mut filled),
            Segment::Emoji(emoji) => filled.push_str(&emoji.code()),
        }
    }
    filled
}

fn fill_run(text: &str, code_for: &mut impl FnMut(&str) -> Option<String>, out: &mut String) {
    let mut rest = text;
    while let Some(at) = rest.find(':') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let replaced = after.find(':').and_then(|end| {
            let name = &after[..end];
            valid_name(name)
                .then(|| code_for(name))
                .flatten()
                .map(|code| (code, end))
        });
        match replaced {
            Some((code, end)) => {
                out.push_str(&code);
                rest = &after[end + 1..];
            }
            None => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

/// A translation of [`plain`]`(original)` with the original's codes put
/// back where their `:name:` survived translation.
pub fn restore(original: &str, translated: &str) -> String {
    let codes = find(original)
        .into_iter()
        .rev()
        .map(|(_, emoji)| (emoji.name, emoji.code()))
        .collect::<HashMap<_, _>>();
    if codes.is_empty() {
        return translated.to_string();
    }
    fill(translated, |name| codes.get(name).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTY: &str = "<:party:3f9ce1.media-0.8a2b>";

    #[test]
    fn codes_are_found_in_text() {
        let text = format!("let's {PARTY}! <:bad name:1.m.2> <:x:1.m.2> <:a:1.m.2");
        let found = find(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].1,
            CustomEmoji {
                name: "party",
                guild_id: "3f9ce1",
                id: "media-0.8a2b"
            }
        );
        assert_eq!(&text[found[0].0.clone()], PARTY);
        assert_eq!(found[0].1.code(), PARTY);
        assert_eq!(found[0].1.key(), "3f9ce1.media-0.8a2b");
        assert_eq!(
            plain(&text),
            "let's :party:! <:bad name:1.m.2> <:x:1.m.2> <:a:1.m.2"
        );
        assert!(parse(PARTY).is_some());
        assert!(parse(&format!("{PARTY} ")).is_none());
        assert!(parse("<:party:z.z.media-0.8a2b>").is_none());
        assert!(parse("<:party:g1.media-0>").is_none());
    }

    #[test]
    fn text_splits_into_runs_and_codes() {
        let text = format!("{PARTY}hi{PARTY}");
        assert_eq!(
            segments(&text),
            vec![
                Segment::Emoji(parse(PARTY).unwrap()),
                Segment::Text("hi"),
                Segment::Emoji(parse(PARTY).unwrap()),
            ]
        );
        assert!(only_emoji(&format!(" {PARTY}\n{PARTY} ")));
        assert!(!only_emoji(&format!("{PARTY} hi")));
        assert!(!only_emoji("  "));
    }

    #[test]
    fn names_typed_become_codes() {
        let text = format!("at 10:30: :party: and :cake: {PARTY} ::party::");
        let filled = fill(&text, |name| (name == "party").then(|| PARTY.to_string()));
        assert_eq!(
            filled,
            format!("at 10:30: {PARTY} and :cake: {PARTY} :{PARTY}:")
        );
    }

    #[test]
    fn translations_get_their_codes_back() {
        let original = format!("{PARTY} 明日は :cake: パーティー");
        let translated = "Party tomorrow :party: :cake:";
        assert_eq!(
            restore(&original, translated),
            format!("Party tomorrow {PARTY} :cake:")
        );
        assert_eq!(restore("plain", "text :party:"), "text :party:");
        assert!(valid_name("a_1"));
        assert!(!valid_name("a"));
        assert!(!valid_name("emoji-name"));
    }
}
