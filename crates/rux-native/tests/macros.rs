//! The attributes, compiled as an app's `native/` crate compiles them, and
//! their descriptors called as the generated registration calls them.

extern crate rux_native as rux;

use rux::{Any, Call, Export, ItemKind};

pub mod shop {
    use std::sync::Mutex;

    #[rux::export]
    #[derive(Clone, Debug, PartialEq)]
    pub struct Product {
        pub id: u64,
        pub unit_price: f64,
        pub note: Option<String>,
    }

    #[rux::export]
    impl Product {
        pub fn discounted_price(&self, off: f64) -> f64 {
            self.unit_price * (1.0 - off)
        }

        #[rux(name = "make")]
        pub fn new(id: u64) -> Product {
            Product { id, unit_price: 1.0, note: None }
        }

        #[rux(skip)]
        pub fn not_for_rux(&self) {}
    }

    #[rux::resource]
    pub struct Counter {
        pub n: Mutex<i64>,
    }

    #[rux::export]
    impl Counter {
        pub fn bump(&self, by: i64) -> i64 {
            let mut n = self.n.lock().unwrap();
            *n += by;
            *n
        }

        pub async fn later(&self) -> i64 {
            *self.n.lock().unwrap()
        }
    }

    #[rux::export]
    pub fn counter(start: i64) -> Counter {
        Counter { n: Mutex::new(start) }
    }

    #[rux::export]
    pub fn total(items: &[Product], label: &str) -> String {
        format!("{label}: {}", items.iter().map(|p| p.unit_price).sum::<f64>())
    }

    #[rux::export]
    pub fn parse(text: String) -> Result<u8, std::num::ParseIntError> {
        text.parse()
    }

    #[rux::export]
    pub fn explode() -> i64 {
        panic!("native code panicked on purpose")
    }

    #[rux::export]
    pub async fn slow_double(n: i64) -> i64 {
        n * 2
    }
}

fn exports() -> Vec<Export> {
    let mut all = Vec::new();
    all.extend(shop::Product::__rux_type());
    all.extend(shop::Product::__rux_methods());
    all.extend(shop::Counter::__rux_type());
    all.extend(shop::Counter::__rux_methods());
    all.extend(shop::__rux_export_counter());
    all.extend(shop::__rux_export_total());
    all.extend(shop::__rux_export_parse());
    all.extend(shop::__rux_export_explode());
    all.extend(shop::__rux_export_slow_double());
    all
}

fn find(key: &str) -> Export {
    exports().into_iter().find(|e| e.item.key() == key).unwrap_or_else(|| panic!("no export {key}"))
}

fn call(key: &str, args: Vec<Any>) -> Result<Any, rux::Error> {
    match find(key).call.expect("callable") {
        Call::Sync(f) => f(args),
        Call::Async(f) => rux::block_on(f(args)),
    }
}

fn product(id: i64, price: f64) -> Any {
    let mut m = std::collections::BTreeMap::new();
    m.insert("id".to_string(), Any::Int(id));
    m.insert("unitPrice".to_string(), Any::Float(price));
    Any::Map(m)
}

#[test]
fn a_struct_is_a_record_type_with_camel_case_fields() {
    let e = find("Product");
    let ItemKind::Record(fields) = e.item.kind else { panic!("a record") };
    let shown: Vec<(String, String, bool)> = fields.into_iter().map(|f| (f.name, f.ty, f.optional)).collect();
    assert_eq!(
        shown,
        [
            ("id".into(), "int".into(), false),
            ("unitPrice".into(), "float".into(), false),
            ("note".into(), "string".into(), true)
        ]
    );
}

#[test]
fn methods_take_the_receiver_first() {
    assert_eq!(call("Product.discountedPrice", vec![product(1, 10.0), Any::Float(0.25)]), Ok(Any::Float(7.5)));
    let made = call("make", vec![Any::Int(4)]).unwrap();
    let Any::Map(m) = made else { panic!("a record") };
    assert_eq!(m["id"], Any::Int(4));
    assert_eq!(m["note"], Any::None);
    assert!(exports().iter().all(|e| e.item.name != "notForRux"));
}

#[test]
fn borrowed_parameters_are_held_for_the_call() {
    let items = Any::Array(vec![product(1, 1.5), product(2, 2.0)]);
    assert_eq!(call("total", vec![items, Any::Str("sum".into())]), Ok(Any::Str("sum: 3.5".into())));
}

#[test]
fn a_resource_keeps_its_state_between_calls() {
    let c = call("counter", vec![Any::Int(10)]).unwrap();
    assert!(matches!(&c, Any::Resource(h) if h.type_name() == "Counter"));
    assert_eq!(call("Counter.bump", vec![c.clone(), Any::Int(5)]), Ok(Any::Int(15)));
    assert_eq!(call("Counter.bump", vec![c.clone(), Any::Int(1)]), Ok(Any::Int(16)));
    assert_eq!(call("Counter.later", vec![c.clone()]), Ok(Any::Int(16)));
    // A value of another type in its place is refused, not reinterpreted.
    let e = call("Counter.bump", vec![product(1, 1.0), Any::Int(1)]).unwrap_err();
    assert_eq!(e.kind, "type");
}

#[test]
fn errors_panics_and_overflow_are_thrown_as_rux_errors() {
    assert_eq!(call("parse", vec![Any::Str("7".into())]), Ok(Any::Int(7)));
    let e = call("parse", vec![Any::Str("300".into())]).unwrap_err();
    assert_eq!(e.kind, "ParseIntError");
    let e = call("explode", vec![]).unwrap_err();
    assert_eq!((e.kind.as_str(), e.message.as_str()), ("panic", "native code panicked on purpose"));
    let e = call("counter", vec![Any::Float(1.5)]).unwrap_err();
    assert_eq!(e.kind, "type");
    let e = call("total", vec![]).unwrap_err();
    assert_eq!(e.message, "total takes 2 arguments, was given 0");
}

#[test]
fn an_async_export_is_a_future() {
    let e = find("slowDouble");
    let ItemKind::Fn(sig) = &e.item.kind else { panic!("a fn") };
    assert!(sig.is_async);
    assert!(matches!(e.call, Some(Call::Async(_))));
    assert_eq!(call("slowDouble", vec![Any::Int(21)]), Ok(Any::Int(42)));
}

#[test]
fn a_module_installs_and_is_found_by_its_rux_name() {
    let mut m = rux::Module::new("shop_t");
    for e in exports() {
        m.add(e);
    }
    rux::install(m).unwrap();
    let i = rux::registry::interface("native/shop_t").unwrap();
    assert!(i.render().contains("export fn total(items: Product[], label: string): string;"), "{}", i.render());
    assert!(rux::registry::call("native/shop_t", "Counter.bump").is_some());
}
