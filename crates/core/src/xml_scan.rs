//! Finding elements in the machine-written XML of `.modinfo` and `.civ5proj` files, with their
//! byte positions.
//!
//! `modinfo.rs` only ever reads values, so it scans for payloads. Modinfo Completion also has to
//! insert lines between the ones a file already has without disturbing a byte of the rest, so it
//! needs to know where each element starts and ends. Same scanning approach - these files are
//! written by ModBuddy or a script, and these few shapes are all there is - but positional.

use std::ops::Range;

/// One element: `<name attributes>inner</name>`, or `<name attributes/>`.
#[derive(Debug, Clone)]
pub(crate) struct Element {
    /// Where `<name` begins.
    pub(crate) start: usize,
    /// The attribute list inside the opening tag.
    pub(crate) attributes: Range<usize>,
    /// Between the opening and the closing tag; empty, at the opening tag's end, when
    /// self-closing.
    pub(crate) inner: Range<usize>,
    /// Just past the closing tag (or the self-closing tag).
    pub(crate) end: usize,
    pub(crate) self_closing: bool,
}

/// `xml` with every comment's bytes replaced by spaces, so scanning it never finds a commented-out
/// element and every position still points at the same byte of `xml`.
pub(crate) fn mask_comments(xml: &str) -> String {
    let mut masked = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        masked.push_str(&rest[..start]);
        let comment = &rest[start..];
        let length = comment.find("-->").map_or(comment.len(), |end| end + 3);
        masked.extend(std::iter::repeat_n(' ', length));
        rest = &comment[length..];
    }
    masked.push_str(rest);
    masked
}

/// Every element named `name` within `within`, outermost only, in document order. An element of
/// the same name nested inside one is part of it, not a second find.
pub(crate) fn elements(xml: &str, within: Range<usize>, name: &str) -> Vec<Element> {
    let mut found = Vec::new();
    let mut cursor = within.start;
    while let Some(start) = find_open(xml, cursor..within.end, name) {
        let Some(element) = element_at(xml, start, name, within.end) else {
            break;
        };
        cursor = element.end;
        found.push(element);
    }
    found
}

/// The first element named `name` within `within`.
pub(crate) fn first(xml: &str, within: Range<usize>, name: &str) -> Option<Element> {
    let start = find_open(xml, within.clone(), name)?;
    element_at(xml, start, name, within.end)
}

/// Every child element directly within `within`, whatever its name, with that name.
pub(crate) fn children(xml: &str, within: Range<usize>) -> Vec<(String, Element)> {
    let mut found = Vec::new();
    let mut cursor = within.start;
    while let Some(offset) = xml[cursor..within.end].find('<') {
        let start = cursor + offset;
        let name: String = xml[start + 1..within.end]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
            .collect();
        let starts_a_name = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_');
        if !starts_a_name {
            // A closing tag, a declaration or something malformed: step past the `<`.
            cursor = start + 1;
            continue;
        }
        let Some(element) = element_at(xml, start, &name, within.end) else {
            break;
        };
        cursor = element.end;
        found.push((name, element));
    }
    found
}

/// The trimmed text inside `element`.
pub(crate) fn text<'a>(xml: &'a str, element: &Element) -> &'a str {
    xml[element.inner.clone()].trim()
}

/// The trimmed text of the first child element named `name`, or `""`.
pub(crate) fn child_text<'a>(xml: &'a str, parent: &Element, name: &str) -> &'a str {
    first(xml, parent.inner.clone(), name).map_or("", |child| text(xml, &child))
}

/// The raw value of attribute `name` - still XML-escaped, exactly as written.
pub(crate) fn raw_attribute<'a>(xml: &'a str, element: &Element, name: &str) -> Option<&'a str> {
    let attributes = &xml[element.attributes.clone()];
    let mut rest = attributes;
    loop {
        let start = rest.find(name)?;
        let preceded_ok = start == 0
            || rest[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = rest[start + name.len()..].trim_start();
        if preceded_ok && let Some(quoted) = after.strip_prefix('=') {
            let quoted = quoted.trim_start();
            let quote = quoted.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &quoted[1..];
                return Some(&inner[..inner.find(quote)?]);
            }
        }
        rest = &rest[start + name.len()..];
    }
}

/// Where the line holding position `at` begins, if only whitespace stands before `at` on it -
/// so a block of whole lines can go in front of the tag at `at` without splitting its line.
pub(crate) fn line_start_before(xml: &str, at: usize) -> Option<usize> {
    let line_start = xml[..at].rfind('\n').map_or(0, |newline| newline + 1);
    xml[line_start..at]
        .chars()
        .all(|c| c == ' ' || c == '\t')
        .then_some(line_start)
}

/// The next `<name` opening tag within `within` - `<File` must not match `<Files>`.
fn find_open(xml: &str, within: Range<usize>, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let mut cursor = within.start;
    while let Some(offset) = xml[cursor..within.end].find(&open) {
        let start = cursor + offset;
        let after = start + open.len();
        if xml[after..within.end]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
        {
            return Some(start);
        }
        cursor = after;
    }
    None
}

/// The element whose opening tag starts at `start`, with nested elements of the same name
/// counted so its own closing tag is the one found.
fn element_at(xml: &str, start: usize, name: &str, limit: usize) -> Option<Element> {
    let tag_end = start + xml[start..limit].find('>')?;
    let attributes_start = start + 1 + name.len();
    let self_closing = xml[..tag_end].ends_with('/');
    if self_closing {
        return Some(Element {
            start,
            attributes: attributes_start..tag_end - 1,
            inner: tag_end + 1..tag_end + 1,
            end: tag_end + 1,
            self_closing: true,
        });
    }
    let close = format!("</{name}>");
    let inner_start = tag_end + 1;
    let mut depth = 0usize;
    let mut cursor = inner_start;
    loop {
        let next_close = cursor + xml[cursor..limit].find(&close)?;
        match find_open(xml, cursor..next_close, name) {
            Some(nested) => {
                // A same-named element opens first: it is closed before ours is, unless it
                // closes itself.
                let nested_end = nested + xml[nested..limit].find('>')?;
                if !xml[..nested_end].ends_with('/') {
                    depth += 1;
                }
                cursor = nested_end + 1;
            }
            None if depth > 0 => {
                depth -= 1;
                cursor = next_close + close.len();
            }
            None => {
                return Some(Element {
                    start,
                    attributes: attributes_start..tag_end,
                    inner: inner_start..next_close,
                    end: next_close + close.len(),
                    self_closing: false,
                });
            }
        }
    }
}
