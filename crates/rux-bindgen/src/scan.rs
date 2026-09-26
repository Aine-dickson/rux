//! Reading a whole `native/` crate: every export, with its Rux signature,
//! and the path of the code that registers it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::{ImplItem, Item, Visibility};

use crate::{fn_info, fn_descriptor, is_rux_attr, rux_attr, struct_fields, type_name, METHODS_DESCRIPTOR, TYPE_DESCRIPTOR};

/// One export, by its Rux name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Fn { name: String, params: Vec<(String, String)>, result: String, is_async: bool },
    /// A method of `on`, a type of the same module.
    Method { on: String, name: String, params: Vec<(String, String)>, result: String, is_async: bool },
    /// Fields as name, type and whether it may be left out.
    Record { name: String, fields: Vec<(String, String, bool)> },
    Resource { name: String },
}

/// A Rust module with something exported in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScannedModule {
    /// `""` for the crate root, `shop`, `db::users`.
    pub path: String,
    pub items: Vec<Found>,
    /// The Rust path, from the crate root, of each hidden function that
    /// gives this module's descriptors: `shop::__rux_export_find`.
    pub registrations: Vec<String>,
}

/// Something in the crate that stops an export, where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: PathBuf,
    pub line: usize,
    pub message: String,
}

/// What a crate exports.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub modules: Vec<ScannedModule>,
    /// The Rust paths of the `#[rux::init]` functions.
    pub inits: Vec<String>,
    pub problems: Vec<Problem>,
}

/// Read the crate whose root file is `lib`, following `mod x;` to its file.
/// Never fails: what cannot be read is a [`Problem`].
pub fn scan(lib: &Path) -> Scan {
    let mut s = Scanner { scan: Scan::default(), renames: HashMap::new(), impls: Vec::new() };
    let dir = lib.parent().unwrap_or(Path::new(".")).to_path_buf();
    s.file(lib, "", &dir, true);
    s.finish();
    s.scan.modules.sort_by(|a, b| a.path.cmp(&b.path));
    s.scan
}

/// An exported `impl` block, kept until every type's Rux name is known.
struct PendingImpl {
    module: String,
    self_ty: String,
    file: PathBuf,
    item: syn::ItemImpl,
}

struct Scanner {
    scan: Scan,
    /// A type's Rust name to its Rux name and module, for the impls.
    renames: HashMap<String, (String, String)>,
    impls: Vec<PendingImpl>,
}

fn join(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.to_string()
    } else {
        format!("{module}::{name}")
    }
}

fn is_pub(v: &Visibility) -> bool {
    matches!(v, Visibility::Public(_))
}

impl Scanner {
    fn problem(&mut self, file: &Path, span: proc_macro2::Span, message: impl Into<String>) {
        self.scan.problems.push(Problem { file: file.to_path_buf(), line: span.start().line, message: message.into() });
    }

    fn module(&mut self, path: &str) -> &mut ScannedModule {
        if let Some(i) = self.scan.modules.iter().position(|m| m.path == path) {
            return &mut self.scan.modules[i];
        }
        self.scan.modules.push(ScannedModule { path: path.to_string(), ..Default::default() });
        self.scan.modules.last_mut().expect("pushed")
    }

