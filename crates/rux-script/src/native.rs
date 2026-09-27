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
                    ItemKind::Method { .. } => return None,
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
        V::Native(h) => Any::Resource(h.clone()),
        // Nothing Rust could use: a closure runs only in the interpreter,
        // and an element is the runtime's.
        V::Fn(_) | V::Element(_) => Any::None,
    }
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
        Any::Map(m) => V::Map(Rc::new(m.into_iter().map(|(k, v)| (k, from_any(v))).collect::<BTreeMap<_, _>>())),
        Any::Resource(h) => V::Native(h),
    }
}

/// A native error as the value `catch e` gives: `{ message, kind }`.
pub fn error_value(e: &Error) -> V {
    crate::interp::error_record(V::str(e.message.as_str()), V::str(e.kind.as_str()))
}
