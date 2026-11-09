//! Descriptions written in Markdown, shown formatted with Slint's `StyledText`.
//!
//! `StyledText` reads a subset of `CommonMark` (bold, italics, strike-through, inline code, links
//! and lists) and rejects the whole text on anything else. [`prepare`] turns the rest of the
//! common syntax into that subset first: headings become bold lines, quotes and rules lose
//! their markers, code blocks become inline code, `[ ]`/`[x]` become ☐/☑, bare URLs become
//! links and HTML is shown as typed. What still fails to parse is shown as plain text.

use slint::StyledText;

/// `text` formatted for display.
pub fn styled(text: &str) -> StyledText {
    StyledText::from_markdown(&prepare(text)).unwrap_or_else(|e| {
        tracing::debug!("description shown as plain text: {e}");
        StyledText::from_plain_text(text)
    })
}

/// The first non-empty line of `text`, formatted (the note under a subtask's name).
pub fn first_line(text: &str) -> StyledText {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_fence(line) && !is_rule(line))
        .unwrap_or_default();
    styled(line)
}

/// `text` rewritten into the Markdown subset `StyledText` reads.
pub fn prepare(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if is_fence(line.trim_start()) {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(code_line(line));
        } else if let Some(line) = prose_line(line) {
            out.push(line);
        }
    }
    out.join("\n")
}

/// A line of a code block: inline code, its indentation kept with no-break spaces (four
/// leading spaces would start an indented code block, which `StyledText` rejects).
fn code_line(line: &str) -> String {
    let body = line.trim_start();
    if body.is_empty() {
        return String::new();
    }
    let indent = "\u{a0}".repeat(line.len() - body.len());
    format!("{indent}`` {body} ``")
}

/// A line outside code blocks; `None` drops it (rules, setext underlines).
fn prose_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if is_rule(trimmed) || is_setext(trimmed) {
        return None;
    }
    let body = strip_quote(trimmed);
    if let Some(heading) = heading(body) {
        return Some(if heading.is_empty() {
            String::new()
        } else {
            format!("**{}**", inline(heading))
        });
    }
    if let Some((marker, rest)) = list_item(body) {
        // Nesting comes from the indentation, which only list items keep.
        let indent = &line[..line.len() - line.trim_start().len()];
        return Some(format!("{indent}{marker} {}", inline(&task_box(rest))));
    }
    // Indentation outside lists would start an indented code block.
    Some(inline(body))
}

fn is_fence(line: &str) -> bool {
    line.starts_with("```") || line.starts_with("~~~")
}

/// `---`, `***`, `___` (spaces allowed): a horizontal rule, or a setext heading's underline.
fn is_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3 && matches!(chars[0], '-' | '*' | '_') && chars.iter().all(|&c| c == chars[0])
}

/// `===`: a setext heading's underline.
fn is_setext(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c == '=')
}

fn strip_quote(mut line: &str) -> &str {
    while let Some(rest) = line.strip_prefix('>') {
        line = rest.trim_start();
    }
    line
}

/// The text of a `# Heading` line (closing `#`s removed).
fn heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim_end())
}

/// `- item`, `* item`, `+ item`, `1. item`, `1) item`: the marker and the rest.
fn list_item(line: &str) -> Option<(&str, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let marker_len = match line[digits..].chars().next() {
        Some('-' | '*' | '+') if digits == 0 => 1,
        Some('.' | ')') if (1..=9).contains(&digits) => digits + 1,
        _ => return None,
    };
    let rest = &line[marker_len..];
    if rest.is_empty() {
        return Some((&line[..marker_len], ""));
    }
    rest.strip_prefix(' ')
        .map(|rest| (&line[..marker_len], rest.trim_start()))
}

/// `[ ] item` → `☐ item`, `[x] item` → `☑ item`.
fn task_box(item: &str) -> String {
    if let Some(rest) = item.strip_prefix("[ ] ") {
        format!("☐ {rest}")
    } else if let Some(rest) = item
        .strip_prefix("[x] ")
        .or_else(|| item.strip_prefix("[X] "))
    {
        format!("☑ {rest}")
    } else {
        item.to_owned()
    }
}

