//! Which comments the user has read, for this session only.
//!
//! Acrobat marks a comment read once it has been looked at and lets the
//! user mark it unread again. That is the reader's own state, not the
//! document's: it is never written to the file, where it would change the
//! bytes of every document anyone opened. It lives with the document's
//! canvas so it survives switching tabs, and it goes when the tab closes.

use std::collections::BTreeSet;

use onionskin_core::{ObjRef, ReadAnnotation};

use super::CanvasModel;

/// The session's read marks. A comment the user signed is read without
/// being opened: they wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentReads {
    read: BTreeSet<ObjRef>,
    unread: BTreeSet<ObjRef>,
}

impl CommentReads {
    /// Whether `comment` counts as read for a user signing as `author`.
    pub fn is_read(&self, comment: &ReadAnnotation, author: Option<&str>) -> bool {
        if self.unread.contains(&comment.objref) {
            return false;
        }
        self.read.contains(&comment.objref)
            || author.is_some_and(|author| comment.author.as_deref() == Some(author))
    }

    /// Mark `comment` read or unread, whichever the user chose last.
    pub fn set(&mut self, comment: ObjRef, read: bool) {
        if read {
            self.unread.remove(&comment);
            self.read.insert(comment);
        } else {
            self.read.remove(&comment);
            self.unread.insert(comment);
        }
    }
}

impl CanvasModel {
    /// The session's read marks for this document's comments.
    pub fn comment_reads(&self) -> &CommentReads {
        &self.comment_reads
    }

    pub fn set_comment_read(&mut self, comment: ObjRef, read: bool) {
        self.comment_reads.set(comment, read);
    }
}

#[cfg(test)]
mod tests {
    use onionskin_core::{Flags, Rect, Subtype};

    use super::*;

    fn comment(number: u32, author: &str) -> ReadAnnotation {
        ReadAnnotation {
            objref: ObjRef::new(number, 0),
            page: 0,
            subtype: None::<Subtype>,
            raw_subtype: "Text".into(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            quads: Vec::new(),
            contents: None,
            author: Some(author.into()),
            modified: None,
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

    #[test]
    fn someone_elses_comment_is_unread_until_marked_and_can_be_unread_again() {
        let mut reads = CommentReads::default();
        let theirs = comment(7, "Zoe");
        assert!(!reads.is_read(&theirs, Some("Ana")));
        reads.set(theirs.objref, true);
        assert!(reads.is_read(&theirs, Some("Ana")));
        reads.set(theirs.objref, false);
        assert!(!reads.is_read(&theirs, Some("Ana")));
    }

    #[test]
    fn the_users_own_comment_is_read_unless_they_mark_it_unread() {
        let mut reads = CommentReads::default();
        let mine = comment(8, "Ana");
        assert!(reads.is_read(&mine, Some("Ana")));
        assert!(
            !reads.is_read(&mine, None),
            "no name, no way to know it is theirs"
        );
        reads.set(mine.objref, false);
        assert!(!reads.is_read(&mine, Some("Ana")));
    }
}
