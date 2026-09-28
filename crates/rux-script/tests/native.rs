//! Native modules from script: step 8 of `docs/11-next.md`. The Rust here is
//! written as an app's `native/` crate writes it, with `#[rux::export]`, and
//! installed as the generated registration installs it.

extern crate rux_native as rux;

use std::time::{Duration, Instant};

use rux_script::{link, Builder, Engine};

pub mod shop {
    use std::sync::Mutex;

    #[rux::export]
    pub struct Product {
        pub id: u64,
        pub name: String,
        pub price: f64,
    }

    #[rux::export]
    impl Product {
        pub fn discounted(&self, off: f64) -> f64 {
            self.price * (1.0 - off)
        }
    }

    #[rux::resource]
    pub struct Cart {
        items: Mutex<Vec<u64>>,
    }

    #[rux::export]
    impl Cart {
        pub fn add(&self, id: u64) -> i64 {
            let mut items = self.items.lock().unwrap();
            items.push(id);
            items.len() as i64
        }

        pub async fn checkout(&self) -> Result<String, std::fmt::Error> {
            let n = self.items.lock().unwrap().len();
            if n == 0 {
                return Err(std::fmt::Error);
            }
            Ok(format!("{n} items"))
        }
    }

    #[rux::export]
    pub fn open_cart() -> Cart {
        Cart { items: Mutex::new(Vec::new()) }
    }

    #[rux::export]
    pub fn cheapest(items: Vec<Product>) -> Option<Product> {
        items.into_iter().min_by(|a, b| a.price.total_cmp(&b.price))
    }

    /// Fields not in alphabetical order, so a sorted display would show.
    #[rux::export]
    pub struct Spot {
        pub y: i64,
        pub x: i64,
    }

    #[rux::export]
    pub fn spot() -> Spot {
        Spot { y: 2, x: 1 }
    }

    #[rux::export]
    pub fn big() -> u64 {
        u64::MAX
    }

    #[rux::export]
    pub fn byte(n: u8) -> u8 {
        n
    }

    #[rux::export]
    pub async fn lookup(id: i64) -> String {
        std::thread::sleep(std::time::Duration::from_millis(5));
        format!("product {id}")
    }
}

fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut m = rux::Module::new("nt_shop");
        for e in [
            shop::Product::__rux_type(),
            shop::Product::__rux_methods(),
            shop::Cart::__rux_type(),
            shop::Cart::__rux_methods(),
            shop::__rux_export_open_cart(),
            shop::__rux_export_cheapest(),
            shop::Spot::__rux_type(),
            shop::__rux_export_spot(),
            shop::__rux_export_big(),
            shop::__rux_export_byte(),
            shop::__rux_export_lookup(),
        ]
        .into_iter()
        .flatten()
        {
            m.add(e);
        }
        m.install();
    });
}

fn engine(src: &str) -> Engine {
    install();
    let alias = link::Alias { local: "shop".into(), target: link::Target::Module("native/nt_shop".into()), line: 1 };
    let mut b = Builder::new();
    b.aliases(vec![alias]);
    let e = b.build(src).unwrap_or_else(|e| panic!("{src}\n{e:?}"));
    let findings = e.check_types(&e.linked_context(&Default::default()));
    let errors: Vec<_> = findings.iter().filter(|f| f.is_error).collect();
    assert!(errors.is_empty(), "{src}\n{errors:#?}");
    e
}

fn show(e: &mut Engine, expr: &str) -> String {
    e.eval_display(expr, &[])
}

fn settle(e: &mut Engine) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = e.collect_answers();
        if !ready.is_empty() {
            for id in &ready {
                e.resume_task(*id);
            }
            return;
        }
        assert!(Instant::now() < deadline, "no answer came");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn records_cross_both_ways_and_their_methods_run() {
    let mut e = engine(
        "let items = signal([{ id: 1, name: \"tea\", price: 4.0 }, { id: 2, name: \"bun\", price: 2.5 }]);\n\
         fn best(): string { let p = shop.cheapest(items); if p == none { return \"\"; } return p.name; }\n\
         fn deal(): float { let p = shop.cheapest(items); if p == none { return 0.0; } return p.discounted(0.2); }\n",
    );
    assert_eq!(show(&mut e, "best()"), "bun");
    assert_eq!(show(&mut e, "deal()"), "2");
    // In a binding, whose own expressions are not typed, the method is found
    // by the record it is called on.
    assert_eq!(show(&mut e, "shop.cheapest(items).discounted(0.5)"), "1.25");
}

#[test]
fn a_resource_is_held_in_state_and_keeps_its_own() {
    let mut e = engine(
        "let cart = signal(shop.openCart());\n\
         let n = signal(0);\n\
         fn add(id: int) { n = cart.add(id); }\n",
    );
    e.run_handler("add(7)");
    e.run_handler("add(8)");
    assert_eq!(show(&mut e, "n"), "2");
    // The same cart, read twice, is one resource.
    assert_eq!(show(&mut e, "cart == cart"), "true");
}

#[test]
fn an_int_that_does_not_fit_throws_overflow() {
    let mut e = engine(
        "let kind = signal(\"\");\n\
         fn go() { try { shop.big(); } catch e { kind = e.kind; } }\n\
         fn small() { try { shop.byte(300); } catch e { kind = e.kind + \": \" + e.message; } }\n",
    );
    e.run_handler("go()");
    assert_eq!(show(&mut e, "kind"), "overflow");
    e.run_handler("small()");
    assert_eq!(show(&mut e, "kind"), "overflow: 300 does not fit in u8");
}

#[test]
fn an_async_export_is_awaited_off_the_ui_thread() {
    let mut e = engine(
        "let status = signal(\"\");\n\
         async fn load(id: int) { status = \"loading\"; status = await shop.lookup(id); }\n",
    );
    e.run_handler("load(3)");
    assert_eq!(show(&mut e, "status"), "loading");
    settle(&mut e);
    assert_eq!(show(&mut e, "status"), "product 3");
}

#[test]
fn an_async_method_s_rust_error_is_caught_with_its_kind() {
    let mut e = engine(
        "let cart = signal(shop.openCart());\n\
         let out = signal(\"\");\n\
         async fn pay() { try { out = await cart.checkout(); } catch e { out = e.kind; } }\n",
    );
    e.run_handler("pay()");
    settle(&mut e);
    assert_eq!(show(&mut e, "out"), "Error");
    e.run_handler("cart.add(1)");
    e.run_handler("pay()");
    settle(&mut e);
    assert_eq!(show(&mut e, "out"), "1 items");
}

/// A Rust struct comes back a record in the order its fields are declared
/// (step 10.5.4 of `docs/11-next.md`), as a Rux record type's does.
#[test]
fn a_rust_struct_keeps_its_declared_order() {
    let mut e = engine("fn here(): string { let s = shop.spot(); `${s} ${keys(s)} ${s.x}` }
");
    assert_eq!(show(&mut e, "here()"), "y: 2, x: 1 y, x 1");
}
