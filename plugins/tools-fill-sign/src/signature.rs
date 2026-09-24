//! The signature library: the user's signature and initials, kept on disk
//! as one-page PDFs, and made from typed text, a drawing or a PDF page.
//!
//! `<data dir>/signatures/signature.pdf` and `initials.pdf`. A signature is
//! always a page, whatever it was made from, so placing one is placing a
//! page, the way a custom stamp is.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use onionskin_content::{encode_win_ansi, standard_text_width};
use onionskin_cos::{BytesSource, Dict, Document as CosDocument, Name, ObjRef, Object, Stream};

/// Which one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureKind {
    Signature,
    Initials,
}

impl SignatureKind {
    pub const ALL: [SignatureKind; 2] = [SignatureKind::Signature, SignatureKind::Initials];

    /// The id the Sign tool knows it by.
    pub fn id(self) -> &'static str {
        match self {
            SignatureKind::Signature => "signature",
            SignatureKind::Initials => "initials",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SignatureKind::Signature => "Signature",
            SignatureKind::Initials => "Initials",
        }
    }

    fn file(self) -> &'static str {
        match self {
            SignatureKind::Signature => "signature.pdf",
            SignatureKind::Initials => "initials.pdf",
        }
    }

    /// How wide it is placed, at most, in points.
    pub fn max_width(self) -> f64 {
        match self {
            SignatureKind::Signature => 150.0,
            SignatureKind::Initials => 60.0,
        }
    }
}

#[derive(Debug)]
pub enum SignatureError {
    /// Nothing to make one of: no text, no strokes.
    Empty,
    /// The page could not be read, or its PDF is encrypted.
    NotAPage(String),
    Io(std::io::Error),
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "there is nothing to make a signature of"),
            Self::NotAPage(why) => {
                write!(f, "that is not a page a signature can be made of: {why}")
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// Where the signature and the initials live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureLibrary {
    dir: PathBuf,
}

/// The folder inside the tools' data directory.
const DIR: &str = "signatures";

impl SignatureLibrary {
    /// The library in the tools' data directory `data_dir`.
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join(DIR),
        }
    }

    fn path(&self, kind: SignatureKind) -> PathBuf {
        self.dir.join(kind.file())
    }

    /// The saved page for `kind`, if there is one.
    pub fn get(&self, kind: SignatureKind) -> Option<Arc<Vec<u8>>> {
        std::fs::read(self.path(kind)).ok().map(Arc::new)
    }

    /// Keep `pdf` as `kind`, replacing what was kept. Refused unless it is a
    /// page a signature can be placed from.
    pub fn save(&self, kind: SignatureKind, pdf: &[u8]) -> Result<(), SignatureError> {
        page_size(pdf)?;
        std::fs::create_dir_all(&self.dir).map_err(SignatureError::Io)?;
        let partial = self.path(kind).with_extension("pdf.part");
        std::fs::write(&partial, pdf).map_err(SignatureError::Io)?;
        std::fs::rename(&partial, self.path(kind)).map_err(SignatureError::Io)
    }

    /// Forget `kind`. Forgetting one there is not is not an error.
    pub fn clear(&self, kind: SignatureKind) -> Result<(), SignatureError> {
        match std::fs::remove_file(self.path(kind)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(SignatureError::Io(error))
            }
            _ => Ok(()),
        }
    }
}

/// The size, in points, of page 1 of `pdf`.
pub fn page_size(pdf: &[u8]) -> Result<(f64, f64), SignatureError> {
    let not_a_page = |why: String| SignatureError::NotAPage(why);
    let document = CosDocument::open(Box::new(BytesSource::new(pdf.to_vec())))
        .map_err(|error| not_a_page(error.to_string()))?;
    if document.trailer().get(b"Encrypt").is_some() {
        return Err(not_a_page("it is encrypted".to_owned()));
    }
    let page = document
        .page(0)
        .map_err(|error| not_a_page(error.to_string()))?;
    let boxed = page
        .crop_box
        .or(page.media_box)
        .ok_or_else(|| not_a_page("it has no size".to_owned()))?;
    let size = ((boxed[2] - boxed[0]).abs(), (boxed[3] - boxed[1]).abs());
    if size.0 > 0.0 && size.1 > 0.0 {
        Ok(size)
    } else {
        Err(not_a_page("it has no size".to_owned()))
    }
}

/// How a typed signature is set: the most handwriting-like of the standard
/// fonts, at a size a signature is written at.
const TYPED_FONT: &str = "Times-Italic";
const TYPED_SIZE: f64 = 36.0;
/// Space round the ink, in points.
const MARGIN: f64 = 4.0;

