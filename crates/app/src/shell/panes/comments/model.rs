//! What the Comments pane lists, worked out from the annotations the
//! document carries, with no drawing in it.
//!
//! A **comment** is an annotation that answers nothing. A **reply** answers
//! one (`/IRT`) and has no `/State`. A **status** answers one and carries a
//! `/State`: Acrobat's way of recording Accepted, Rejected and the checkmark,
//! so a comment's status is its newest status answer in each model. A reply
//! whose comment is not in the document any more, which a page deleted
//! elsewhere leaves behind, is listed apart rather than dropped: it is still
//! something someone wrote.

use std::collections::BTreeMap;

use onionskin_core::{ObjRef, ReadAnnotation};

/// How the list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum CommentSort {
    #[default]
    Page,
    Author,
    Date,
    Type,
}

impl CommentSort {
    pub(in crate::shell) const ALL: [Self; 4] = [Self::Page, Self::Author, Self::Date, Self::Type];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Page => "Page",
            Self::Author => "Author",
            Self::Date => "Date",
            Self::Type => "Type",
        }
    }
}

/// What the list is narrowed to. `None` in a field lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::shell) struct CommentFilter {
    pub(in crate::shell) kind: Option<String>,
    pub(in crate::shell) author: Option<String>,
    pub(in crate::shell) status: Option<String>,
}

/// One comment as the pane shows it.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct Thread {
    pub(in crate::shell) comment: ReadAnnotation,
    pub(in crate::shell) replies: Vec<ReadAnnotation>,
    /// The newest `Review` state, if anyone set one.
    pub(in crate::shell) status: Option<String>,
    /// Whether the newest `Marked` state is `Marked`: the checkmark.
    pub(in crate::shell) checked: bool,
}

/// The pane's list: comments with their replies, and replies whose comment
/// is gone.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::shell) struct Listing {
    pub(in crate::shell) threads: Vec<Thread>,
    pub(in crate::shell) orphans: Vec<ReadAnnotation>,
}

/// What kind of comment this is, in the words the pane uses.
pub(in crate::shell) fn kind(annotation: &ReadAnnotation) -> String {
    match annotation.raw_subtype.as_str() {
        "Text" => "Note".to_owned(),
        "FreeText" => "Text Box".to_owned(),
        "StrikeOut" => "Strikethrough".to_owned(),
        "Square" => "Rectangle".to_owned(),
        "Circle" => "Oval".to_owned(),
        "PolyLine" => "Connected Lines".to_owned(),
        "Ink" => "Drawing".to_owned(),
        "FileAttachment" => "Attachment".to_owned(),
        other => other.to_owned(),
    }
}

/// Annotations the pane never lists: a form field's widget, a link and a
/// pop-up are not comments.
fn is_comment_kind(annotation: &ReadAnnotation) -> bool {
    !matches!(
        annotation.raw_subtype.as_str(),
        "Widget" | "Link" | "Popup" | "PrinterMark" | "TrapNet" | "Watermark" | "3D" | "Screen"
    )
}

/// Build the list from every annotation the document carries.
pub(in crate::shell) fn listing(
    annotations: &[ReadAnnotation],
    sort: CommentSort,
    filter: &CommentFilter,
) -> Listing {
    let listed: Vec<&ReadAnnotation> = annotations.iter().filter(|a| is_comment_kind(a)).collect();
    let present: std::collections::BTreeSet<ObjRef> = listed.iter().map(|a| a.objref).collect();

    let mut answers: BTreeMap<ObjRef, Vec<&ReadAnnotation>> = BTreeMap::new();
    let mut orphans = Vec::new();
    let mut comments = Vec::new();
    for annotation in &listed {
        match annotation.in_reply_to {
            Some(parent) if present.contains(&parent) => {
                answers.entry(parent).or_default().push(annotation)
            }
            Some(_) => {
                if annotation.state.is_none() {
                    orphans.push((*annotation).clone());
                }
            }
            None => comments.push(*annotation),
        }
    }

    let mut threads: Vec<Thread> = comments
        .into_iter()
        .map(|comment| {
            let answers = answers.remove(&comment.objref).unwrap_or_default();
            let newest = |model: &str| {
                answers
                    .iter()
                    .filter(|answer| {
                        answer
                            .state
                            .as_ref()
                            .is_some_and(|(_, answer_model)| answer_model == model)
                    })
                    .max_by(|a, b| a.modified.cmp(&b.modified))
                    .and_then(|answer| answer.state.as_ref().map(|(state, _)| state.clone()))
            };
            let status = newest("Review").filter(|state| state != "None");
            let checked = newest("Marked").is_some_and(|state| state == "Marked");
            let replies = answers
                .iter()
                .filter(|answer| answer.state.is_none())
                .map(|answer| (*answer).clone())
                .collect();
            Thread {
                comment: comment.clone(),
                replies,
                status,
                checked,
            }
        })
        .filter(|thread| {
            filter
                .kind
                .as_ref()
                .is_none_or(|wanted| &kind(&thread.comment) == wanted)
                && filter
                    .author
                    .as_ref()
                    .is_none_or(|wanted| thread.comment.author.as_ref() == Some(wanted))
                && filter.status.as_ref().is_none_or(|wanted| {
                    thread.status.as_deref().unwrap_or("None") == wanted.as_str()
                })
        })
        .collect();

    // Stable, so comments equal on the key stay in page and /Annots order.
    match sort {
        CommentSort::Page => {}
        CommentSort::Author => threads.sort_by(|a, b| a.comment.author.cmp(&b.comment.author)),
        // Newest first, the way a review is read.
        CommentSort::Date => threads.sort_by(|a, b| b.comment.modified.cmp(&a.comment.modified)),
        CommentSort::Type => threads.sort_by_key(|thread| kind(&thread.comment)),
    }
    Listing { threads, orphans }
}

