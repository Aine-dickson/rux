//! What a Rux app's native Rust exports, read from its source: step 8 of
//! `docs/11-next.md`.
//!
//! Two readers share this crate. The `#[rux::export]` macro reads one item
//! at a time and needs its Rux signature to describe it at run time; the CLI
//! reads the whole `native/` crate with [`scan`] so the checker knows every
//! export without waiting for `cargo`. Both go through [`rux_type`] and
//! [`fn_info`], so the two can never disagree about what a Rust type is in
//! Rux.

use syn::{Attribute, FnArg, GenericArgument, Pat, PathArguments, ReturnType, Signature, Type};

mod scan;

pub use scan::{scan, Found, Problem, Scan, ScannedModule};

/// The hidden function `#[rux::export]` writes beside an exported `fn foo`,
/// giving its descriptor.
pub fn fn_descriptor(rust_name: &str) -> String {
    format!("__rux_export_{}", rust_name.trim_start_matches("r#"))
}

/// The hidden associated function an exported struct or resource gets.
pub const TYPE_DESCRIPTOR: &str = "__rux_type";

/// The hidden associated function an exported `impl` block gets.
pub const METHODS_DESCRIPTOR: &str = "__rux_methods";

/// `discounted_price` as `discountedPrice`. A leading `r#` is dropped, and
/// a name with no `_` is left as it is.
pub fn camel(snake: &str) -> String {
    let snake = snake.trim_start_matches("r#");
    let lead = snake.len() - snake.trim_start_matches('_').len();
    let mut out = String::with_capacity(snake.len());
    out.push_str(&snake[..lead]);
    let mut up = false;
    for c in snake[lead..].chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Whether `attr` is `#[rux::<name>]`.
pub fn is_rux_attr(attr: &Attribute, name: &str) -> bool {
    let segs: Vec<String> = attr.path().segments.iter().map(|s| s.ident.to_string()).collect();
    segs.len() == 2 && segs[0] == "rux" && segs[1] == name
}

/// What `#[rux(name = "…")]` and `#[rux(skip)]` say about an item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuxAttr {
    pub name: Option<String>,
    pub skip: bool,
}

/// Read the `#[rux(…)]` attributes of an item.
pub fn rux_attr(attrs: &[Attribute]) -> syn::Result<RuxAttr> {
    let mut out = RuxAttr::default();
    for a in attrs.iter().filter(|a| a.path().is_ident("rux")) {
        a.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let s: syn::LitStr = meta.value()?.parse()?;
                let name = s.value();
                if !is_rux_name(&name) {
                    return Err(meta.error(format!("`{name}` is not a Rux name")));
                }
                out.name = Some(name);
                Ok(())
            } else if meta.path.is_ident("skip") {
                out.skip = true;
                Ok(())
            } else {
                Err(meta.error("`#[rux(…)]` takes `name = \"…\"` or `skip`"))
            }
        })?;
    }
    Ok(out)
}

