//! Reading a whole `native/` crate: every export, with its Rux signature,
//! and the path of the code that registers it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::{ImplItem, Item, Visibility};

use crate::{fn_info, fn_descriptor, signal_info, is_rux_attr, rux_attr, struct_fields, type_name, METHODS_DESCRIPTOR, TYPE_DESCRIPTOR};

/// One export, by its Rux name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Fn { name: String, params: Vec<(String, String)>, result: String, is_async: bool },
    /// A method of `on`, a type of the same module.
    Method { on: String, name: String, params: Vec<(String, String)>, result: String, is_async: bool },
    /// Fields as name, type and whether it may be left out.
    Record { name: String, fields: Vec<(String, String, bool)> },
    Resource { name: String },
    /// A `static` [`rux::Signal`], with its Rux type.
    Signal { name: String, ty: String },
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

/// What an exported function can throw, read from its `Result`'s error type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thrown {
    /// The module's Rust path, as [`ScannedModule::path`].
    pub path: String,
    /// The export's key: its name, or `Type.name` for a method.
    pub key: String,
    /// A Rux record type text: `{ message: string, kind: "a" | "b", status?: any }`.
    pub ty: String,
}

/// What a crate exports.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub modules: Vec<ScannedModule>,
    /// What each function with a `Result` can throw.
    pub throws: Vec<Thrown>,
    /// The Rust paths of the `#[rux::init]` functions.
    pub inits: Vec<String>,
    pub problems: Vec<Problem>,
}

/// Read the crate whose root file is `lib`, following `mod x;` to its file.
/// Never fails: what cannot be read is a [`Problem`].
pub fn scan(lib: &Path) -> Scan {
    let mut s = Scanner {
        scan: Scan::default(),
        renames: HashMap::new(),
        impls: Vec::new(),
        fn_errors: Vec::new(),
        error_impls: HashMap::new(),
        structs: HashMap::new(),
        defined: HashSet::new(),
        inline: 0,
    };
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
    /// Each function with a `Result`: its module path, key and `E`'s name.
    fn_errors: Vec<(String, String, Option<String>)>,
    /// `impl IntoError for X`, by `X`.
    error_impls: HashMap<String, ErrorImpl>,
    /// An exported struct's fields, by its Rust name, for `with_fields`.
    structs: HashMap<String, Vec<(String, String, bool)>>,
    /// The structs and enums this crate defines, by Rust name.
    defined: HashSet<String>,
    /// How many inline `mod x { }` blocks the items being read are inside.
    inline: usize,
}

