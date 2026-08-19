//! A small document tree for the game's GameData XML.
//!
//! The applier in [`crate::apply`] wants to look at a file the way the game does - a root
//! element whose children are table operations - so this parses the whole file into a plain
//! tree first and keeps the streaming details of quick-xml out of the semantics code. Mod
//! update files are at most a few megabytes; holding one as a tree is nothing.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// One XML element: name, attributes in document order, child elements in document order,
/// and the concatenated character data directly inside it.
///
/// Leaf value elements (`<Cost>50</Cost>`) carry their value in `text`; container elements
/// collect only the whitespace between their children there, which nobody reads.
pub(crate) struct Element {
    pub(crate) name: String,
    pub(crate) attributes: Vec<(String, String)>,
    pub(crate) children: Vec<Element>,
    pub(crate) text: String,
}

impl Element {
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Parse a whole update file into its root element.
///
/// Tolerates what mods actually ship: a UTF-8 BOM, an XML declaration, comments and
/// processing instructions anywhere. Errors are the raw detail half of a [`BoundaryError`];
/// the caller names the file.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("file is not valid UTF-8: {error}"))?;

    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        let event = reader.read_event().map_err(|error| {
            format!("XML error near byte {}: {error}", reader.buffer_position())
        })?;
        match event {
            Event::Start(start) => stack.push(element_from(&start)?),
            Event::Empty(start) => {
                let element = element_from(&start)?;
                attach(element, &mut stack, &mut root)?;
            }
            Event::End(_) => {
                let element = stack
                    .pop()
                    .ok_or_else(|| "unbalanced closing tag".to_string())?;
                attach(element, &mut stack, &mut root)?;
            }
            Event::Text(data) => {
                let value = data
                    .xml_content()
                    .map_err(|error| format!("bad character data: {error}"))?;
                match stack.last_mut() {
                    Some(open) => open.text.push_str(&value),
                    None if value.trim().is_empty() => {}
                    None => return Err("character data outside the root element".to_string()),
                }
            }
            Event::CData(data) => {
                let value = String::from_utf8(data.into_inner().into_owned())
                    .map_err(|error| format!("CDATA is not valid UTF-8: {error}"))?;
                match stack.last_mut() {
                    Some(open) => open.text.push_str(&value),
                    None => return Err("CDATA outside the root element".to_string()),
                }
            }
            Event::GeneralRef(reference) => {
                let resolved = resolve_reference(&reference)?;
                match stack.last_mut() {
                    Some(open) => open.text.push(resolved),
                    None => return Err("entity reference outside the root element".to_string()),
                }
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
            Event::Eof => break,
        }
    }

    if !stack.is_empty() {
        return Err("the file ends with elements still open".to_string());
    }
    root.ok_or_else(|| "the file has no root element".to_string())
}

fn element_from(start: &BytesStart<'_>) -> Result<Element, String> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|error| format!("element name is not valid UTF-8: {error}"))?
        .to_string();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| format!("bad attribute on <{name}>: {error}"))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| format!("attribute name on <{name}> is not valid UTF-8: {error}"))?
            .to_string();
        let value = attribute
            .unescape_value()
            .map_err(|error| format!("bad value for {key} on <{name}>: {error}"))?
            .into_owned();
        attributes.push((key, value));
    }
    Ok(Element {
        name,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}

fn attach(
    element: Element,
    stack: &mut [Element],
    root: &mut Option<Element>,
) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(element);
        return Ok(());
    }
    if root.is_some() {
        return Err("more than one root element".to_string());
    }
    *root = Some(element);
    Ok(())
}

/// Resolve `&#NN;` / `&#xHH;` and the five predefined entities - everything the game's own
/// XML would ever contain.
fn resolve_reference(name: &[u8]) -> Result<char, String> {
    match name {
        b"amp" => return Ok('&'),
        b"lt" => return Ok('<'),
        b"gt" => return Ok('>'),
        b"quot" => return Ok('"'),
        b"apos" => return Ok('\''),
        _ => {}
    }
    if let Some(number) = name.strip_prefix(b"#") {
        let (digits, radix) = match number.strip_prefix(b"x").or(number.strip_prefix(b"X")) {
            Some(hex) => (hex, 16),
            None => (number, 10),
        };
        let digits =
            std::str::from_utf8(digits).map_err(|_| "bad character reference".to_string())?;
        let code = u32::from_str_radix(digits, radix)
            .map_err(|_| format!("bad character reference &#{digits};"))?;
        return char::from_u32(code).ok_or_else(|| format!("bad character reference &#{digits};"));
    }
    Err(format!(
        "unknown entity &{};",
        String::from_utf8_lossy(name)
    ))
}
