//! Native modules in the language: step 8.3 of `docs/11-next.md`.
//!
//! A native module is linked the way a script module is (`use native::shop;`
//! makes `shop.cheapest(x)` the linked name `native/shop::cheapest`), with
//! its exports read from [`rux_native::registry`] rather than checked from a
//! script. A method of a native type (`p.discountedPrice(0.1)`) is found by
//! the receiver's type name, and called with the receiver first.
//!
//! What crosses is converted here, between the interpreter's [`V`] and
//! [`Any`]; an `int` stays an `int`, and a resource stays the resource.

use std::collections::BTreeMap;
use std::rc::Rc;

use rux_native::registry::{self, ItemKind};
use rux_native::{Any, Error};

use crate::interp::value::V;
use crate::link::{Export, ExportKind};
use crate::types::{parse_type, Type};

/// The module Rux's own native functions live in, the file dialogs among
/// them (Mava Studio's MS-005). A script calls them by bare name; see
/// [`builtin_aliases`].
pub const BUILTIN_MODULE: &str = "native/rux";

/// The functions [`BUILTIN_MODULE`] gives every script.
pub const BUILTIN_FNS: &[&str] = &["pickFile", "pickFiles", "pickSaveFile", "loadSound"];

/// The built-in module's interface: what `rux check` and every document are
/// checked against, whether or not a shell has installed the code. The shell
/// installs the same items with code behind them.
pub fn builtin_interface() -> rux_native::Interface {
    use rux_native::{FieldSig, Item, Sig};
    let field = |name: &str, ty: &str, optional: bool| FieldSig { name: name.into(), ty: ty.into(), optional };
    let dialog = |name: &str, result: &str| Item {
        name: name.into(),
        kind: ItemKind::Fn(Sig {
            params: vec![("options".into(), "FileDialog".into())],
            result: result.into(),
            is_async: true,
        }),
    };
    rux_native::Interface {
        path: "rux".into(),
        items: vec![
            Item {
                name: "FileFilter".into(),
                kind: ItemKind::Record(vec![field("name", "string", false), field("extensions", "string[]", false)]),
            },
            Item {
                name: "FileDialog".into(),
                kind: ItemKind::Record(vec![
                    field("title", "string", true),
                    field("filters", "FileFilter[]", true),
                    field("folder", "string", true),
                    field("name", "string", true),
                ]),
            },
            // A file the person chose (`docs/12-file-input-proposal.md`): what
            // `<input type="file">` binds. `path` is the desktop's alone; the
            // web and Android have no path to give.
            Item { name: "File".into(), kind: ItemKind::Record(file_fields()) },
            Item {
                name: "text".into(),
                kind: ItemKind::Method {
                    on: "File".into(),
                    sig: Sig { params: Vec::new(), result: "string".into(), is_async: true },
                },
            },
            // What was picked is a `File`, as an `<input type="file">` binds,
            // so one way to pick works everywhere (decision 3 of the file
            // input proposal). A save dialog names a file not there yet, so
            // it is still a path.
            dialog("pickFile", "File?"),
            dialog("pickFiles", "File[]"),
            dialog("pickSaveFile", "string?"),
        ]
        .into_iter()
        .chain(sound_items())
        .collect(),
    }
}

/// A sound (`docs/13-file-input-and-media-plan.md`, B.1): `loadSound(src)`
/// gives one, its methods play it, and its properties follow it as it plays.
fn sound_items() -> Vec<rux_native::Item> {
    use rux_native::{Item, Sig};
    let method = |name: &str, params: &[(&str, &str)]| Item {
        name: name.into(),
        kind: ItemKind::Method {
            on: "Sound".into(),
            sig: Sig {
                params: params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
                result: "void".into(),
                is_async: false,
            },
        },
    };
    let property = |name: &str, ty: &str, writable: bool| Item {
        name: name.into(),
        kind: ItemKind::Property { on: "Sound".into(), ty: ty.into(), writable },
    };
    vec![
        Item { name: "Sound".into(), kind: ItemKind::Resource },
        Item {
            name: "loadSound".into(),
            kind: ItemKind::Fn(Sig { params: vec![("src".into(), "string".into())], result: "Sound".into(), is_async: true }),
        },
        method("play", &[]),
        method("pause", &[]),
        method("stop", &[]),
        method("seek", &[("seconds", "float")]),
        property("volume", "float", true),
        property("loop", "bool", true),
        // As HTML's: writing it seeks.
        property("currentTime", "float", true),
        // `none` while the file has not said.
        property("duration", "float?", false),
        property("playing", "bool", false),
    ]
}