/// Inline syntax: images become links, HTML is escaped (outside code spans), bare URLs become
/// links.
fn inline(text: &str) -> String {
    let text = text.replace("![", "[");
    let escaped: String = text
        .split('`')
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 0 {
                part.replace('<', "\\<")
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("`");
    escaped.split(' ').map(link).collect::<Vec<_>>().join(" ")
}

/// A bare `http(s)://` word as a link; trailing punctuation stays outside it.
fn link(word: &str) -> String {
    if !(word.starts_with("http://") || word.starts_with("https://")) {
        return word.to_owned();
    }
    let url = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
    let tail = &word[url.len()..];
    format!("[{url}]({url}){tail}")
}

/// Whether the formatted `text` has links (a click on it then opens them instead of editing).
pub fn has_links(text: &str) -> bool {
    prepare(text).contains("](")
}

/// Whether a clicked link may be opened: web and mail links only.
pub fn is_openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parses(text: &str) -> bool {
        StyledText::from_markdown(&prepare(text)).is_ok()
    }

    #[test]
    fn the_supported_subset_passes_through() {
        let text = "**negrito** *itálico* ~~riscado~~ `código` [site](https://a.b)\n- um\n  - dois\n1. três";
        assert_eq!(prepare(text), text);
        assert!(parses(text));
    }

    #[test]
    fn unsupported_blocks_become_supported_ones() {
        assert_eq!(prepare("# Título #"), "**Título**");
        assert_eq!(prepare("## "), "");
        assert_eq!(prepare("> citação\n>> funda"), "citação\nfunda");
        assert_eq!(prepare("a\n---\nb\n* * *"), "a\nb");
        assert_eq!(prepare("Título\n===="), "Título");
        assert_eq!(prepare("- [ ] fazer\n- [x] feito"), "- ☐ fazer\n- ☑ feito");
        assert_eq!(
            prepare("```rust\nfn a() {\n    b();\n```"),
            "`` fn a() { ``\n\u{a0}\u{a0}\u{a0}\u{a0}`` b(); ``"
        );
        assert_eq!(prepare("    recuado"), "recuado");
    }

    #[test]
    fn inline_html_images_and_bare_urls() {
        assert_eq!(prepare("a <b> `<c>`"), "a \\<b> `<c>`");
        assert_eq!(
            prepare("![logo](https://x.y/l.png)"),
            "[logo](https://x.y/l.png)"
        );
        assert_eq!(
            prepare("veja https://x.y/a."),
            "veja [https://x.y/a](https://x.y/a)."
        );
    }

    #[test]
    fn everything_typed_still_parses() {
        let samples = [
            "# T\n> q\n---\n```\n<html>\n```\n- [ ] a\n    código\n![i](u)\n<div>x</div>",
            "texto **sem fim",
            "1) a\n2) b\n\n   continuação",
            "|a|b|\n|-|-|\n|1|2|",
            "",
            "\n\n\n",
            "* \n- \n#",
        ];
        for sample in samples {
            assert!(parses(sample), "{sample:?} → {:?}", prepare(sample));
        }
    }

    #[test]
    fn the_note_is_the_first_written_line() {
        assert_eq!(first_line("\n---\n**a**\nb"), styled("**a**"));
        assert_eq!(first_line(""), styled(""));
    }

    #[test]
    fn links_are_found_after_preparing() {
        assert!(has_links("veja https://a.b"));
        assert!(has_links("[a](https://a.b)"));
        assert!(!has_links("**nada** aqui"));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(is_openable("https://a.b"));
        assert!(is_openable("MAILTO:a@b.c"));
        assert!(!is_openable("file:///C:/x.exe"));
        assert!(!is_openable("javascript:alert(1)"));
    }
}