/// Every value a filter can take for `field`, from the comments present, so
/// a filter never offers something that would empty the list.
pub(in crate::shell) fn filter_values(
    annotations: &[ReadAnnotation],
    field: FilterField,
) -> Vec<String> {
    let listing = listing(annotations, CommentSort::Page, &CommentFilter::default());
    let mut values: Vec<String> = listing
        .threads
        .iter()
        .filter_map(|thread| match field {
            FilterField::Kind => Some(kind(&thread.comment)),
            FilterField::Author => thread.comment.author.clone(),
            FilterField::Status => Some(thread.status.clone().unwrap_or_else(|| "None".into())),
        })
        .collect();
    values.sort();
    values.dedup();
    values
}

/// Which filter a control changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FilterField {
    Kind,
    Author,
    Status,
}

impl FilterField {
    pub(in crate::shell) const ALL: [Self; 3] = [Self::Kind, Self::Author, Self::Status];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Kind => "Type",
            Self::Author => "Author",
            Self::Status => "Status",
        }
    }
}

impl CommentFilter {
    /// Step `field` to the next value `values` offers, and after the last one
    /// back to letting everything through.
    pub(in crate::shell) fn cycle(&mut self, field: FilterField, values: &[String]) {
        let slot = match field {
            FilterField::Kind => &mut self.kind,
            FilterField::Author => &mut self.author,
            FilterField::Status => &mut self.status,
        };
        *slot = match slot.as_ref() {
            None => values.first().cloned(),
            Some(current) => values
                .iter()
                .position(|value| value == current)
                .and_then(|at| values.get(at + 1))
                .cloned(),
        };
    }

    pub(in crate::shell) fn value(&self, field: FilterField) -> Option<&str> {
        match field {
            FilterField::Kind => self.kind.as_deref(),
            FilterField::Author => self.author.as_deref(),
            FilterField::Status => self.status.as_deref(),
        }
    }
}

