//! PDF JavaScript for forms: the Acrobat forms API subset - field
//! calculation, validation and formatting - on a pure-Rust engine,
//! sandboxed with no I/O, no network and a fuel budget. Real AcroForms
//! compute, so a form whose scripts never run is filled wrong; a form
//! whose scripts fail gets a visible notice rather than silent wrong
//! values. Document-level and interactive JS beyond forms is out of scope.