/// What `impl rux::IntoError for X` says script can see of an `X`.
#[derive(Default)]
struct ErrorImpl {
    /// `KINDS`; `None` when it is not written out as string literals.
    kinds: Option<Vec<String>>,
    /// Fields added by `.with("name", ..)`.
    named: Vec<String>,
    /// The struct handed to `.with_fields(S { .. })`.
    record: Option<String>,
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
                        self.inline += 1;
                        self.items(inner, &sub, &dir, file, reach);
                        self.inline -= 1;
                        continue;
                    }
                    if let Some(attr) = m.attrs.iter().find(|a| a.path().is_ident("path")) {
                        // The file a `#[path = "..."]` names, relative to the directory of the file
                        // holding the declaration (Rust's rule), or of the inline modules around it.
                        // This is how two apps compile, and so export, one shared source file.
                        let Some(named) = path_attr(attr) else {
                            self.problem(file, m.span(), "`#[path]` takes a string: `#[path = \"../shared/session.rs\"]`");
                            continue;
                        };
                        let base = if self.inline > 0 { child_dir.to_path_buf() } else { file.parent().unwrap_or(child_dir).to_path_buf() };
                        let target = base.join(&named);
                        if !target.is_file() {
                            self.problem(file, m.span(), format!("no file for `#[path = {named:?}] mod {}`: looked for {}", m.ident, target.display()));
                            continue;
                        }
                        // A file named this way holds its own `mod x;` lines beside itself.
                        let own = target.parent().unwrap_or(&base).to_path_buf();
                        self.file(&target, &sub, &own, reach);
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
                            if i.throws {
                                self.fn_errors.push((module.to_string(), i.rux.clone(), i.error.clone()));
                            }
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
                Item::Static(st) if st.attrs.iter().any(|a| is_rux_attr(a, "export")) => {
                    if !(reachable && is_pub(&st.vis)) {
                        self.problem(file, st.ident.span(), unreachable_message("static", &st.ident.to_string()));
                        continue;
                    }
                    match signal_info(st) {
                        Ok((name, ty)) => {
                            let reg = join(module, &fn_descriptor(&st.ident.to_string()));
                            let m = self.module(module);
                            m.items.push(Found::Signal { name, ty });
                            m.registrations.push(reg);
                        }
                        Err(e) => self.problem(file, e.span(), e.to_string()),
                    }
                }
                Item::Enum(e) => {
                    self.defined.insert(e.ident.to_string());
                }
                Item::Impl(im)
                    if im.trait_.as_ref().is_some_and(|(_, p, _)| p.segments.last().is_some_and(|s| s.ident == "IntoError")) =>
                {
                    if let syn::Type::Path(p) = &*im.self_ty {
                        if let Some(name) = p.path.segments.last() {
                            let read = error_impl(im);
                            self.error_impls.insert(name.ident.to_string(), read);
                        }
                    }
                }
                Item::Struct(st) => {
                    self.defined.insert(st.ident.to_string());
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
                            Ok(fields) => {
                                let fields: Vec<(String, String, bool)> =
                                    fields.into_iter().map(|f| (f.rux, f.rux_ty, f.optional)).collect();
                                self.structs.insert(st.ident.to_string(), fields.clone());
                                Found::Record { name: name.clone(), fields }
                            }
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
                        if i.throws {
                            let key = if i.method { format!("{on}.{}", i.rux) } else { i.rux.clone() };
                            self.fn_errors.push((p.module.clone(), key, i.error.clone()));
                        }
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
        for (path, key, error) in std::mem::take(&mut self.fn_errors) {
            let ty = self.thrown_type(error.as_deref());
            self.scan.throws.push(Thrown { path, key, ty });
        }
    }

    /// The Rux record type of what a function whose `Result` has error type
    /// `error` throws. Every field but `message` and `kind` is optional,
    /// because the language raises errors that have none of them.
    fn thrown_type(&self, error: Option<&str>) -> String {
        let plain = |kind: String| format!("{{ message: string, kind: {kind} }}");
        let Some(name) = error else { return plain("string".into()) };
        let Some(imp) = self.error_impls.get(name) else {
            // A type of this crate with no impl is thrown as its own name;
            // `rux::Error` is built by the app and one from another crate may
            // implement the trait there, so their kinds are not known here.
            return if self.defined.contains(name) { plain(format!("{name:?}")) } else { plain("string".into()) };
        };
        let kind = match &imp.kinds {
            Some(k) if !k.is_empty() => k.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(" | "),
            _ => "string".to_string(),
        };
        let mut fields: Vec<(String, String)> = Vec::new();
        if let Some(rec) = imp.record.as_ref().and_then(|r| self.structs.get(r)) {
            fields.extend(rec.iter().map(|(n, t, _)| (n.clone(), t.trim_end_matches('?').to_string())));
        }
        for n in &imp.named {
            if !fields.iter().any(|(f, _)| f == n) {
                fields.push((n.clone(), "any".to_string()));
            }
        }
        let rest: String = fields.iter().map(|(n, t)| format!(", {n}?: {t}")).collect();
        format!("{{ message: string, kind: {kind}{rest} }}")
    }
}

