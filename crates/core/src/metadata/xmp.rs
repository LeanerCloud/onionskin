//! The XMP packet: a reader for the Dublin Core, PDF and XMP basic schemas,
//! and a writer that sets those and keeps everything else.
//!
//! **The reader parses the packet.** It is what the metadata round trip is
//! asserted with, so it must not be able to agree with the writer by
//! construction: it reads XML, following namespaces rather than prefixes,
//! and accepts a property written as an element or as an attribute of
//! `rdf:Description`, which are the two forms other writers use.
//!
//! **The writer keeps what it does not own.** A packet from another producer
//! carries schemas this module knows nothing about - rights, PDF/A
//! identification, media management - and dropping them to rewrite the four
//! fields a user edited would be the quiet damage the review risk names. Every
//! property outside the three schemas written here is copied into the new
//! packet as it was, with the namespaces it needs.

use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(crate) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub(crate) const DC: &str = "http://purl.org/dc/elements/1.1/";
pub(crate) const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
pub(crate) const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const OWNED: [&str; 3] = [DC, PDF, XMP];

/// What an XMP packet says, in the fields the Description tab shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmpFields {
    /// `dc:title`, its default-language alternative.
    pub title: Option<String>,
    /// `dc:creator`, in order.
    pub authors: Vec<String>,
    /// `dc:description`, its default-language alternative.
    pub subject: Option<String>,
    /// `pdf:Keywords`.
    pub keywords: Option<String>,
    /// `pdf:Producer`.
    pub producer: Option<String>,
    /// `xmp:CreatorTool`.
    pub creator_tool: Option<String>,
    /// `xmp:CreateDate`, as the packet writes it.
    pub created: Option<String>,
    /// `xmp:ModifyDate`, as the packet writes it.
    pub modified: Option<String>,
}

/// Parse a packet. `None` for bytes that are not XML with an `rdf:RDF`.
pub fn read(packet: &[u8]) -> Option<XmpFields> {
    let text = std::str::from_utf8(strip_bom(packet)).ok()?;
    let document = roxmltree::Document::parse(xml_only(text)).ok()?;
    let mut fields = XmpFields::default();
    for description in descriptions(&document) {
        read_attributes(&description, &mut fields);
        for property in description.children().filter(|node| node.is_element()) {
            read_property(&property, &mut fields);
        }
    }
    Some(fields)
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// The packet without its `<?xpacket?>` wrapper, which some writers put
/// outside the root element in a way an XML parser rejects.
fn xml_only(text: &str) -> &str {
    let start = text.find("<x:xmpmeta").or_else(|| text.find("<rdf:RDF"));
    let Some(start) = start else {
        return text;
    };
    let end = text
        .rfind("</x:xmpmeta>")
        .map(|at| at + "</x:xmpmeta>".len())
        .or_else(|| text.rfind("</rdf:RDF>").map(|at| at + "</rdf:RDF>".len()))
        .unwrap_or(text.len());
    &text[start..end.max(start)]
}

fn descriptions<'a, 'input>(
    document: &'a roxmltree::Document<'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    document
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "Description")))
}

fn read_attributes(description: &roxmltree::Node, fields: &mut XmpFields) {
    for attribute in description.attributes() {
        let Some(namespace) = attribute.namespace() else {
            continue;
        };
        set_simple(fields, namespace, attribute.name(), attribute.value());
    }
}

fn read_property(property: &roxmltree::Node, fields: &mut XmpFields) {
    let Some(namespace) = property.tag_name().namespace() else {
        return;
    };
    match (namespace, property.tag_name().name()) {
        (DC, "title") => fields.title = alternative(property),
        (DC, "description") => fields.subject = alternative(property),
        (DC, "creator") => fields.authors = list(property),
        (namespace, name) => {
            if let Some(text) = property.text() {
                set_simple(fields, namespace, name, text.trim());
            }
        }
    }
}

fn set_simple(fields: &mut XmpFields, namespace: &str, name: &str, value: &str) {
    let value = Some(value.to_owned());
    match (namespace, name) {
        (PDF, "Keywords") => fields.keywords = value,
        (PDF, "Producer") => fields.producer = value,
        (XMP, "CreatorTool") => fields.creator_tool = value,
        (XMP, "CreateDate") => fields.created = value,
        (XMP, "ModifyDate") => fields.modified = value,
        _ => {}
    }
}

