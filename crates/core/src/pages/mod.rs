//! `core::pages`: every page-set operation as one transformation.
//!
//! Delete, reorder, insert and import are all the same thing, a new page
//! order, so there is one entry point, [`rewrite_page_tree`], and no separate
//! `delete_page` to get subtly different.
//!
//! # What it produces
//!
//! A single **flat** `/Pages` node reusing the original root's object number,
//! with each surviving page dict rewritten so that its inheritance is
//! materialized, its `/Parent` points at the new node, and everything else on
//! it is carried through untouched. That is the "unimplemented means untouched"
//! rule at the page level: a `/Tabs`, a `/Group`, a `/UserUnit` or a private
//! key this crate has never heard of survives a page-set change unchanged.
//!
//! # What happens to a removed page: nothing
//!
//! **A removed page is removed by not being listed, and nothing else happens to
//! it.** Its dict is not rewritten and not freed; neither are its annotations,
//! their appearance streams, their `/Popup` partners, its content streams, or
//! the internal `/Pages` nodes it hung under. They become one internally
//! consistent garbage subtree that nothing reaches.
//!
//! This is T5's free-nothing rule, and it is the reason **no `delete_object`
//! call appears anywhere in this module**. Freeing them is what manufactures
//! dangling references: an object number handed out again is a reference that
//! used to mean one thing and now means another, and every incremental section
//! ever appended to the file still names the old one.
//!
//! # The seven document-level fix-ups
//!
//! A page-set change breaks seven other parts of a document, and **each is its
//! own module with its own fixture and its own test**, because each walks a
//! different part of the file and no one of them finds another's case:
//!
//! | Module | What it repairs |
//! | --- | --- |
//! | [`labels`] | `/PageLabels`, a number tree keyed on page index |
//! | [`destinations`] | `/Dests` and the `/Names /Dests` **name tree** |
//! | [`outline`] | the bookmark chain: `/Prev`, `/Next`, `/First`, `/Last`, `/Count` |
//! | [`links`] | `/Link` annotations on surviving pages naming a removed one |
//! | [`fields`] | `/AcroForm /Fields` whose widgets were on a removed page |
//! | [`threads`] | article bead rings, re-linked through `/N` and `/V` |
//! | [`actions`] | `/OpenAction` and page-level `/AA` |
//!
//! The `/Count` on the new `/Pages` node is not one of the seven: it is the
//! length of the list.

mod actions;
mod assemble;
mod destinations;
mod fields;
mod import;
mod inherit;
mod labels;
mod links;
mod ops;
mod outline;
mod print_form;
mod rewrite;

pub(crate) use rewrite::{dict_at, resolve};
mod threads;
mod thumbs;
mod tree;

pub use assemble::{Assembled, Assembly, Tagging, Untagged};
pub use import::{extract_pages, import_page_as_form, import_pages};
pub(crate) use ops::{current_page_count, page_ref};
pub use ops::{
    delete_pages, insert_blank_pages, insert_pages_from, move_pages, page_count,
    replace_pages_from, rotate_pages, set_page_labels, LabelRange, LabelStyle,
};
pub use print_form::import_page_for_print;
pub use rewrite::{rewrite_page_tree, PageSource, Rewrite};
pub use thumbs::{embed_thumbnails, remove_thumbnails, THUMBNAIL_SIDE};
