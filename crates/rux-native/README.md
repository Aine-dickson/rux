# rux-native

Rust code a [Rux](https://ruxlang.dev) app calls from script. Step 8 of
`docs/11-next.md`.

An app keeps its Rust in a `native/` crate beside `rux.toml`, and depends on
this crate under the name `rux`:

```toml
[dependencies]
rux = { package = "rux-native", version = "0.8" }
```

```rust
// native/src/lib.rs
pub mod shop;

// native/src/shop.rs
#[rux::export]
pub struct Product { pub id: u64, pub name: String, pub price: f64 }

#[rux::export]
impl Product {
    pub fn discounted_price(&self, off: f64) -> f64 { self.price * (1.0 - off) }
}

#[rux::export]
pub fn cheapest(items: Vec<Product>) -> Option<Product> {
    items.into_iter().min_by(|a, b| a.price.total_cmp(&b.price))
}
```

```rux
<script>
  use native::shop;
  use type native::shop::{Product};

  let best: Product? = shop.cheapest(items);
</script>
```

`rux run` and `rux build` compile the crate into the app and register every
export; `rux check` and the editor read the Rust source for the signatures,
without building it. `rux native` prints what Rux sees.

## What crosses

| Rust | Rux |
|---|---|
| `bool` | `bool` |
| every integer type | `int` (a value that does not fit throws `kind` `"overflow"`) |
| `f32`, `f64` | `float` |
| `String`, `&str` | `string` |
| `Option<T>` | `T?` |
| `Vec<T>`, `&[T]` | `T[]` |
| `HashMap<String, T>`, `BTreeMap<String, T>` | `Map<string, T>` |
| `()` | `void` |
| `Result<T, E>` | `T`; an `Err` is thrown as an `Error` |
| `rux::Any` | `any` |
| a `#[rux::export]` struct | a record type of the same name |
| a `#[rux::resource]` struct | an opaque type Rux can hold and call methods on |

Anything else is a compile error at the export, saying why.
