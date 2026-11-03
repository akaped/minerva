//! Links to blocks and notes shown inside other notes.
//!
//! `[label](#note/{id}#{block})` links to one block of a note.
//! `![label](#note/{id})` shows a whole note inside another, and
//! `![label](#note/{id}#{block})` one block (with the blocks nested under it).
//! Rendering leaves a slot (`render::embed_slot`). The app fills it from
//! `POST embeds` and checks it for changes, so an embed stays current.
use super::*;

/// The note and block a `#note/{id}[#{block}]` destination points at.
pub(crate) fn target(destination: &str) -> Option<(&str, Option<&str>)> {
    let rest = destination.strip_prefix("#note/")?;
    let (note, block) = match rest.split_once('#') {
        Some((note, block)) => (note, Some(block)),
        None => (rest, None),
    };
    Uuid::parse_str(note).ok().filter(|_| note.len() == 36)?;
    if let Some(block) = block {
        let valid = (1..=64).contains(&block.len())
            && block
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid {
            return None;
        }
    }
    Some((note, block))
}

/// A link or embed of a note written in Markdown: `[label](#note/…)` or
/// `![label](#note/…)`.
#[derive(Debug, PartialEq)]
pub(crate) struct NoteRef<'a> {
    /// Where it is written, from `[` (or `!`) to `)`.
    pub start: usize,
    pub end: usize,
    pub embed: bool,
    /// As written, with its escapes.
    pub label: &'a str,
    pub note: &'a str,
    pub block: Option<&'a str>,
}

/// Every well-formed note link and embed in `markdown`, in order. Code is not
/// skipped: callers that must leave code alone check for it themselves.
pub(crate) fn note_refs(markdown: &str) -> Vec<NoteRef<'_>> {
    let bytes = markdown.as_bytes();
    let mut out = vec![];
    let mut from = 0;
    while let Some(at) = markdown[from..].find("](#note/").map(|i| i + from) {
        from = at + 2;
        let Some(close) = markdown[at + 2..].find(')').map(|i| i + at + 2) else {
            break;
        };
        let Some((note, block)) = target(&markdown[at + 2..close]) else {
            continue;
        };
        // The label's opening bracket (labels escape inner brackets).
        let Some(open) = (0..at)
            .rev()
            .find(|&i| bytes[i] == b'[' && (i == 0 || bytes[i - 1] != b'\\'))
        else {
            continue;
        };
        if out.last().is_some_and(|r: &NoteRef| r.end > open) {
            continue;
        }
        let embed = open > 0 && bytes[open - 1] == b'!';
        out.push(NoteRef {
            start: if embed { open - 1 } else { open },
            end: close + 1,
            embed,
            label: &markdown[open + 1..at],
            note,
            block,
        });
        from = close + 1;
    }
    out
}

/// Calls `replace` with each note link and embed in `markdown`; where it
/// returns text, that text takes the reference's place.
pub(crate) fn replace_refs<'a>(
    markdown: &'a str,
    mut replace: impl FnMut(&NoteRef) -> Option<String>,
) -> std::borrow::Cow<'a, str> {
    let refs = note_refs(markdown);
    let mut out = String::new();
    let mut last = 0;
    for r in &refs {
        if let Some(text) = replace(r) {
            out.push_str(&markdown[last..r.start]);
            out.push_str(&text);
            last = r.end;
        }
    }
    if last == 0 {
        return markdown.into();
    }
    out.push_str(&markdown[last..]);
    out.into()
}