/// A signature page with `text` typed on it.
pub fn typed(text: &str) -> Result<Vec<u8>, SignatureError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(SignatureError::Empty);
    }
    let bytes = encode_win_ansi(text);
    let width =
        standard_text_width(TYPED_FONT, &bytes).expect("a standard font") * TYPED_SIZE / 1000.0;
    let mut content = format!(
        "BT /F0 {TYPED_SIZE} Tf 0 g {MARGIN} {} Td (",
        TYPED_SIZE * 0.3
    )
    .into_bytes();
    for byte in bytes {
        if matches!(byte, b'(' | b')' | b'\\') {
            content.push(b'\\');
        }
        content.push(byte);
    }
    content.extend_from_slice(b") Tj ET");
    let mut font = Dict::new();
    font.set(Name::new("Type"), Object::name("Font"));
    font.set(Name::new("Subtype"), Object::name("Type1"));
    font.set(Name::new("BaseFont"), Object::name(TYPED_FONT));
    font.set(Name::new("Encoding"), Object::name("WinAnsiEncoding"));
    let mut fonts = Dict::new();
    fonts.set(Name::new("F0"), Object::Dict(font));
    let mut resources = Dict::new();
    resources.set(Name::new("Font"), Object::Dict(fonts));
    one_page((width + 2.0 * MARGIN, TYPED_SIZE * 1.3), content, resources)
}

/// A signature page of the strokes drawn: each a run of points on a pad
/// whose `y` grows downwards, as a screen's does.
pub fn drawn(strokes: &[Vec<(f64, f64)>]) -> Result<Vec<u8>, SignatureError> {
    let points: Vec<(f64, f64)> = strokes.iter().flatten().copied().collect();
    if points.is_empty() {
        return Err(SignatureError::Empty);
    }
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let place = |(x, y): (f64, f64)| (x - min_x + MARGIN, max_y - y + MARGIN);
    let mut content = String::from("0 G 2 w 1 J 1 j\n");
    for stroke in strokes.iter().filter(|stroke| !stroke.is_empty()) {
        let (x, y) = place(stroke[0]);
        content.push_str(&format!("{x} {y} m\n"));
        // A single point is a dot: a zero-length line with round caps.
        let rest = if stroke.len() == 1 {
            &stroke[..]
        } else {
            &stroke[1..]
        };
        for point in rest {
            let (x, y) = place(*point);
            content.push_str(&format!("{x} {y} l\n"));
        }
        content.push_str("S\n");
    }
    one_page(
        (max_x - min_x + 2.0 * MARGIN, max_y - min_y + 2.0 * MARGIN),
        content.into_bytes(),
        Dict::new(),
    )
}

/// A one-page PDF of `size` drawing `content` with `resources`.
fn one_page(
    size: (f64, f64),
    content: Vec<u8>,
    resources: Dict,
) -> Result<Vec<u8>, SignatureError> {
    let reference = |number| Object::Ref(ObjRef::new(number, 0));
    let mut catalog = Dict::new();
    catalog.set(Name::new("Type"), Object::name("Catalog"));
    catalog.set(Name::new("Pages"), reference(2));
    let mut pages = Dict::new();
    pages.set(Name::new("Type"), Object::name("Pages"));
    pages.set(Name::new("Kids"), Object::Array(vec![reference(3)]));
    pages.set(Name::new("Count"), Object::Integer(1));
    let mut page = Dict::new();
    page.set(Name::new("Type"), Object::name("Page"));
    page.set(Name::new("Parent"), reference(2));
    page.set(
        Name::new("MediaBox"),
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(size.0),
            Object::Real(size.1),
        ]),
    );
    page.set(Name::new("Resources"), Object::Dict(resources));
    page.set(Name::new("Contents"), reference(4));
    let mut stream = Dict::new();
    stream.set(Name::new("Length"), Object::Integer(content.len() as i64));
    let mut trailer = Dict::new();
    trailer.set(Name::new("Root"), reference(1));
    CosDocument::write_new(
        &[
            (ObjRef::new(1, 0), Object::Dict(catalog)),
            (ObjRef::new(2, 0), Object::Dict(pages)),
            (ObjRef::new(3, 0), Object::Dict(page)),
            (
                ObjRef::new(4, 0),
                Object::Stream(Stream {
                    dict: stream,
                    raw: content,
                }),
            ),
        ],
        trailer,
    )
    .map_err(|error| SignatureError::NotAPage(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_signature_is_a_page_as_wide_as_the_name() {
        let pdf = typed("  Ana Pop ").expect("makes");
        let (width, height) = page_size(&pdf).expect("a page");
        assert!(width > 100.0 && width < 200.0, "{width}");
        assert!((height - 46.8).abs() < 0.01);
        assert!(matches!(typed(" "), Err(SignatureError::Empty)));
    }

    #[test]
    fn a_drawn_signature_is_the_strokes_bounds_turned_upright() {
        let pdf =
            drawn(&[vec![(10.0, 10.0), (60.0, 30.0)], vec![(40.0, 5.0)], vec![]]).expect("makes");
        let (width, height) = page_size(&pdf).expect("a page");
        assert_eq!((width, height), (58.0, 33.0));
        assert!(matches!(drawn(&[]), Err(SignatureError::Empty)));
        assert!(matches!(drawn(&[vec![]]), Err(SignatureError::Empty)));
    }

    #[test]
    fn what_is_not_a_page_is_refused_and_named() {
        let refused = page_size(b"nonsense").unwrap_err().to_string();
        assert!(refused.contains("not a page"), "{refused}");
        assert!(SignatureError::Empty.to_string().contains("nothing"));
        assert_eq!(SignatureKind::Initials.max_width(), 60.0);
        assert_eq!(SignatureKind::Signature.label(), "Signature");
    }
}