/// An `rdf:Alt`'s default-language item, else its first; or the property's
/// own text when it is written without the array.
fn alternative(property: &roxmltree::Node) -> Option<String> {
    let items: Vec<roxmltree::Node> = property
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "li")))
        .collect();
    let default = items
        .iter()
        .find(|item| item.attribute((XML, "lang")) == Some("x-default"));
    default
        .or(items.first())
        .map(|item| item.text().unwrap_or("").to_owned())
        .or_else(|| property.text().map(|text| text.trim().to_owned()))
}

/// An `rdf:Seq` or `rdf:Bag`'s items, in order.
fn list(property: &roxmltree::Node) -> Vec<String> {
    property
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "li")))
        .map(|item| item.text().unwrap_or("").to_owned())
        .collect()
}

/// A packet setting `fields` and keeping every other property `previous`
/// carried.
pub fn write(fields: &XmpFields, previous: Option<&[u8]>) -> Vec<u8> {
    let kept = previous.map(kept_properties).unwrap_or_default();
    let mut namespaces: BTreeMap<String, String> = kept.namespaces;
    for (prefix, uri) in [("dc", DC), ("pdf", PDF), ("xmp", XMP)] {
        namespaces.insert(prefix.to_owned(), uri.to_owned());
    }

    let mut out = String::from("<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n");
    let _ = writeln!(out, " <rdf:RDF xmlns:rdf=\"{RDF}\">");
    out.push_str("  <rdf:Description rdf:about=\"\"");
    for (prefix, uri) in &namespaces {
        let _ = write!(out, "\n    xmlns:{prefix}=\"{}\"", escape(uri));
    }
    for (name, value) in &kept.attributes {
        let _ = write!(out, "\n    {name}=\"{}\"", escape(value));
    }
    out.push_str(">\n");
    write_fields(&mut out, fields);
    for element in &kept.elements {
        let _ = writeln!(out, "   {element}");
    }
    out.push_str("  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n");
    // Room for another writer to edit the packet in place, which is what the
    // padding in every XMP packet is for.
    for _ in 0..20 {
        out.push_str(&" ".repeat(99));
        out.push('\n');
    }
    out.push_str("<?xpacket end=\"w\"?>");
    out.into_bytes()
}

fn write_fields(out: &mut String, fields: &XmpFields) {
    if let Some(title) = &fields.title {
        let _ = writeln!(
            out,
            "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>",
            escape(title)
        );
    }
    if !fields.authors.is_empty() {
        out.push_str("   <dc:creator><rdf:Seq>");
        for author in &fields.authors {
            let _ = write!(out, "<rdf:li>{}</rdf:li>", escape(author));
        }
        out.push_str("</rdf:Seq></dc:creator>\n");
    }
    if let Some(subject) = &fields.subject {
        let _ = writeln!(
            out,
            "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>",
            escape(subject)
        );
    }
    for (element, value) in [
        ("pdf:Keywords", &fields.keywords),
        ("pdf:Producer", &fields.producer),
        ("xmp:CreatorTool", &fields.creator_tool),
        ("xmp:CreateDate", &fields.created),
        ("xmp:ModifyDate", &fields.modified),
        ("xmp:MetadataDate", &fields.modified),
    ] {
        if let Some(value) = value {
            let _ = writeln!(out, "   <{element}>{}</{element}>", escape(value));
        }
    }
}

/// What a previous packet carried outside the schemas written here.
#[derive(Default)]
struct Kept {
    namespaces: BTreeMap<String, String>,
    attributes: Vec<(String, String)>,
    elements: Vec<String>,
}

fn kept_properties(previous: &[u8]) -> Kept {
    let mut kept = Kept::default();
    let Ok(text) = std::str::from_utf8(strip_bom(previous)) else {
        return kept;
    };
    let xml = xml_only(text);
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return kept;
    };
    for description in descriptions(&document) {
        for namespace in description.namespaces() {
            if let Some(prefix) = namespace.name() {
                if !["x", "rdf", "xml"].contains(&prefix) && !OWNED.contains(&namespace.uri()) {
                    kept.namespaces
                        .insert(prefix.to_owned(), namespace.uri().to_owned());
                }
            }
        }
        for attribute in description.attributes() {
            let Some(namespace) = attribute.namespace() else {
                continue;
            };
            if OWNED.contains(&namespace) || namespace == RDF {
                continue;
            }
            if let Some(prefix) = description.lookup_prefix(namespace) {
                kept.attributes.push((
                    format!("{prefix}:{}", attribute.name()),
                    attribute.value().to_owned(),
                ));
            }
        }
        for property in description.children().filter(|node| node.is_element()) {
            let owned = property
                .tag_name()
                .namespace()
                .is_some_and(|namespace| OWNED.contains(&namespace));
            if !owned {
                kept.elements.push(xml[property.range()].to_owned());
            }
        }
    }
    kept
}