/// A block and the blocks nested under it, with the block's own indentation
/// removed so it reads as the start of the embed.
pub(crate) fn with_children<'a>(n: &'a Note, block: &str) -> Option<Vec<(&'a Block, String)>> {
    let i = n.blocks.iter().position(|b| b.id == block)?;
    let level = render::indentation(&n.blocks[i].markdown).0;
    let outdent = |markdown: &str| {
        markdown
            .split('\n')
            .map(|line| {
                let spaces = line.len() - line.trim_start_matches(' ').len();
                &line[spaces.min(level * 2)..]
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut out = vec![(&n.blocks[i], outdent(&n.blocks[i].markdown))];
    for b in &n.blocks[i + 1..] {
        if b.markdown.trim().is_empty() || render::indentation(&b.markdown).0 <= level {
            break;
        }
        out.push((b, outdent(&b.markdown)));
    }
    Some(out)
}

/// The most embeds one request fills, and the most blocks shown for one note.
const MAX_ITEMS: usize = 50;
const MAX_BLOCKS: usize = 300;

/// `POST embeds` with `{items: [{note, block?, revision?}]}`: what each embed
/// shows. An item whose note is still at `revision` comes back `unchanged`.
/// A note the person cannot open, or that does not exist, is `unavailable`
/// either way, so an embed does not reveal which notes exist.
pub(super) fn route(d: &Data, u: &User, v: &Value) -> Result<Value, ApiError> {
    let items = v["items"]
        .as_array()
        .ok_or_else(|| bad("items is required"))?;
    if items.len() > MAX_ITEMS {
        return Err(bad("Too many embeds"));
    }
    let out = items
        .iter()
        .map(|item| {
            let note = item["note"].as_str().unwrap_or_default();
            let block = item["block"].as_str().filter(|b| !b.is_empty());
            let destination = match block {
                Some(b) => format!("#note/{note}#{b}"),
                None => format!("#note/{note}"),
            };
            if target(&destination).is_none() {
                return Err(bad("Invalid embed"));
            }
            let mut view = json!({"note": note, "block": block});
            let Some(n) = d
                .notes
                .iter()
                .find(|n| n.id == note && note_role(d, u, n).is_some())
            else {
                view["state"] = json!("unavailable");
                return Ok(view);
            };
            view["title"] = json!(n.title);
            view["icon"] = json!(n.icon);
            view["revision"] = json!(n.revision);
            if n.trashed {
                view["state"] = json!("trashed");
                return Ok(view);
            }
            let shown: Vec<(&Block, String)> = match block {
                Some(b) => match with_children(n, b) {
                    Some(blocks) => blocks,
                    None => {
                        view["state"] = json!("no-block");
                        return Ok(view);
                    }
                },
                None => n.blocks.iter().map(|b| (b, b.markdown.clone())).collect(),
            };
            view["state"] = json!("ok");
            if item["revision"].as_u64() == Some(n.revision) {
                view["unchanged"] = json!(true);
                return Ok(view);
            }
            view["more"] = json!(shown.len() > MAX_BLOCKS);
            view["blocks"] = json!(shown
                .iter()
                .take(MAX_BLOCKS)
                .map(|(b, markdown)| json!({"id": b.id, "html": render(markdown)}))
                .collect::<Vec<_>>());
            Ok(view)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(json!(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "0b8f5f9e-0000-4000-8000-000000000000";

    #[test]
    fn reads_targets() {
        assert_eq!(target(&format!("#note/{NOTE}")), Some((NOTE, None)));
        assert_eq!(
            target(&format!("#note/{NOTE}#b-1_x")),
            Some((NOTE, Some("b-1_x")))
        );
        assert_eq!(target("#note/123"), None);
        assert_eq!(target(&format!("#note/{NOTE}#")), None);
        assert_eq!(target(&format!("#note/{NOTE}#a b")), None);
        assert_eq!(target(&format!("#note/{NOTE}x")), None);
    }

    #[test]
    fn finds_links_and_embeds() {
        let md = format!(
            "See [a \\[b\\]](#note/{NOTE}#blk), ![Plan](#note/{NOTE}) and [web](https://x.y) [bad](#note/12)."
        );
        let refs = note_refs(&md);
        assert_eq!(refs.len(), 2);
        assert!(!refs[0].embed);
        assert_eq!(refs[0].label, "a \\[b\\]");
        assert_eq!(refs[0].block, Some("blk"));
        assert_eq!(
            &md[refs[0].start..refs[0].end],
            format!("[a \\[b\\]](#note/{NOTE}#blk)")
        );
        assert!(refs[1].embed);
        assert_eq!(
            &md[refs[1].start..refs[1].end],
            format!("![Plan](#note/{NOTE})")
        );
        let out = replace_refs(&md, |r| r.embed.then(|| "E".into()));
        assert!(out.contains(", E and"));
        assert!(out.contains("#blk)"));
    }

    #[test]
    fn a_block_brings_its_nested_blocks() {
        let mut n = getting_started("f", "en");
        n.blocks = ["- top", "  - child", "    - grandchild", "- next"]
            .iter()
            .enumerate()
            .map(|(i, m)| Block::new(m, i))
            .collect();
        n.blocks[1].id = "child".into();
        let shown = with_children(&n, "child").unwrap();
        let text: Vec<&str> = shown.iter().map(|(_, m)| m.as_str()).collect();
        assert_eq!(text, ["- child", "  - grandchild"]);
        assert_eq!(with_children(&n, &n.blocks[0].id.clone()).unwrap().len(), 3);
        assert!(with_children(&n, "gone").is_none());
    }
}