/// `KINDS`, the struct handed to `with_fields` and the names given to `with`
/// in an `impl IntoError`, read from its tokens.
fn error_impl(im: &syn::ItemImpl) -> ErrorImpl {
    use quote::ToTokens;
    let mut out = ErrorImpl::default();
    for it in &im.items {
        match it {
            ImplItem::Const(c) if c.ident == "KINDS" => {
                let mut kinds = Vec::new();
                let mut plain = true;
                let mut expr = &c.expr;
                while let syn::Expr::Reference(r) = expr {
                    expr = &r.expr;
                }
                match expr {
                    syn::Expr::Array(a) => {
                        for e in &a.elems {
                            match e {
                                syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => kinds.push(s.value()),
                                _ => plain = false,
                            }
                        }
                    }
                    _ => plain = false,
                }
                out.kinds = plain.then_some(kinds);
            }
            ImplItem::Fn(f) if f.sig.ident == "into_error" => {
                let mut flat = Vec::new();
                flatten(f.block.to_token_stream(), &mut flat);
                for (i, t) in flat.iter().enumerate() {
                    let Tok::Ident(name) = t else { continue };
                    match (name.as_str(), flat.get(i + 1)) {
                        ("with", Some(Tok::Open)) => {
                            if let Some(Tok::Str(s)) = flat.get(i + 2) {
                                out.named.push(s.clone());
                            }
                        }
                        ("with_fields", Some(Tok::Open)) => {
                            // `S { .. }` or `path::S { .. }`: the ident before the brace.
                            let mut j = i + 2;
                            let mut last = None;
                            while let Some(Tok::Ident(n)) = flat.get(j) {
                                last = Some(n.clone());
                                j += 1;
                                if matches!(flat.get(j), Some(Tok::Colons)) {
                                    j += 1;
                                }
                            }
                            if matches!(flat.get(j), Some(Tok::Brace)) {
                                out.record = last;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The few kinds of token `error_impl` looks for, with groups opened out.
enum Tok {
    Ident(String),
    Str(String),
    /// `(`
    Open,
    /// `{`
    Brace,
    /// `::`
    Colons,
    Other,
}

fn flatten(ts: proc_macro2::TokenStream, out: &mut Vec<Tok>) {
    use proc_macro2::{Delimiter, TokenTree};
    for tt in ts {
        match tt {
            TokenTree::Group(g) => {
                out.push(match g.delimiter() {
                    Delimiter::Parenthesis => Tok::Open,
                    Delimiter::Brace => Tok::Brace,
                    _ => Tok::Other,
                });
                flatten(g.stream(), out);
            }
            TokenTree::Ident(i) => out.push(Tok::Ident(i.to_string())),
            TokenTree::Literal(l) => match syn::parse_str::<syn::LitStr>(&l.to_string()) {
                Ok(s) => out.push(Tok::Str(s.value())),
                Err(_) => out.push(Tok::Other),
            },
            // `::` is two joint colons; one marker is enough.
            TokenTree::Punct(p) if p.as_char() == ':' => {
                if !matches!(out.last(), Some(Tok::Colons)) {
                    out.push(Tok::Colons);
                }
            }
            TokenTree::Punct(_) => out.push(Tok::Other),
        }
    }
}

/// The string of a `#[path = "..."]`.
fn path_attr(attr: &syn::Attribute) -> Option<String> {
    match &attr.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
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
    fn what_a_function_throws_is_read_from_its_error_type() {
        let (_d, s) = crate_with(&[(
            "lib.rs",
            "pub struct ApiFailure(u8);\n\
             pub struct Plain;\n\
             #[rux::export]\npub struct ApiFields { pub status: u16, pub details: rux::Any, pub note: Option<String> }\n\
             impl rux::IntoError for ApiFailure {\n\
               const KINDS: &'static [&'static str] = &[\"phone_required\", \"quote_expired\"];\n\
               fn into_error(self) -> rux::Error {\n\
                 rux::Error::new(\"x\", \"y\").with(\"retry\", 1).with_fields(ApiFields { status: 1, details: rux::Any::None, note: None })\n\
               }\n\
             }\n\
             #[rux::export]\npub fn a() -> Result<i64, ApiFailure> { todo!() }\n\
             #[rux::export]\npub fn b() -> Result<i64, Plain> { todo!() }\n\
             #[rux::export]\npub fn c() -> Result<i64, rux::Error> { todo!() }\n\
             #[rux::export]\npub fn d() -> Result<i64, std::io::Error> { todo!() }\n\
             #[rux::export]\npub fn e() -> i64 { 1 }\n",
        )]);
        assert_eq!(s.problems, vec![]);
        let of = |k: &str| s.throws.iter().find(|t| t.key == k).map(|t| t.ty.as_str());
        assert_eq!(
            of("a"),
            Some("{ message: string, kind: \"phone_required\" | \"quote_expired\", status?: int, details?: any, note?: string, retry?: any }")
        );
        assert_eq!(of("b"), Some("{ message: string, kind: \"Plain\" }"));
        assert_eq!(of("c"), Some("{ message: string, kind: string }"));
        assert_eq!(of("d"), Some("{ message: string, kind: string }"));
        assert_eq!(of("e"), None);
    }

    #[test]
    fn a_path_module_is_read_from_the_file_it_names() {
        // Two apps compile one shared source file: `native/lib.rs` names it.
        let (dir, _) = crate_with(&[
            ("app/native/lib.rs", "#[path = \"../../shared/session.rs\"]\npub mod session;\n"),
            ("shared/session.rs", "#[rux::export]\npub fn restore(token: String) -> bool { true }\n"),
        ]);
        let scan = scan(&dir.0.join("app/native/lib.rs"));
        assert_eq!(scan.problems, vec![]);
        let session = scan.modules.iter().find(|m| m.path == "session").expect("the shared module is read");
        assert_eq!(session.registrations, ["session::__rux_export_restore"]);
        assert!(session.items.iter().any(|f| matches!(f, Found::Fn { name, .. } if name == "restore")));
    }

    #[test]
    fn a_path_module_with_no_file_is_said() {
        let (_d, s) = crate_with(&[("lib.rs", "#[path = \"gone.rs\"]\npub mod x;\n")]);
        assert_eq!(s.problems.len(), 1);
        assert!(s.problems[0].message.contains("no file for `#[path"), "{}", s.problems[0].message);
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