/// Text for an element or an attribute value.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            character if (character as u32) < 0x20 && !matches!(character, '\t' | '\n' | '\r') => {}
            character => out.push(character),
        }
    }
    out
}

/// A PDF date, `D:YYYYMMDDHHmmSS` and an optional offset, as an XMP date:
/// `YYYY-MM-DDTHH:mm:SS` and `Z` or `+HH:mm`. `None` for anything else.
pub fn xmp_date(pdf: &str) -> Option<String> {
    let text = pdf.strip_prefix("D:").unwrap_or(pdf);
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |from: usize, default: &'static str| digits.get(from..from + 2).unwrap_or(default);
    let mut out = format!(
        "{}-{}-{}T{}:{}:{}",
        &digits[0..4],
        part(4, "01"),
        part(6, "01"),
        part(8, "00"),
        part(10, "00"),
        part(12, "00")
    );
    let rest = &text[digits.len()..];
    match rest.chars().next() {
        Some('+' | '-') => {
            let offset: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
            let hours = offset.get(0..2).unwrap_or("00");
            let minutes = offset.get(2..4).unwrap_or("00");
            let _ = write!(out, "{}{hours}:{minutes}", &rest[..1]);
        }
        _ => out.push('Z'),
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet the way another producer writes one: attribute-form
    /// properties, a `dc:creator` bag, and a schema this module does not own.
    const FOREIGN: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
  xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/"
  pdf:Producer="Some Writer 1.0" xmpRights:Marked="True">
</rdf:Description>
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
<dc:title><rdf:Alt><rdf:li xml:lang="de">Bericht</rdf:li><rdf:li xml:lang="x-default">Report</rdf:li></rdf:Alt></dc:title>
<dc:creator><rdf:Bag><rdf:li>Ana</rdf:li><rdf:li>Bo</rdf:li></rdf:Bag></dc:creator>
<pdfaid:part>2</pdfaid:part>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    #[test]
    fn another_writers_packet_reads_by_namespace_in_either_form() {
        let fields = read(FOREIGN.as_bytes()).expect("parses");
        assert_eq!(
            fields.title.as_deref(),
            Some("Report"),
            "the default language"
        );
        assert_eq!(fields.authors, ["Ana", "Bo"]);
        assert_eq!(
            fields.producer.as_deref(),
            Some("Some Writer 1.0"),
            "an attribute"
        );
    }

    #[test]
    fn what_is_written_reads_back_and_what_was_not_owned_is_kept() {
        let fields = XmpFields {
            title: Some("Q3 <draft> & notes".into()),
            authors: vec!["Ana".into()],
            subject: Some("Figures".into()),
            keywords: Some("budget, q3".into()),
            producer: Some("Some Writer 1.0".into()),
            creator_tool: None,
            created: Some("2026-01-02T03:04:05Z".into()),
            modified: Some("2026-09-21T14:05:00Z".into()),
        };
        let packet = write(&fields, Some(FOREIGN.as_bytes()));
        assert_eq!(read(&packet), Some(fields));
        let text = String::from_utf8(packet).expect("utf-8");
        assert!(text.contains("<pdfaid:part>2</pdfaid:part>"), "{text}");
        assert!(text.contains("xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\""));
        assert!(text.contains("xmpRights:Marked=\"True\""));
        assert!(
            !text.contains("Bericht"),
            "an owned property is replaced, not kept"
        );
        assert!(text.starts_with("<?xpacket begin=\"\u{FEFF}\""));
        assert!(text.ends_with("<?xpacket end=\"w\"?>"));
    }

    #[test]
    fn a_packet_that_is_not_xmp_reads_as_nothing() {
        assert_eq!(read(b"not xml at all"), None);
        assert_eq!(read(&[0xFF, 0xFE, 0x00]), None);
    }

    #[test]
    fn pdf_dates_become_xmp_dates() {
        assert_eq!(
            xmp_date("D:20260921140500Z").as_deref(),
            Some("2026-09-21T14:05:00Z")
        );
        assert_eq!(
            xmp_date("D:20260921140500+02'00'").as_deref(),
            Some("2026-09-21T14:05:00+02:00")
        );
        assert_eq!(xmp_date("D:2026").as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(xmp_date("yesterday"), None);
    }
}
