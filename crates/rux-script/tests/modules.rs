//! Script modules linked into a document: step 7 of `docs/11-next.md`,
//! "Modules". What the runtime does with files is in `rux-runtime`'s
//! `tests/modules.rs`; this is the engine's half, with the files handed in.

use rux_script::link::{Alias, Target};
use rux_script::{Builder, Engine, ModuleSource};

const CART: &str = "
export type Item = { name: string, qty: int };
export let items: Item[] = signal([]);
export let limit = 3;
let added = signal(0);
export fn add(name: string) {
  if items.length < limit { items.push({ name: name, qty: 1 }); added++; }
}
export fn count(): int { items.length }
export fn times(): int { added }
";

fn cart() -> ModuleSource {
    ModuleSource { name: "stores/cart".into(), script: CART.into(), aliases: Vec::new(), types: Vec::new() }
}

fn whole(local: &str, module: &str) -> Alias {
    Alias { local: local.into(), target: Target::Module(module.into()), line: 1 }
}

fn picked(local: &str, module: &str, name: &str) -> Alias {
    Alias { local: local.into(), target: Target::Member(module.into(), name.into()), line: 2 }
}

fn engine(modules: Vec<ModuleSource>, aliases: Vec<Alias>, src: &str) -> Engine {
    let mut b = Builder::new();
    for m in modules {
        b.module(m);
    }
    b.aliases(aliases);
    b.build(src).unwrap_or_else(|e| panic!("{src}\n{e:?}"))
}

fn shown(e: &mut Engine, expr: &str) -> String {
    e.eval_display(expr, &[])
}

#[test]
fn a_store_is_reached_whole_and_by_name() {
    let mut e = engine(
        vec![cart()],
        vec![whole("cart", "stores/cart"), picked("put", "stores/cart", "add")],
        "fn buy(n: string) { cart.add(n); }",
    );
    e.run_handler("buy(\"tea\")");
    e.run_handler("put(\"cake\")");
    e.run_handler("cart.add(\"jam\")");
    assert_eq!(shown(&mut e, "cart.count()"), "3");
    assert_eq!(shown(&mut e, "cart.items[1].name"), "cake");
    // Its top level ran once: `limit` holds, and so does its private state.
    e.run_handler("cart.add(\"one too many\")");
    assert_eq!(shown(&mut e, "cart.items.length"), "3");
    assert_eq!(shown(&mut e, "cart.times()"), "3");
    assert!(e.module_findings().iter().all(|(_, f)| f.is_empty()), "{:?}", e.module_findings());
}

#[test]
fn a_binding_follows_the_stores_signal_by_its_linked_name() {
    let mut e = engine(vec![cart()], vec![whole("cart", "stores/cart")], "");
    let (_, reads) = e.eval_display_tracked("cart.items.length", &[]);
    assert!(reads.contains("stores/cart::items"), "{reads:?}");
    let writes = e.run_handler_tracked("cart.add(\"tea\")");
    assert!(writes.contains("stores/cart::items"), "{writes:?}");
}

#[test]
fn a_module_may_import_another() {
    let money = ModuleSource {
        name: "utils/money".into(),
        script: "export fn price(qty: int): float { qty.toFloat() * 2.5 }".into(),
        aliases: Vec::new(),
        types: Vec::new(),
    };
    let till = ModuleSource {
        name: "stores/till".into(),
        script: "export let total = signal(0.0);\nexport fn ring(qty: int) { total += money.price(qty); }".into(),
        aliases: vec![whole("money", "utils/money")],
        types: Vec::new(),
    };
    let mut e = engine(vec![money, till], vec![picked("ring", "stores/till", "ring"), whole("till", "stores/till")], "");
    e.run_handler("ring(2); ring(1);");
    assert_eq!(shown(&mut e, "till.total"), "7.5");
}

#[test]
fn what_cannot_be_linked_is_reported_and_types_are_checked() {
    let e = engine(vec![cart()], vec![whole("cart", "stores/cart")], "fn f() { cart.add(3); cart.items = []; }");
    // A handler that names something the module does not export.
    let err = e.check_syntax("cart.secret()").unwrap_err();
    assert!(err.contains("does not export `secret`"), "{err}");
    // The document's own script, checked with its modules.
    let (script, src) = e.parsed();
    let cx = e.linked_context(&Default::default());
    let (findings, _) = e.check_types_typed(Some((script, src)), &cx);
    let said: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
    assert!(said.iter().any(|m| m.contains("string")), "the argument's type: {said:?}");
    let mut script = rux_syntax::parse("cart.items = [];", Default::default()).unwrap();
    let problems = e.link(&mut script);
    assert!(problems[0].message.contains("only stores/cart changes it"), "{problems:?}");
}

#[test]
fn a_problem_in_a_module_is_its_own() {
    let bad = ModuleSource {
        name: "stores/bad".into(),
        script: "export fn f(): int { \"text\" }".into(),
        aliases: Vec::new(),
        types: Vec::new(),
    };
    let e = engine(vec![bad], vec![whole("bad", "stores/bad")], "");
    let found = e.module_findings();
    assert_eq!(found[0].0, "stores/bad");
    assert!(found[0].1.iter().any(|f| f.is_error), "{found:?}");
}
