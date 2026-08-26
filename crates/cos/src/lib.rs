//! COS object layer: lexer and parser, xref tables and streams, object
//! streams, stream filters and encryption. Parsing is lazy by
//! construction - objects resolve on demand through the xref, never the
//! whole file up front - and repairing like Acrobat, so a broken xref,
//! junk before the header or a truncated tail all recover through a
//! scan-and-rebuild path. Every object keeps its source byte span. The
//! writer emits incremental-update sections; a full rewrite exists only
//! behind an explicit flatten API.