fn is_rux_name(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What a Rust type is in Rux, as type text: `int`, `Product[]`,
/// `Map<string, float>?`. An `Err` says why it cannot cross.
pub fn rux_type(ty: &Type) -> Result<String, String> {
    match ty {
        Type::Reference(r) => {
            if r.mutability.is_some() {
                return Err("a `&mut` cannot cross: Rux hands over a value, and gets one back".into());
            }
            rux_type(&r.elem)
        }
        Type::Slice(s) => Ok(array(&rux_type(&s.elem)?)),
        Type::Tuple(t) if t.elems.is_empty() => Ok("void".into()),
        Type::Tuple(_) => Err("a tuple has no Rux type: use a `#[rux::export]` struct".into()),
        Type::Paren(p) => rux_type(&p.elem),
        Type::Group(g) => rux_type(&g.elem),
        Type::Path(p) if p.qself.is_none() => {
            let seg = p.path.segments.last().ok_or("an empty path")?;
            let name = seg.ident.to_string();
            let args = type_args(&seg.arguments);
            let one = |what: &str| -> Result<&Type, String> {
                match args.as_slice() {
                    [t] => Ok(*t),
                    _ => Err(format!("`{what}` takes one type")),
                }
            };
            match name.as_str() {
                "bool" => Ok("bool".into()),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => {
                    Ok("int".into())
                }
                "f32" | "f64" => Ok("float".into()),
                "String" | "str" => Ok("string".into()),
                "char" => Err("a `char` has no Rux type: take or give a `String`".into()),
                "Any" => Ok("any".into()),
                "Option" => Ok(optional(&rux_type(one("Option")?)?)),
                "Vec" => Ok(array(&rux_type(one("Vec")?)?)),
                "Box" | "Arc" => rux_type(one(&name)?),
                "HashMap" | "BTreeMap" => match args.as_slice() {
                    [k, v] if rux_type(k).as_deref() == Ok("string") => Ok(format!("Map<string, {}>", rux_type(v)?)),
                    [_, _] => Err(format!("a `{name}` crosses when its key is a `String`: a Rux map's keys are strings")),
                    _ => Err(format!("`{name}` takes a key and a value type")),
                },
                "Result" => Err("a `Result` is a return type: its `Err` is thrown in Rux".into()),
                "Rc" | "RefCell" | "Cell" => {
                    Err(format!("a `{name}` cannot cross: a native call may run on another thread"))
                }
                _ if !args.is_empty() => Err(format!("`{name}<…>` cannot cross: generic Rust types have no Rux type")),
                _ if name.starts_with(|c: char| c.is_ascii_uppercase()) => Ok(name),
                _ => Err(format!("`{name}` has no Rux type")),
            }
        }
        _ => Err("this type has no Rux type".into()),
    }
}

fn type_args(args: &PathArguments) -> Vec<&Type> {
    match args {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|g| match g {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `T[]`, or `Array<T>` where `T[]` would not read as meant.
fn array(t: &str) -> String {
    if t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '[' || c == ']') {
        format!("{t}[]")
    } else {
        format!("Array<{t}>")
    }
}

fn optional(t: &str) -> String {
    if t.ends_with('?') {
        t.to_string()
    } else {
        format!("{t}?")
    }
}

/// What a function's result is in Rux, and whether it throws (returns a
/// `Result`).
pub fn return_type(ret: &ReturnType) -> Result<(String, bool), String> {
    let ty = match ret {
        ReturnType::Default => return Ok(("void".into(), false)),
        ReturnType::Type(_, t) => t,
    };
    if let Some(ok) = result_ok(ty) {
        return Ok((rux_type(ok)?, true));
    }
    Ok((rux_type(ty)?, false))
}

/// `T` of a `Result<T, E>`, `io::Result<T>` or any other type named
/// `Result`.
pub fn result_ok(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Result" {
        return None;
    }
    type_args(&seg.arguments).first().copied()
}

/// A parameter of an exported function.
#[derive(Clone, Debug)]
pub struct ParamInfo {
    /// Its name in Rux.
    pub name: String,
    /// Its Rust type as written.
    pub ty: Type,
    /// `T` of a parameter written `&T`, which is held while the call runs.
    pub by_ref: Option<Type>,
    /// Its Rux type text.
    pub rux: String,
}

/// An exported function, as both readers need it.
#[derive(Clone, Debug)]
pub struct FnInfo {
    pub rust: syn::Ident,
    pub rux: String,
    /// Takes `&self`.
    pub method: bool,
    pub params: Vec<ParamInfo>,
    pub result: String,
    pub throws: bool,
    pub is_async: bool,
}

/// Read an exported function's signature. `in_impl` is set for a function
/// of an exported `impl` block, where `&self` is allowed.
pub fn fn_info(sig: &Signature, attrs: &[Attribute], in_impl: bool) -> syn::Result<FnInfo> {
    let err = |msg: String| syn::Error::new_spanned(&sig.ident, msg);
    if !sig.generics.params.iter().all(|g| matches!(g, syn::GenericParam::Lifetime(_))) {
        return Err(err("a generic function cannot be exported: Rux calls one signature".into()));
    }
    if sig.unsafety.is_some() {
        return Err(err("an `unsafe fn` cannot be exported: Rux cannot promise what it asks".into()));
    }
    if sig.abi.is_some() || sig.variadic.is_some() {
        return Err(err("an `extern` function cannot be exported".into()));
    }
    let attr = rux_attr(attrs)?;
    let mut method = false;
    let mut params = Vec::new();
    for input in &sig.inputs {
        match input {
            FnArg::Receiver(r) => {
                if !in_impl {
                    return Err(syn::Error::new_spanned(r, "`self` outside an `impl`"));
                }
                if r.reference.is_none() || r.mutability.is_some() {
                    return Err(syn::Error::new_spanned(
                        r,
                        "a method Rux calls takes `&self`: Rux may hold the same value in several places, \
                         so a change goes through a lock inside it",
                    ));
                }
                method = true;
            }
            FnArg::Typed(pt) => {
                let Pat::Ident(pi) = &*pt.pat else {
                    return Err(syn::Error::new_spanned(&pt.pat, "a parameter Rux passes needs a plain name"));
                };
                let rux = rux_type(&pt.ty).map_err(|m| syn::Error::new_spanned(&pt.ty, m))?;
                let by_ref = match &*pt.ty {
                    Type::Reference(r) => Some((*r.elem).clone()),
                    _ => None,
                };
                params.push(ParamInfo { name: camel(&pi.ident.to_string()), ty: (*pt.ty).clone(), by_ref, rux });
            }
        }
    }
    let (result, throws) = return_type(&sig.output).map_err(|m| syn::Error::new_spanned(&sig.output, m))?;
    Ok(FnInfo {
        rust: sig.ident.clone(),
        rux: attr.name.unwrap_or_else(|| camel(&sig.ident.to_string())),
        method,
        params,
        result,
        throws,
        is_async: sig.asyncness.is_some(),
    })
}

/// A field of an exported struct.
#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub rust: syn::Ident,
    pub rux: String,
    pub ty: Type,
    /// Its Rux type; an `Option<T>`'s is `T`'s, and `optional` is set.
    pub rux_ty: String,
    pub optional: bool,
}

/// Read an exported value struct's fields: every one must be `pub` and
/// named, since the whole value crosses.
pub fn struct_fields(item: &syn::ItemStruct) -> syn::Result<Vec<FieldInfo>> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&item.generics, "a generic struct cannot be exported"));
    }
    let syn::Fields::Named(named) = &item.fields else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "an exported struct has named fields: they are its fields in Rux",
        ));
    };
    let mut out = Vec::new();
    for f in &named.named {
        let ident = f.ident.clone().expect("named");
        if !matches!(f.vis, syn::Visibility::Public(_)) {
            return Err(syn::Error::new_spanned(
                f,
                format!(
                    "`{ident}` is not `pub`, and an exported struct crosses whole, every field copied. \
                     Make the field `pub`, or make the struct a `#[rux::resource]`, which Rux holds \
                     without seeing inside"
                ),
            ));
        }
        let attr = rux_attr(&f.attrs)?;
        let (rux_ty, optional) = match option_inner(&f.ty) {
            Some(inner) => (rux_type(inner), true),
            None => (rux_type(&f.ty), false),
        };
        let rux_ty = rux_ty.map_err(|m| syn::Error::new_spanned(&f.ty, m))?;
        out.push(FieldInfo {
            rux: attr.name.unwrap_or_else(|| camel(&ident.to_string())),
            rust: ident,
            ty: f.ty.clone(),
            rux_ty,
            optional,
        });
    }
    Ok(out)
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    type_args(&seg.arguments).first().copied()
}

