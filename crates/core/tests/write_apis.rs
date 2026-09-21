//! T2: no `cos` write API is called from `core` or from any plugin.
//!
//! Every write goes through the transaction. `cos`'s own edit map is not read
//! by the section builder, so a call into it compiles and half-works: a page
//! dictionary is saved and the objects it names are not, or an import's
//! objects never reach the file at all. P11's importer is the package that
//! would have broken this, which is why the check landed with it.
//!
//! `core`'s own transaction writes with `put_object`, a name `cos` does not
//! use, so the check can ban `cos`'s five write methods by name everywhere
//! without an allow-list that would have to know which receiver is which.

use std::path::{Path, PathBuf};

/// Parsed, not grepped: these names appear in doc comments explaining why they
/// are not called, and a comment is not a call.
#[test]
fn nothing_in_core_or_any_plugin_calls_a_cos_write_api() {
    const FORBIDDEN: [&str; 5] = [
        "add_object",
        "set_object",
        "delete_object",
        "set_trailer_entry",
        "set_info_field",
    ];
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut roots = vec![workspace.join("crates/core/src")];
    for plugin in std::fs::read_dir(workspace.join("plugins")).expect("plugins") {
        roots.push(plugin.expect("entry").path().join("src"));
    }

    let mut checked = 0;
    let mut found = Vec::new();
    for file in roots.iter().flat_map(|root| rust_files(root)) {
        let source = std::fs::read_to_string(&file).expect("readable");
        let parsed = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("{} does not parse ({error})", file.display()));
        for name in calls(&parsed) {
            if FORBIDDEN.contains(&name.as_str()) {
                found.push(format!("{}: {name}", file.display()));
            }
        }
        checked += 1;
    }
    assert!(
        found.is_empty(),
        "cos write APIs called:\n{}",
        found.join("\n")
    );
    assert!(
        checked > 60,
        "core and every plugin: only {checked} files were checked"
    );
}

/// The scanner itself, against calls it must find and a comment it must not.
#[test]
fn the_call_scanner_sees_calls_and_not_comments() {
    let file = syn::parse_file(
        "/// add_object is not called here\nfn f(d: &mut D) { d.add_object(1); D::delete_object(d); }",
    )
    .expect("parses");
    assert_eq!(calls(&file), ["add_object", "delete_object"]);
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            files.push(path);
        }
    }
    files
}

/// The name of every method called and every path-qualified function called.
fn calls(file: &syn::File) -> Vec<String> {
    use syn::visit::Visit;

    #[derive(Default)]
    struct Calls(Vec<String>);
    impl<'ast> Visit<'ast> for Calls {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            self.0.push(call.method.to_string());
            syn::visit::visit_expr_method_call(self, call);
        }
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = &*call.func {
                if let Some(last) = path.path.segments.last() {
                    self.0.push(last.ident.to_string());
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let mut calls = Calls::default();
    calls.visit_file(file);
    calls.0
}