fn file_fields() -> Vec<rux_native::FieldSig> {
    let field = |name: &str, ty: &str| rux_native::FieldSig { name: name.into(), ty: ty.into(), optional: false };
    vec![
        field("name", "string"),
        field("size", "int"),
        field("type", "string"),
        field("url", "string"),
        field("path", "string?"),
    ]
}

/// The types every script can name without a `use`, as name and text: `File`,
/// which an `<input type="file">` binds, so `let photo: File? = none` needs
/// no import, and `Sound`, which `loadSound` gives. A file's own type of the
/// same name is its own.
pub fn prelude_types() -> Vec<(String, String)> {
    vec![
        ("File".to_string(), rux_native::Interface::record_text(&file_fields())),
        ("Sound".to_string(), "#opaque Sound".to_string()),
    ]
}

/// The module behind `settings.get`, `settings.set` and `settings.remove`:
/// an app's own small store (Mava Studio's MS-007).
pub const SETTINGS_MODULE: &str = "native/rux/settings";

/// The name a script reaches [`SETTINGS_MODULE`] by without importing it.
pub const SETTINGS_NAME: &str = "settings";

/// The settings module's interface, as [`builtin_interface`] is the dialogs'.
pub fn settings_interface() -> rux_native::Interface {
    use rux_native::{Item, Sig};
    let f = |name: &str, params: &[(&str, &str)], result: &str| Item {
        name: name.into(),
        kind: ItemKind::Fn(Sig {
            params: params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
            result: result.into(),
            is_async: false,
        }),
    };
    rux_native::Interface {
        path: "rux::settings".into(),
        items: vec![
            f("get", &[("key", "string")], "any?"),
            f("set", &[("key", "string"), ("value", "any")], "void"),
            f("remove", &[("key", "string")], "void"),
        ],
    }
}

/// Declare the built-in modules if no shell has installed them, so a
/// document checked or run without one still knows the names.
pub fn ensure_builtins() {
    if registry::interface(BUILTIN_MODULE).is_none() {
        let _ = registry::declare(builtin_interface());
    }
    if registry::interface(SETTINGS_MODULE).is_none() {
        let _ = registry::declare(settings_interface());
    }
}