/// The name a struct or resource has in Rux.
pub fn type_name(ident: &syn::Ident, attrs: &[Attribute]) -> syn::Result<String> {
    Ok(rux_attr(attrs)?.name.unwrap_or_else(|| ident.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(s: &str) -> Result<String, String> {
        rux_type(&syn::parse_str::<Type>(s).unwrap())
    }

    #[test]
    fn names_become_camel_case() {
        assert_eq!(camel("discounted_price"), "discountedPrice");
        assert_eq!(camel("open"), "open");
        assert_eq!(camel("r#type"), "type");
        assert_eq!(camel("_private_x"), "_privateX");
    }

    #[test]
    fn types_map_as_the_reference_says() {
        assert_eq!(ty("u64").unwrap(), "int");
        assert_eq!(ty("&str").unwrap(), "string");
        assert_eq!(ty("Option<Vec<f32>>").unwrap(), "float[]?");
        assert_eq!(ty("Vec<Option<i32>>").unwrap(), "Array<int?>");
        assert_eq!(ty("&[Product]").unwrap(), "Product[]");
        assert_eq!(ty("std::collections::HashMap<String, Vec<i64>>").unwrap(), "Map<string, int[]>");
        assert_eq!(ty("Arc<Database>").unwrap(), "Database");
        assert_eq!(ty("rux::Any").unwrap(), "any");
        assert!(ty("HashMap<u32, i64>").unwrap_err().contains("key"));
        assert!(ty("(i32, i32)").unwrap_err().contains("tuple"));
        assert!(ty("Rc<Database>").unwrap_err().contains("thread"));
        assert!(ty("Page<T>").unwrap_err().contains("generic"));
    }

    #[test]
    fn a_result_throws_and_gives_its_ok() {
        let f: syn::ItemFn = syn::parse_str("async fn load(id: u32, name: &str) -> Result<Vec<Row>, DbError> { todo!() }").unwrap();
        let i = fn_info(&f.sig, &f.attrs, false).unwrap();
        assert_eq!((i.result.as_str(), i.throws, i.is_async), ("Row[]", true, true));
        assert_eq!(i.params.iter().map(|p| p.rux.as_str()).collect::<Vec<_>>(), ["int", "string"]);
        assert!(i.params[1].by_ref.is_some());
    }

    #[test]
    fn a_method_takes_a_shared_self() {
        let f: syn::ImplItemFn = syn::parse_str("pub fn set(&mut self, x: i64) {}").unwrap();
        let e = fn_info(&f.sig, &f.attrs, true).unwrap_err().to_string();
        assert!(e.contains("`&self`"), "{e}");
        let f: syn::ImplItemFn = syn::parse_str("#[rux(name = \"total\")] pub fn sum(&self) -> f64 { 0.0 }").unwrap();
        let i = fn_info(&f.sig, &f.attrs, true).unwrap();
        assert!(i.method);
        assert_eq!(i.rux, "total");
    }

    #[test]
    fn a_value_struct_crosses_whole() {
        let s: syn::ItemStruct = syn::parse_str("pub struct P { pub id: u64, pub note: Option<String> }").unwrap();
        let f = struct_fields(&s).unwrap();
        assert_eq!((f[1].rux_ty.as_str(), f[1].optional), ("string", true));
        let s: syn::ItemStruct = syn::parse_str("pub struct P { pub id: u64, secret: String }").unwrap();
        assert!(struct_fields(&s).unwrap_err().to_string().contains("#[rux::resource]"));
    }
}
