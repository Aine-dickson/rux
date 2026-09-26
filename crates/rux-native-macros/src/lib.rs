//! The attributes of `rux-native`: step 8 of `docs/11-next.md`.
//!
//! Each writes the item back as it was (less its `#[rux(…)]` attributes) and
//! adds what Rux needs: conversions for a struct, and a hidden function per
//! export that describes it, with the code that converts its arguments,
//! calls it and converts what it gives back. The CLI finds those hidden
//! functions by reading the crate's source with `rux-bindgen`, and calls
//! them to register the exports; the names the two use come from that crate.
//!
//! The generated code names the support crate as `::rux`, the name an app's
//! `native/` crate depends on it under.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use rux_bindgen::{fn_descriptor, fn_info, result_ok, struct_fields, type_name, FnInfo, METHODS_DESCRIPTOR, TYPE_DESCRIPTOR};
use syn::{parse_macro_input, Attribute, ImplItem, Item};

/// Export a function, a struct (a value Rux copies) or an `impl` block's
/// `pub fn`s to Rux.
#[proc_macro_attribute]
pub fn export(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let e = syn::Error::new(proc_macro2::Span::call_site(), "`#[rux::export]` takes nothing; rename with `#[rux(name = \"…\")]`");
        return e.to_compile_error().into();
    }
    let item = parse_macro_input!(item as Item);
    let out = match item {
        Item::Fn(f) => export_fn(f),
        Item::Struct(s) => export_struct(s),
        Item::Impl(i) => export_impl(i),
        other => Err(syn::Error::new_spanned(other, "`#[rux::export]` goes on a `fn`, a `struct` or an `impl`")),
    };
    out.unwrap_or_else(|e| e.to_compile_error()).into()
}

/// A struct Rux holds without seeing inside: a database pool, a file, a
/// connection. Rux gets its exported methods and passes it by handle.
#[proc_macro_attribute]
pub fn resource(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let e = syn::Error::new(proc_macro2::Span::call_site(), "`#[rux::resource]` takes nothing");
        return e.to_compile_error().into();
    }
    let s = parse_macro_input!(item as syn::ItemStruct);
    resource_struct(s).unwrap_or_else(|e| e.to_compile_error()).into()
}