    fn file(&mut self, path: &Path, module: &str, child_dir: &Path, reachable: bool) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.scan.problems.push(Problem { file: path.to_path_buf(), line: 0, message: format!("reading it: {e}") });
                return;
            }
        };
        match syn::parse_file(&text) {
            Ok(f) => self.items(&f.items, module, child_dir, path, reachable),
            Err(e) => self.problem(path, e.span(), e.to_string()),
        }
    }

    fn items(&mut self, items: &[Item], module: &str, child_dir: &Path, file: &Path, reachable: bool) {
        for item in items {
            match item {
                Item::Mod(m) => {
                    let sub = join(module, &m.ident.to_string());
                    let reach = reachable && is_pub(&m.vis);
                    let dir = child_dir.join(m.ident.to_string());
                    if let Some((_, inner)) = &m.content {
                        self.items(inner, &sub, &dir, file, reach);
                        continue;
                    }
                    if m.attrs.iter().any(|a| a.path().is_ident("path")) {
                        self.problem(file, m.span(), "a `#[path]` module is not read for exports; name the file after the module");
                        continue;
                    }
                    let a = child_dir.join(format!("{}.rs", m.ident));
                    let b = dir.join("mod.rs");
                    match (a.exists(), b.exists()) {
                        (true, _) => self.file(&a, &sub, &dir, reach),
                        (false, true) => self.file(&b, &sub, &dir, reach),
                        _ => self.problem(file, m.span(), format!("no file for `mod {}`: looked for {}", m.ident, a.display())),
                    }
                }
                Item::Fn(f) if f.attrs.iter().any(|a| is_rux_attr(a, "init")) => {
                    if !f.sig.inputs.is_empty() || f.sig.asyncness.is_some() {
                        self.problem(file, f.sig.span(), "a `#[rux::init]` function takes nothing and is not `async`");
                    } else if !(reachable && is_pub(&f.vis)) {
                        self.problem(file, f.sig.span(), unreachable_message("function", &f.sig.ident.to_string()));
                    } else {
                        self.scan.inits.push(join(module, &f.sig.ident.to_string()));
                    }
                }
                Item::Fn(f) if f.attrs.iter().any(|a| is_rux_attr(a, "export")) => {
                    if !(reachable && is_pub(&f.vis)) {
                        self.problem(file, f.sig.span(), unreachable_message("function", &f.sig.ident.to_string()));
                        continue;
                    }
                    match fn_info(&f.sig, &f.attrs, false) {
                        Ok(i) => {
                            let found = Found::Fn {
                                name: i.rux,
                                params: i.params.into_iter().map(|p| (p.name, p.rux)).collect(),
                                result: i.result,
                                is_async: i.is_async,
                            };
                            let reg = join(module, &fn_descriptor(&f.sig.ident.to_string()));
                            let m = self.module(module);
                            m.items.push(found);
                            m.registrations.push(reg);
                        }
                        Err(e) => self.problem(file, e.span(), e.to_string()),
                    }
                }
                Item::Struct(st) => {
                    let export = st.attrs.iter().any(|a| is_rux_attr(a, "export"));
                    let resource = st.attrs.iter().any(|a| is_rux_attr(a, "resource"));
                    if !export && !resource {
                        continue;
                    }
                    if export && resource {
                        self.problem(file, st.ident.span(), "a struct is either `#[rux::export]` (a value) or `#[rux::resource]`, not both");
                        continue;
                    }
                    if !(reachable && is_pub(&st.vis)) {
                        self.problem(file, st.ident.span(), unreachable_message("struct", &st.ident.to_string()));
                        continue;
                    }
                    let name = match type_name(&st.ident, &st.attrs) {
                        Ok(n) => n,
                        Err(e) => {
                            self.problem(file, e.span(), e.to_string());
                            continue;
                        }
                    };
                    let found = if resource {
                        Found::Resource { name: name.clone() }
                    } else {
                        match struct_fields(st) {
                            Ok(fields) => Found::Record {
                                name: name.clone(),
                                fields: fields.into_iter().map(|f| (f.rux, f.rux_ty, f.optional)).collect(),
                            },
                            Err(e) => {
                                self.problem(file, e.span(), e.to_string());
                                continue;
                            }
                        }
                    };
                    self.renames.insert(st.ident.to_string(), (name, module.to_string()));
                    let reg = join(module, &format!("{}::{TYPE_DESCRIPTOR}", st.ident));
                    let m = self.module(module);
                    m.items.push(found);
                    m.registrations.push(reg);
                }
                Item::Impl(im) if im.attrs.iter().any(|a| is_rux_attr(a, "export")) => {
                    let syn::Type::Path(p) = &*im.self_ty else {
                        self.problem(file, im.self_ty.span(), "an exported `impl` is for a struct of this crate");
                        continue;
                    };
                    if im.trait_.is_some() || !im.generics.params.is_empty() {
                        self.problem(file, im.span(), "an exported `impl` is a plain `impl Type`, not a trait's and not generic");
                        continue;
                    }
                    let self_ty = p.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                    self.impls.push(PendingImpl {
                        module: module.to_string(),
                        self_ty,
                        file: file.to_path_buf(),
                        item: im.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    /// The impls, once every type's Rux name is known.
    fn finish(&mut self) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for p in std::mem::take(&mut self.impls) {
            let Some((on, type_module)) = self.renames.get(&p.self_ty).cloned() else {
                self.problem(
                    &p.file,
                    p.item.self_ty.span(),
                    format!("`{}` is not exported: mark it `#[rux::export]` or `#[rux::resource]` before its methods", p.self_ty),
                );
                continue;
            };
            if type_module != p.module {
                self.problem(&p.file, p.item.span(), format!("an exported `impl {}` goes in the module that declares it", p.self_ty));
                continue;
            }
            let count = seen.entry(join(&p.module, &p.self_ty)).or_insert(0);
            *count += 1;
            if *count > 1 {
                self.problem(&p.file, p.item.span(), format!("`{}` has one exported `impl` block; move these methods into it", p.self_ty));
                continue;
            }
            let mut found = Vec::new();
            for it in &p.item.items {
                let ImplItem::Fn(f) = it else { continue };
                if !is_pub(&f.vis) || rux_attr(&f.attrs).map(|a| a.skip).unwrap_or(false) {
                    continue;
                }
                match fn_info(&f.sig, &f.attrs, true) {
                    Ok(i) => {
                        let params = i.params.into_iter().map(|q| (q.name, q.rux)).collect();
                        found.push(if i.method {
                            Found::Method { on: on.clone(), name: i.rux, params, result: i.result, is_async: i.is_async }
                        } else {
                            Found::Fn { name: i.rux, params, result: i.result, is_async: i.is_async }
                        });
                    }
                    Err(e) => self.problem(&p.file, e.span(), e.to_string()),
                }
            }
            let reg = join(&p.module, &format!("{}::{METHODS_DESCRIPTOR}", p.self_ty));
            let m = self.module(&p.module);
            m.items.extend(found);
            m.registrations.push(reg);
        }
    }
}

fn unreachable_message(what: &str, name: &str) -> String {
    format!(
        "the {what} `{name}` is exported to Rux but cannot be reached from the crate root: \
         it and every module around it must be `pub`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_with(files: &[(&str, &str)]) -> (tempdir::Dir, Scan) {
        let dir = tempdir::Dir::new();
        for (name, text) in files {
            let p = dir.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let s = scan(&dir.0.join("lib.rs"));
        (dir, s)
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Dir {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let p = std::env::temp_dir()
                    .join(format!("rux-bindgen-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_crate_is_read_module_by_module() {
        let (_d, s) = crate_with(&[
            ("lib.rs", "pub mod shop;\n#[rux::export]\npub fn greet(name: &str) -> String { todo!() }\n#[rux::init]\npub fn setup() {}\n"),
            (
                "shop.rs",
                "#[rux::export]\npub struct Product { pub id: u64, pub price: f64 }\n\
                 #[rux::export]\nimpl Product {\n  pub fn discounted_price(&self, off: f64) -> f64 { 0.0 }\n  pub fn new(id: u64) -> Product { todo!() }\n  fn hidden(&self) {}\n}\n\
                 #[rux::resource]\npub struct Db { pool: u8 }\n\
                 #[rux::export]\npub async fn open(path: String) -> Result<Db, std::io::Error> { todo!() }\n",
            ),
        ]);
        assert_eq!(s.problems, vec![]);
        assert_eq!(s.inits, ["setup"]);
        let root = &s.modules[0];
        assert_eq!(root.path, "");
        assert_eq!(root.registrations, ["__rux_export_greet"]);
        let shop = &s.modules[1];
        assert_eq!(
            shop.registrations,
            ["shop::Product::__rux_type", "shop::Db::__rux_type", "shop::__rux_export_open", "shop::Product::__rux_methods"]
        );
        assert!(shop.items.contains(&Found::Method {
            on: "Product".into(),
            name: "discountedPrice".into(),
            params: vec![("off".into(), "float".into())],
            result: "float".into(),
            is_async: false,
        }));
        assert!(shop.items.contains(&Found::Fn {
            name: "open".into(),
            params: vec![("path".into(), "string".into())],
            result: "Db".into(),
            is_async: true
        }));
        assert!(shop.items.iter().any(|f| matches!(f, Found::Fn { name, .. } if name == "new")));
        assert!(!shop.items.iter().any(|f| matches!(f, Found::Method { name, .. } if name == "hidden")));
    }

    #[test]
    fn what_cannot_be_reached_is_said_where_it_is() {
        let (_d, s) = crate_with(&[
            ("lib.rs", "mod inner;\n"),
            ("inner.rs", "\n\n#[rux::export]\npub fn f() {}\n"),
        ]);
        assert_eq!(s.problems.len(), 1);
        assert_eq!(s.problems[0].line, 4);
        assert!(s.problems[0].message.contains("must be `pub`"), "{}", s.problems[0].message);
    }

    #[test]
    fn methods_need_their_type_exported() {
        let (_d, s) = crate_with(&[("lib.rs", "pub struct A;\n#[rux::export]\nimpl A { pub fn x(&self) {} }\n")]);
        assert!(s.problems[0].message.contains("`A` is not exported"));
    }
}