/// A PDF date, `D:YYYYMMDDHHmmSS...`, as the pane shows it. Anything else is
/// shown as written.
pub(in crate::shell) fn display_date(raw: &str) -> String {
    let digits = raw.trim_start_matches("D:");
    if digits.len() >= 12 && digits[..12].bytes().all(|b| b.is_ascii_digit()) {
        format!(
            "{}-{}-{} {}:{}",
            &digits[0..4],
            &digits[4..6],
            &digits[6..8],
            &digits[8..10],
            &digits[10..12]
        )
    } else {
        raw.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use onionskin_core::{Flags, Rect, Subtype};

    use super::*;

    fn annotation(
        number: u32,
        page: usize,
        subtype: &str,
        author: &str,
        modified: &str,
    ) -> ReadAnnotation {
        ReadAnnotation {
            objref: ObjRef::new(number, 0),
            page,
            subtype: None::<Subtype>,
            raw_subtype: subtype.to_owned(),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            quads: Vec::new(),
            contents: Some(format!("comment {number}")),
            author: Some(author.to_owned()),
            modified: Some(modified.to_owned()),
            color: None,
            flags: Flags(4),
            in_reply_to: None,
            has_appearance: true,
            ink: Vec::new(),
            border_width: 1.0,
            subject: None,
            state: None,
            opacity: None,
        }
    }

    fn answer(
        number: u32,
        parent: u32,
        modified: &str,
        state: Option<(&str, &str)>,
    ) -> ReadAnnotation {
        let mut answer = annotation(number, 0, "Text", "Ana", modified);
        answer.in_reply_to = Some(ObjRef::new(parent, 0));
        answer.state = state.map(|(state, model)| (state.to_owned(), model.to_owned()));
        answer
    }

    fn fixture() -> Vec<ReadAnnotation> {
        vec![
            annotation(10, 0, "Highlight", "Zoe", "D:20260103000000"),
            annotation(11, 0, "Text", "Ana", "D:20260101000000"),
            annotation(12, 1, "Square", "Max", "D:20260102000000"),
            annotation(13, 1, "Link", "Max", "D:20260102000000"),
            answer(20, 11, "D:20260104000000", None),
            answer(21, 11, "D:20260105000000", Some(("Rejected", "Review"))),
            answer(22, 11, "D:20260106000000", Some(("Accepted", "Review"))),
            answer(23, 12, "D:20260107000000", Some(("Marked", "Marked"))),
            answer(24, 99, "D:20260108000000", None),
        ]
    }

    fn numbers(listing: &Listing) -> Vec<u32> {
        listing
            .threads
            .iter()
            .map(|thread| thread.comment.objref.number)
            .collect()
    }

    #[test]
    fn comments_carry_their_replies_and_newest_status_and_links_are_not_comments() {
        let listing = listing(&fixture(), CommentSort::Page, &CommentFilter::default());
        assert_eq!(numbers(&listing), [10, 11, 12], "the link is left out");
        let note = &listing.threads[1];
        assert_eq!(note.replies.len(), 1, "status answers are not replies");
        assert_eq!(note.status.as_deref(), Some("Accepted"), "the newest wins");
        assert!(listing.threads[2].checked);
        assert!(!note.checked);
    }

    /// A reply whose comment is gone is listed, apart, with its own text.
    #[test]
    fn a_reply_whose_comment_is_gone_is_listed_as_an_orphan() {
        let listing = listing(&fixture(), CommentSort::Page, &CommentFilter::default());
        assert_eq!(listing.orphans.len(), 1);
        assert_eq!(listing.orphans[0].contents.as_deref(), Some("comment 24"));
    }

    #[test]
    fn each_sort_orders_the_list_by_its_key() {
        let all = CommentFilter::default();
        let by = |sort| numbers(&listing(&fixture(), sort, &all));
        assert_eq!(by(CommentSort::Page), [10, 11, 12]);
        assert_eq!(by(CommentSort::Author), [11, 12, 10], "Ana, Max, Zoe");
        assert_eq!(by(CommentSort::Date), [10, 12, 11], "newest first");
        assert_eq!(
            by(CommentSort::Type),
            [10, 11, 12],
            "Highlight, Note, Rectangle"
        );
    }

    #[test]
    fn each_filter_narrows_the_list_and_cycles_back_to_everything() {
        let annotations = fixture();
        let mut filter = CommentFilter::default();
        let authors = filter_values(&annotations, FilterField::Author);
        assert_eq!(authors, ["Ana", "Max", "Zoe"]);
        filter.cycle(FilterField::Author, &authors);
        assert_eq!(
            numbers(&listing(&annotations, CommentSort::Page, &filter)),
            [11]
        );
        filter.cycle(FilterField::Author, &authors);
        filter.cycle(FilterField::Author, &authors);
        filter.cycle(FilterField::Author, &authors);
        assert_eq!(filter.author, None, "past the last value, everything again");

        filter.status = Some("Accepted".into());
        assert_eq!(
            numbers(&listing(&annotations, CommentSort::Page, &filter)),
            [11]
        );
        filter.status = Some("None".into());
        assert_eq!(
            numbers(&listing(&annotations, CommentSort::Page, &filter)),
            [10, 12]
        );
        filter.status = None;
        filter.kind = Some("Rectangle".into());
        assert_eq!(
            numbers(&listing(&annotations, CommentSort::Page, &filter)),
            [12]
        );
    }

    #[test]
    fn a_pdf_date_reads_as_a_date() {
        assert_eq!(display_date("D:20260921143000+02'00'"), "2026-09-21 14:30");
        assert_eq!(display_date("yesterday"), "yesterday");
    }
}