/// The imports a scope gets for free: each built-in function its code calls
/// and does not declare itself, linked to [`BUILTIN_MODULE`] as an explicit
/// `use native::rux::{pickFile};` would link it.
pub fn builtin_aliases(code: &str, aliases: &[crate::link::Alias]) -> Vec<crate::link::Alias> {
    let mut out = builtin_fn_aliases(code, aliases);
    // `settings.get(…)`: the store, as a whole module, unless the file has a
    // `settings` of its own (a `use native::settings;`, a `let settings`).
    let uses = code.match_indices("settings.").any(|(at, _)| {
        !code[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
    });
    let own = ["let settings", "fn settings", "const settings", "settings ="].iter().any(|d| code.contains(d))
        || aliases.iter().any(|a| a.local == SETTINGS_NAME);
    if uses && !own {
        out.push(crate::link::Alias {
            local: SETTINGS_NAME.to_string(),
            target: crate::link::Target::Module(SETTINGS_MODULE.to_string()),
            line: 0,
        });
    }
    out
}

fn builtin_fn_aliases(code: &str, aliases: &[crate::link::Alias]) -> Vec<crate::link::Alias> {
    BUILTIN_FNS
        .iter()
        .filter(|name| {
            let called = code.match_indices(*name).any(|(at, _)| {
                let before = code[..at].chars().next_back();
                let after = code[at + name.len()..].trim_start().chars().next();
                !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') && after == Some('(')
            });
            let declared = code.contains(&format!("fn {name}"));
            called && !declared && !aliases.iter().any(|a| a.local == **name)
        })
        .map(|name| crate::link::Alias {
            local: name.to_string(),
            target: crate::link::Target::Member(BUILTIN_MODULE.to_string(), name.to_string()),
            line: 0,
        })
        .collect()
}

/// Whether a linked module name is a native one: `native`, `native/shop`.
pub fn is_native(module: &str) -> bool {
    module == "native" || module.starts_with("native/")
}

/// A type's text as the registry holds it, read; what cannot be read is
/// `any`, which the source it came from was already checked against.
fn ty(text: &str) -> Type {
    parse_type(text).unwrap_or(Type::Any)
}

/// What native module `module` exports, as an importing file's checker
/// sees it: its functions and its types. `None` when there is no such
/// module in this process.
pub fn exports(module: &str) -> Option<Vec<Export>> {
    let i = registry::interface(module)?;
    Some(
        i.items
            .iter()
            .filter_map(|item| {
                let kind = match &item.kind {
                    ItemKind::Fn(s) => ExportKind::Fn {
                        params: s.params.iter().map(|(_, t)| ty(t)).collect(),
                        result: ty(&s.result),
                        is_async: s.is_async,
                    },
                    ItemKind::Record(_) | ItemKind::Resource => ExportKind::Type,
                    // Read and followed like a script module's signal, and
                    // like one never written from outside its module.
                    ItemKind::Signal(t) => ExportKind::Signal(ty(t)),
                    ItemKind::Method { .. } | ItemKind::Property { .. } => return None,
                };
                Some(Export { name: item.name.clone(), kind })
            })
            .collect(),
    )
}

/// The types module `module` declares, as name, the text
/// [`crate::types::parse_decl`] reads, and the path an import would name:
/// what a file importing anything of it can resolve.
pub fn types(module: &str) -> Vec<(String, String, String)> {
    let Some(i) = registry::interface(module) else { return Vec::new() };
    let path = module.replace('/', "::");
    i.items
        .iter()
        .filter_map(|item| {
            let text = match &item.kind {
                ItemKind::Record(fields) => rux_native::Interface::record_text(fields),
                ItemKind::Resource => format!("#opaque {}", item.name),
                _ => return None,
            };
            Some((item.name.clone(), text, format!("{path}::{}", item.name)))
        })
        .collect()
}

/// A native type's text, for a file that imports it with `use type`.
pub fn type_text(item: &rux_native::Item) -> String {
    match &item.kind {
        ItemKind::Record(fields) => rux_native::Interface::record_text(fields),
        _ => format!("#opaque {}", item.name),
    }
}

/// A method of a native type.
#[derive(Clone, Debug)]
pub struct NativeMethod {
    /// Its linked name, `native/shop::Product.discountedPrice`.
    pub key: String,
    pub params: Vec<Type>,
    pub result: Type,
    pub is_async: bool,
}

/// The name of the native type a value of type `t` is, looking through
/// `T?`.
pub fn type_name(t: &Type) -> Option<&str> {
    match t {
        Type::Named(n) | Type::Opaque(n) => Some(n),
        Type::Union(m) => {
            let named: Vec<&str> = m.iter().filter(|x| **x != Type::Null).filter_map(type_name).collect();
            match named.as_slice() {
                [one] if m.len() == 2 => Some(one),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Method `name` of the native type called `on`, in whichever native module
/// declares it.
pub fn method(on: &str, name: &str) -> Option<NativeMethod> {
    for i in registry::all() {
        for item in &i.items {
            if let ItemKind::Method { on: t, sig } = &item.kind {
                if t == on && item.name == name {
                    return Some(NativeMethod {
                        key: format!("{}::{on}.{name}", i.rux_name()),
                        params: sig.params.iter().map(|(_, t)| ty(t)).collect(),
                        result: ty(&sig.result),
                        is_async: sig.is_async,
                    });
                }
            }
        }
    }
    None
}

/// The linked name of method `name` of the native type `v` is, found from
/// the value where its type was not known: a resource by its own type, a
/// record by being the one native record type with such a method whose
/// fields it has. `None` when that is not exactly one.
pub fn method_for_value(v: &V, name: &str) -> Option<String> {
    match v {
        V::Native(h) => method(h.type_name(), name).map(|m| m.key),
        V::Map(_) | V::Rec(_) => {
            let has = |name: &str| match v {
                V::Map(m) => m.contains_key(name),
                V::Rec(r) => r.has(name),
                _ => false,
            };
            let mut found = Vec::new();
            for i in registry::all() {
                let records: Vec<(&str, &Vec<rux_native::FieldSig>)> = i
                    .items
                    .iter()
                    .filter_map(|it| match &it.kind {
                        ItemKind::Record(f) => Some((it.name.as_str(), f)),
                        _ => None,
                    })
                    .collect();
                for item in &i.items {
                    let ItemKind::Method { on, .. } = &item.kind else { continue };
                    if item.name != name {
                        continue;
                    }
                    let fits = records
                        .iter()
                        .find(|(n, _)| n == on)
                        .is_some_and(|(_, fs)| fs.iter().all(|f| f.optional || has(&f.name)));
                    if fits {
                        found.push(format!("{}::{on}.{name}", i.rux_name()));
                    }
                }
            }
            match found.as_slice() {
                [one] => Some(one.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether any native type has a method called `name`.
pub fn has_method_named(name: &str) -> bool {
    registry::all().iter().any(|i| i.items.iter().any(|it| matches!(it.kind, ItemKind::Method { .. }) && it.name == name))
}

/// Whether native type `on` has any methods, for a message about one it
/// does not have.
/// A property of a native resource type, as the checker and lowering see it.
pub struct NativeProperty {
    /// Its getter's linked name, `native/rux::Sound.currentTime`.
    pub key: String,
    /// Its setter's, `native/rux::Sound.currentTime=`, when it is writable.
    pub setter: Option<String>,
    pub ty: Type,
}

/// Property `name` of the native type called `on`, in whichever native
/// module declares it.
pub fn property(on: &str, name: &str) -> Option<NativeProperty> {
    for i in registry::all() {
        for item in &i.items {
            if let ItemKind::Property { on: t, ty: text, .. } = &item.kind {
                if t == on && item.name == name {
                    let module = i.rux_name();
                    return Some(NativeProperty {
                        key: format!("{module}::{}", item.key()),
                        setter: item.setter_key().map(|k| format!("{module}::{k}")),
                        ty: ty(text),
                    });
                }
            }
        }
    }
    None
}

/// The name every read of a native property counts as, so whatever read one
/// is read again when Rust says something moved ([`rux_native::notify`]).
pub const PROPERTIES_READ: &str = "#native-properties";

/// Where [`crate::MEDIA_FN`] finds an `<audio>`'s sound: the shell's, which
/// loads and holds it. Unset, every element's sound is `none`.
static MEDIA: std::sync::OnceLock<fn(&str) -> Any> = std::sync::OnceLock::new();

/// Give [`crate::MEDIA_FN`] its lookup. Once per process; later calls are
/// the same code again and change nothing.
pub fn set_media(lookup: fn(&str) -> Any) {
    let _ = MEDIA.set(lookup);
}

/// The sound of the `<audio>` keyed `key`, as the interpreter holds values.
pub fn media(key: &str) -> V {
    MEDIA.get().map_or(V::None, |lookup| from_any(lookup(key)))
}

/// Whether linked name `key` is a native property's getter: `Type.name`
/// where `Type` has a property `name`. Asked once per key by the
/// interpreter, which caches its calls.
pub fn is_getter(key: &str) -> bool {
    let Some((_, member)) = crate::link::split(key) else { return false };
    let Some((on, name)) = member.split_once('.') else { return false };
    !name.ends_with('=') && property(on, name).is_some()
}

/// The names of the properties of native type `on`.
pub fn properties_of(on: &str) -> Vec<String> {
    let mut out = Vec::new();
    for i in registry::all() {
        for item in &i.items {
            if matches!(&item.kind, ItemKind::Property { on: t, .. } if t == on) {
                out.push(item.name.clone());
            }
        }
    }
    out
}

pub fn methods_of(on: &str) -> Vec<String> {
    let mut out = Vec::new();
    for i in registry::all() {
        for item in &i.items {
            if matches!(&item.kind, ItemKind::Method { on: t, .. } if t == on) {
                out.push(item.name.clone());
            }
        }
    }
    out
}

/// How to call the export with linked name `key`. An `Err` says why not.
pub fn call_of(key: &str) -> Result<rux_native::Call, String> {
    let (module, member) = crate::link::split(key).ok_or_else(|| format!("`{key}` is not a native name"))?;
    if let Some(c) = registry::call(module, member) {
        return Ok(c);
    }
    let shown = module.replace('/', "::");
    if registry::interface(module).is_some() {
        Err(format!(
            "{shown} is known from its Rust source but is not compiled into this program: \
             `rux run` builds the app's native/ crate in"
        ))
    } else {
        Err(format!("there is no native module {shown}"))
    }
}

/// What native signal `key` holds now, as the interpreter's own value; `none`
/// if it cannot be read.
pub fn read_signal(key: &str) -> V {
    let read = match call_of(key) {
        Ok(rux_native::Call::Sync(read)) => read(Vec::new()).ok(),
        _ => None,
    };
    read.map(from_any).unwrap_or(V::None)
}

/// A value on its way into Rust.
pub fn to_any(v: &V) -> Any {
    match v {
        V::None => Any::None,
        V::Bool(b) => Any::Bool(*b),
        V::Int(i) => Any::Int(*i),
        V::Float(f) => Any::Float(*f),
        V::Str(s) => Any::Str(s.to_string()),
        V::Array(items) => Any::Array(items.iter().map(to_any).collect()),
        V::Map(m) => Any::Map(m.iter().map(|(k, v)| (k.clone(), to_any(v))).collect()),
        V::Rec(r) => Any::Map(r.entries().map(|(k, v)| (k.to_string(), to_any(v))).collect()),
        V::Range(a, b) => Any::Array((*a..*b).map(Any::Int).collect()),
        V::Native(h) => Any::Resource(h.as_ref().clone()),
        // Nothing Rust could use: a closure runs only in the interpreter,
        // and an element is the runtime's.
        V::Fn(_) | V::Element(_) => Any::None,
    }
}

/// The shape of the one native record type a map from Rust with these keys
/// is (step 10.5.4), found once per set of keys while the modules stay as
/// they are: Rust hands a struct over keyed, without its fields' order.
fn record_shape(m: &BTreeMap<String, Any>) -> Option<Rc<crate::interp::value::Shape>> {
    thread_local! {
        static FOUND: std::cell::RefCell<(u64, std::collections::HashMap<Vec<String>, Option<Rc<crate::interp::value::Shape>>>)> =
            std::cell::RefCell::new((u64::MAX, std::collections::HashMap::new()));
    }
    let keys: Vec<String> = m.keys().cloned().collect();
    FOUND.with(|f| {
        let mut f = f.borrow_mut();
        let now = registry::generation();
        if f.0 != now {
            *f = (now, std::collections::HashMap::new());
        }
        if let Some(s) = f.1.get(&keys) {
            return s.clone();
        }
        let names: Vec<&str> = keys.iter().map(String::as_str).collect();
        let shape = registry::record_with(&names).map(|fields| {
            let fields: Vec<rux_ir::types::Field> = fields
                .iter()
                .map(|f| rux_ir::types::Field { name: f.name.clone(), optional: f.optional, ty: rux_ir::types::Type::Any })
                .collect();
            crate::interp::value::Shape::closed(&fields)
        });
        f.1.insert(keys, shape.clone());
        shape
    })
}

/// A value on its way back from Rust.
pub fn from_any(a: Any) -> V {
    match a {
        Any::None => V::None,
        Any::Bool(b) => V::Bool(b),
        Any::Int(i) => V::Int(i),
        Any::Float(f) => V::Float(f),
        Any::Str(s) => V::str(s),
        Any::Array(items) => V::array(items.into_iter().map(from_any).collect()),
        Any::Map(mut m) => match record_shape(&m) {
            // A Rust struct a module exports: a record in its declared order.
            Some(shape) => {
                let vals = shape.names().iter().map(|n| m.remove(&**n).map(from_any).unwrap_or(V::None)).collect();
                V::Rec(Rc::new(crate::interp::value::Record { shape, vals }))
            }
            None => V::Map(Rc::new(m.into_iter().map(|(k, v)| (k, from_any(v))).collect::<BTreeMap<_, _>>())),
        },
        Any::Resource(h) => V::Native(Rc::new(h)),
    }
}

/// A native error as the value `catch e` gives: `{ message, kind }`.
pub fn error_value(e: &Error) -> V {
    crate::interp::error_record(V::str(e.message.as_str()), V::str(e.kind.as_str()))
}