/// A function run once before the first document loads: where an app sets
/// a spawner or opens what its exports share.
#[proc_macro_attribute]
pub fn init(attr: TokenStream, item: TokenStream) -> TokenStream {
    let f = parse_macro_input!(item as syn::ItemFn);
    if !attr.is_empty() || !f.sig.inputs.is_empty() || f.sig.asyncness.is_some() || !f.sig.generics.params.is_empty() {
        return syn::Error::new_spanned(&f.sig, "`#[rux::init]` goes on a `pub fn name()` that takes nothing and is not `async`")
            .to_compile_error()
            .into();
    }
    quote!(#f).into()
}

/// `attrs` without the `#[rux(…)]` ones, which only these macros read.
fn strip(attrs: &mut Vec<Attribute>) {
    attrs.retain(|a| !a.path().is_ident("rux"));
}

fn lit_strings(items: impl IntoIterator<Item = (String, String)>) -> Tokens {
    let pairs = items.into_iter().map(|(a, b)| quote!((#a.to_string(), #b.to_string())));
    quote!(::std::vec![#(#pairs),*])
}

fn sig_tokens(i: &FnInfo) -> Tokens {
    let params = lit_strings(i.params.iter().map(|p| (p.name.clone(), p.rux.clone())));
    let result = &i.result;
    let is_async = i.is_async;
    quote!(::rux::Sig { params: #params, result: #result.to_string(), is_async: #is_async })
}

/// The call: arguments converted, the function called by `callee`, and
/// what it gives converted back. `receiver` puts `&self` first.
fn call_tokens(i: &FnInfo, callee: Tokens, receiver: bool, output: &syn::ReturnType) -> Tokens {
    let name = &i.rux;
    let offset = usize::from(receiver);
    let count = i.params.len() + offset;
    let mut takes = Vec::new();
    let mut passes = Vec::new();
    if receiver {
        takes.push(quote!(let __this = <Self as ::rux::FromRuxRef>::hold(::rux::__private::arg(&mut __args, 0))?;));
        passes.push(quote!(&*__this));
    }
    for (n, p) in i.params.iter().enumerate() {
        let at = n + offset;
        let v = format_ident!("__a{}", n);
        match &p.by_ref {
            Some(inner) => {
                takes.push(quote!(let #v = <#inner as ::rux::FromRuxRef>::hold(::rux::__private::arg(&mut __args, #at))?;));
                passes.push(quote!(&*#v));
            }
            None => {
                let ty = &p.ty;
                takes.push(quote!(let #v = <#ty as ::rux::FromRux>::from_rux(::rux::__private::arg(&mut __args, #at))?;));
                passes.push(quote!(#v));
            }
        }
    }
    let called = if i.is_async { quote!(#callee(#(#passes),*).await) } else { quote!(#callee(#(#passes),*)) };
    let gives = match output {
        syn::ReturnType::Type(_, t) if result_ok(t).is_some() => quote!(::rux::__private::result(#called)),
        _ => quote!(::rux::__private::ok(#called)),
    };
    if i.is_async {
        quote! {
            ::rux::Call::future(|__args: ::std::vec::Vec<::rux::Any>| {
                #[allow(unused_mut)]
                let mut __args = __args;
                let __counted = ::rux::__private::arity(#name, &__args, #count);
                ::rux::__private::guarded_async(async move {
                    __counted?;
                    #(#takes)*
                    #gives
                })
            })
        }
    } else {
        quote! {
            ::rux::Call::sync(|__args: ::std::vec::Vec<::rux::Any>| {
                #[allow(unused_mut)]
                let mut __args = __args;
                ::rux::__private::arity(#name, &__args, #count)?;
                ::rux::__private::guarded(move || {
                    #(#takes)*
                    #gives
                })
            })
        }
    }
}

fn export_fn(mut f: syn::ItemFn) -> syn::Result<Tokens> {
    let info = fn_info(&f.sig, &f.attrs, false)?;
    strip(&mut f.attrs);
    let ident = &f.sig.ident;
    let descriptor = format_ident!("{}", fn_descriptor(&ident.to_string()));
    let name = &info.rux;
    let sig = sig_tokens(&info);
    let call = call_tokens(&info, quote!(#ident), false, &f.sig.output);
    let vis = &f.vis;
    Ok(quote! {
        #f

        #[doc(hidden)]
        #[allow(non_snake_case)]
        #vis fn #descriptor() -> ::std::vec::Vec<::rux::Export> {
            ::std::vec![::rux::Export {
                item: ::rux::Item { name: #name.to_string(), kind: ::rux::ItemKind::Fn(#sig) },
                call: ::std::option::Option::Some(#call),
            }]
        }
    })
}

fn export_struct(mut s: syn::ItemStruct) -> syn::Result<Tokens> {
    let fields = struct_fields(&s)?;
    let name = type_name(&s.ident, &s.attrs)?;
    strip(&mut s.attrs);
    if let syn::Fields::Named(named) = &mut s.fields {
        for f in named.named.iter_mut() {
            strip(&mut f.attrs);
        }
    }
    let ident = &s.ident;
    let descriptor = format_ident!("{}", TYPE_DESCRIPTOR);
    let wanted = format!("a {name}");
    let reads = fields.iter().map(|f| {
        let (rust, rux) = (&f.rust, &f.rux);
        quote!(#rust: ::rux::field(&mut __m, #rux)?)
    });
    let writes = fields.iter().map(|f| {
        let (rust, rux) = (&f.rust, &f.rux);
        quote!(__m.insert(#rux.to_string(), ::rux::IntoRux::into_rux(self.#rust)?);)
    });
    let sigs = fields.iter().map(|f| {
        let (rux, ty, optional) = (&f.rux, &f.rux_ty, f.optional);
        quote!(::rux::FieldSig { name: #rux.to_string(), ty: #ty.to_string(), optional: #optional })
    });
    Ok(quote! {
        #s

        impl ::rux::__private::Named for #ident {
            const NAME: &'static str = #name;
        }

        impl ::rux::FromRux for #ident {
            fn from_rux(__v: ::rux::Any) -> ::std::result::Result<Self, ::rux::Error> {
                let mut __m = match __v {
                    ::rux::Any::Map(m) => m,
                    other => return ::std::result::Result::Err(::rux::Error::wrong(#wanted, &other)),
                };
                ::std::result::Result::Ok(#ident { #(#reads),* })
            }
        }

        impl ::rux::IntoRux for #ident {
            fn into_rux(self) -> ::std::result::Result<::rux::Any, ::rux::Error> {
                let mut __m = ::rux::__private::BTreeMap::new();
                #(#writes)*
                ::std::result::Result::Ok(::rux::Any::Map(__m))
            }
        }

        impl ::rux::FromRuxRef for #ident {
            type Holder = ::std::boxed::Box<#ident>;
            fn hold(__v: ::rux::Any) -> ::std::result::Result<Self::Holder, ::rux::Error> {
                <#ident as ::rux::FromRux>::from_rux(__v).map(::std::boxed::Box::new)
            }
        }

        impl #ident {
            #[doc(hidden)]
            pub fn #descriptor() -> ::std::vec::Vec<::rux::Export> {
                ::std::vec![::rux::Export {
                    item: ::rux::Item {
                        name: #name.to_string(),
                        kind: ::rux::ItemKind::Record(::std::vec![#(#sigs),*]),
                    },
                    call: ::std::option::Option::None,
                }]
            }
        }
    })
}

fn resource_struct(mut s: syn::ItemStruct) -> syn::Result<Tokens> {
    if !s.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&s.generics, "a generic struct cannot be a resource"));
    }
    let name = type_name(&s.ident, &s.attrs)?;
    strip(&mut s.attrs);
    let ident = &s.ident;
    let descriptor = format_ident!("{}", TYPE_DESCRIPTOR);
    Ok(quote! {
        #s

        impl ::rux::Resource for #ident {
            const NAME: &'static str = #name;
        }

        impl ::rux::__private::Named for #ident {
            const NAME: &'static str = #name;
        }

        impl ::rux::IntoRux for #ident {
            fn into_rux(self) -> ::std::result::Result<::rux::Any, ::rux::Error> {
                ::std::result::Result::Ok(::rux::Any::Resource(::rux::Handle::new(self)))
            }
        }

        impl ::rux::FromRuxRef for #ident {
            type Holder = ::rux::__private::Arc<#ident>;
            fn hold(__v: ::rux::Any) -> ::std::result::Result<Self::Holder, ::rux::Error> {
                <::rux::__private::Arc<#ident> as ::rux::FromRux>::from_rux(__v)
            }
        }

        impl #ident {
            #[doc(hidden)]
            pub fn #descriptor() -> ::std::vec::Vec<::rux::Export> {
                ::std::vec![::rux::Export {
                    item: ::rux::Item { name: #name.to_string(), kind: ::rux::ItemKind::Resource },
                    call: ::std::option::Option::None,
                }]
            }
        }
    })
}

fn export_impl(mut im: syn::ItemImpl) -> syn::Result<Tokens> {
    if im.trait_.is_some() || !im.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&im.self_ty, "`#[rux::export]` goes on a plain `impl Type`, not a trait's and not generic"));
    }
    let mut exports = Vec::new();
    for it in im.items.iter_mut() {
        let ImplItem::Fn(f) = it else { continue };
        let public = matches!(f.vis, syn::Visibility::Public(_));
        let attr = rux_bindgen::rux_attr(&f.attrs)?;
        if public && !attr.skip {
            let info = fn_info(&f.sig, &f.attrs, true)?;
            let rust = &f.sig.ident;
            let call = call_tokens(&info, quote!(Self::#rust), info.method, &f.sig.output);
            let sig = sig_tokens(&info);
            let name = &info.rux;
            let kind = if info.method {
                quote!(::rux::ItemKind::Method { on: __on.to_string(), sig: #sig })
            } else {
                quote!(::rux::ItemKind::Fn(#sig))
            };
            exports.push(quote! {
                ::rux::Export {
                    item: ::rux::Item { name: #name.to_string(), kind: #kind },
                    call: ::std::option::Option::Some(#call),
                }
            });
        }
        strip(&mut f.attrs);
    }
    let self_ty = &im.self_ty;
    let descriptor = format_ident!("{}", METHODS_DESCRIPTOR);
    Ok(quote! {
        #im

        impl #self_ty {
            #[doc(hidden)]
            pub fn #descriptor() -> ::std::vec::Vec<::rux::Export> {
                let __on = <Self as ::rux::__private::Named>::NAME;
                let _ = __on;
                ::std::vec![#(#exports),*]
            }
        }
    })
}
