//! The native modules a process has: what each exports, with its Rux
//! signature, and how to call it.
//!
//! Process-wide, as an app registers its Rust once and every document it
//! loads may import it. A module is either **installed**, with the code to
//! call (what `rux run` and `rux build` generate, or a Rust program
//! registering its own), or only **declared**, with the signatures read from
//! source and nothing to call (what `rux check` has).

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::value::{Any, Error};

/// What an `async` export gives back: runs off the UI thread, once.
pub type BoxFuture = Pin<Box<dyn Future<Output = Result<Any, Error>> + Send + 'static>>;

/// How an export is called, with its arguments in order (a method's
/// receiver first).
#[derive(Clone)]
pub enum Call {
    Sync(Arc<dyn Fn(Vec<Any>) -> Result<Any, Error> + Send + Sync>),
    Async(Arc<dyn Fn(Vec<Any>) -> BoxFuture + Send + Sync>),
}

impl Call {
    pub fn sync(f: impl Fn(Vec<Any>) -> Result<Any, Error> + Send + Sync + 'static) -> Call {
        Call::Sync(Arc::new(f))
    }

    pub fn future<F>(f: impl Fn(Vec<Any>) -> F + Send + Sync + 'static) -> Call
    where
        F: Future<Output = Result<Any, Error>> + Send + 'static,
    {
        Call::Async(Arc::new(move |args| Box::pin(f(args))))
    }
}

impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Call::Sync(_) => "Call::Sync",
            Call::Async(_) => "Call::Async",
        })
    }
}

/// A function's Rux signature, as type text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sig {
    pub params: Vec<(String, String)>,
    pub result: String,
    pub is_async: bool,
}

/// A record type's field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSig {
    pub name: String,
    pub ty: String,
    /// An `Option` field: may be left out when Rux builds one.
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A function of the module.
    Fn(Sig),
    /// A method of the module's type `on`; the receiver is not in `params`.
    Method { on: String, sig: Sig },
    /// A `#[rux::export]` struct.
    Record(Vec<FieldSig>),
    /// A `#[rux::resource]`.
    Resource,
}

/// One thing a module exports, by its Rux name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub kind: ItemKind,
}

impl Item {
    /// Its key among the module's calls: `name`, or `Type.name` for a
    /// method.
    pub fn key(&self) -> String {
        match &self.kind {
            ItemKind::Method { on, .. } => format!("{on}.{}", self.name),
            _ => self.name.clone(),
        }
    }

    /// Whether this is a type.
    pub fn is_type(&self) -> bool {
        matches!(self.kind, ItemKind::Record(_) | ItemKind::Resource)
    }
}

/// What a module exports, without the code: what the checker reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Interface {
    /// Its Rust path inside the crate, `::`-joined: `""` for the crate root,
    /// `shop`, `db::users`.
    pub path: String,
    pub items: Vec<Item>,
}

impl Interface {
    /// Its name as a Rux module: `native`, `native/shop`, `native/db/users`.
    pub fn rux_name(&self) -> String {
        rux_name(&self.path)
    }

    /// The type text of a record export, `{ id: int, note?: string }`.
    pub fn record_text(fields: &[FieldSig]) -> String {
        let parts: Vec<String> = fields
            .iter()
            .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
            .collect();
        format!("{{ {} }}", parts.join(", "))
    }

    /// The module as a person reads it, in Rux's own syntax.
    pub fn render(&self) -> String {
        let mut out = format!("// {}\n", self.rux_name().replace('/', "::"));
        let sig = |s: &Sig| {
            let params: Vec<String> = s.params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            format!("({}): {}", params.join(", "), s.result)
        };
        for item in &self.items {
            let line = match &item.kind {
                ItemKind::Record(fields) => format!("export type {} = {};", item.name, Interface::record_text(fields)),
                ItemKind::Resource => format!("export type {};  // a resource: opaque to Rux", item.name),
                ItemKind::Fn(s) => {
                    format!("export {}fn {}{};", if s.is_async { "async " } else { "" }, item.name, sig(s))
                }
                ItemKind::Method { on, sig: s } => {
                    format!("//   {on}.{}{}{}", item.name, sig(s), if s.is_async { "  (async)" } else { "" })
                }
            };
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

/// `shop` as `native/shop`, `""` as `native`.
pub fn rux_name(path: &str) -> String {
    if path.is_empty() {
        "native".to_string()
    } else {
        format!("native/{}", path.replace("::", "/"))
    }
}

/// One export as `#[rux::export]` describes it: the item, and its code
/// unless it is a type.
#[derive(Clone, Debug)]
pub struct Export {
    pub item: Item,
    pub call: Option<Call>,
}

impl Export {
    /// A function, from Rust written by hand.
    pub fn function(name: &str, params: &[(&str, &str)], result: &str, call: Call) -> Export {
        let is_async = matches!(call, Call::Async(_));
        let sig = Sig {
            params: params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            result: result.to_string(),
            is_async,
        };
        Export { item: Item { name: name.to_string(), kind: ItemKind::Fn(sig) }, call: Some(call) }
    }

    /// A record type, from Rust written by hand. A field whose type ends in
    /// `?` may be left out.
    pub fn record(name: &str, fields: &[(&str, &str)]) -> Export {
        let fields = fields
            .iter()
            .map(|(n, t)| FieldSig {
                name: n.to_string(),
                ty: t.strip_suffix('?').unwrap_or(t).to_string(),
                optional: t.ends_with('?'),
            })
            .collect();
        Export { item: Item { name: name.to_string(), kind: ItemKind::Record(fields) }, call: None }
    }
}

/// A module being installed.
#[derive(Clone, Debug, Default)]
pub struct Module {
    pub interface: Interface,
    pub calls: HashMap<String, Call>,
}

impl Module {
    /// An empty module at `path` (`""` for the crate root).
    pub fn new(path: &str) -> Module {
        Module { interface: Interface { path: path.to_string(), items: Vec::new() }, calls: HashMap::new() }
    }

    pub fn export(mut self, e: Export) -> Module {
        self.add(e);
        self
    }

    pub fn add(&mut self, e: Export) {
        if let Some(call) = e.call {
            self.calls.insert(e.item.key(), call);
        }
        self.interface.items.push(e.item);
    }

    /// Install it for every document loaded after this.
    pub fn install(self) {
        if let Err(e) = install(self) {
            panic!("{e}");
        }
    }
}

struct Entry {
    module: Module,
    compiled: bool,
}

static MODULES: Mutex<BTreeMap<String, Entry>> = Mutex::new(BTreeMap::new());

fn modules() -> std::sync::MutexGuard<'static, BTreeMap<String, Entry>> {
    MODULES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Two exports of one module under one Rux name, which Rux could not tell
/// apart.
fn clash(i: &Interface) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    for item in &i.items {
        if !seen.insert(item.key()) {
            return Some(format!("{} exports `{}` twice", i.rux_name().replace('/', "::"), item.key()));
        }
    }
    None
}

/// Install `m`, with its code, replacing whatever had its name.
pub fn install(m: Module) -> Result<(), String> {
    if let Some(e) = clash(&m.interface) {
        return Err(e);
    }
    modules().insert(m.interface.rux_name(), Entry { module: m, compiled: true });
    Ok(())
}

/// Declare `i` as read from source, with no code of its own, unless a module
/// of its name is installed.
///
/// What checks a document without building its Rust (`rux check`) still runs
/// the document's top level, which may call it. Each export then answers the
/// empty value of its declared type (`0`, `""`, `none`, `[]`, a record of
/// those, a placeholder resource), and never runs the app's Rust: nothing
/// that only reads a project executes code from it.
pub fn declare(i: Interface) -> Result<(), String> {
    if let Some(e) = clash(&i) {
        return Err(e);
    }
    let mut all = modules();
    let name = i.rux_name();
    if all.get(&name).is_some_and(|e| e.compiled) {
        return Ok(());
    }
    let mut calls = HashMap::new();
    for item in &i.items {
        let (ItemKind::Fn(sig) | ItemKind::Method { sig, .. }) = &item.kind else { continue };
        let empty = empty_of(&sig.result, &i);
        let call = if sig.is_async {
            Call::future(move |_| {
                let v = empty.clone();
                async move { Ok(v) }
            })
        } else {
            Call::sync(move |_| Ok(empty.clone()))
        };
        calls.insert(item.key(), call);
    }
    all.insert(name, Entry { module: Module { interface: i, calls }, compiled: false });
    Ok(())
}

/// A resource that stands in for one a declared export would give.
struct Placeholder;

/// The empty value of type text `ty`, reading the module's own types.
fn empty_of(ty: &str, i: &Interface) -> Any {
    let ty = ty.trim();
    if ty.ends_with('?') || matches!(ty, "void" | "any") {
        return Any::None;
    }
    if ty.ends_with("[]") || ty.starts_with("Array<") {
        return Any::Array(Vec::new());
    }
    if ty.starts_with("Map<") {
        return Any::Map(BTreeMap::new());
    }
    match ty {
        "int" => return Any::Int(0),
        "float" => return Any::Float(0.0),
        "string" => return Any::Str(String::new()),
        "bool" => return Any::Bool(false),
        _ => {}
    }
    match i.items.iter().find(|it| it.name == ty).map(|it| &it.kind) {
        Some(ItemKind::Record(fields)) => Any::Map(
            fields
                .iter()
                .filter(|f| !f.optional)
                .map(|f| (f.name.clone(), if f.ty == ty { Any::None } else { empty_of(&f.ty, i) }))
                .collect(),
        ),
        Some(ItemKind::Resource) => {
            // The type's name is kept for as long as the process: a checker
            // declares a module's few types once.
            let name: &'static str = Box::leak(ty.to_string().into_boxed_str());
            Any::Resource(crate::value::Handle::placeholder(name, Arc::new(Placeholder)))
        }
        _ => Any::None,
    }
}

/// Forget every declared module (not the installed ones): a checker reading
/// the source again.
pub fn forget_declared() {
    modules().retain(|_, e| e.compiled);
}

/// The module Rux calls `name` (`native/shop`), when there is one.
pub fn interface(name: &str) -> Option<Interface> {
    modules().get(name).map(|e| e.module.interface.clone())
}

/// Whether `name`'s code is in this process, or only its signatures.
pub fn is_compiled(name: &str) -> bool {
    modules().get(name).is_some_and(|e| e.compiled)
}

/// Every module, by Rux name.
pub fn all() -> Vec<Interface> {
    modules().values().map(|e| e.module.interface.clone()).collect()
}

/// How to call `key` (`fn` or `Type.method`) of module `name`.
pub fn call(name: &str, key: &str) -> Option<Call> {
    modules().get(name).and_then(|e| e.module.calls.get(key).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_signatures_never_replace_installed_code() {
        let m = Module::new("reg_t1").export(Export::function(
            "twice",
            &[("n", "int")],
            "int",
            Call::sync(|a| match a.first() {
                Some(Any::Int(n)) => Ok(Any::Int(n * 2)),
                _ => Err(Error::new("type", "no")),
            }),
        ));
        install(m).unwrap();
        declare(Interface { path: "reg_t1".into(), items: vec![] }).unwrap();
        assert!(is_compiled("native/reg_t1"));
        let Some(Call::Sync(f)) = call("native/reg_t1", "twice") else { panic!("installed") };
        assert_eq!(f(vec![Any::Int(4)]), Ok(Any::Int(8)));
        forget_declared();
        assert!(interface("native/reg_t1").is_some());
    }

    #[test]
    fn a_name_exported_twice_is_refused() {
        let f = || Export::function("a", &[], "void", Call::sync(|_| Ok(Any::None)));
        let e = install(Module::new("reg_t2").export(f()).export(f())).unwrap_err();
        assert_eq!(e, "native::reg_t2 exports `a` twice");
    }

    #[test]
    fn rendered_as_rux() {
        let i = Module::new("shop")
            .export(Export::record("Product", &[("id", "int"), ("note", "string?")]))
            .export(Export::function("find", &[("id", "int")], "Product?", Call::sync(|_| Ok(Any::None))))
            .interface;
        assert_eq!(
            i.render(),
            "// native::shop\nexport type Product = { id: int, note?: string };\nexport fn find(id: int): Product?;\n"
        );
    }
}
