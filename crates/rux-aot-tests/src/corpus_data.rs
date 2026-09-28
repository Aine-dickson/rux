// The scripts the step 9 proof compiles, each with what is run against it:
// a handler (`!` first) or a binding. Included by build.rs, which generates
// Rust for each, and by the tests, which run each on both engines.
//
// What each exercises is said beside it. A case the generator hands back to
// the interpreter is as much a part of the proof as one it compiles: the two
// share a frame, and must not disagree about anything in it.

pub const CORPUS: &[(&str, &[&str])] = &[
    // Arithmetic, recursion, int overflow, the step budget.
    (
        "fn fib(n: int): int { if n < 2 { return n; } fib(n - 1) + fib(n - 2) }\n\
         fn big(): int { 9223372036854775807 + 1 }\n\
         fn forever(): int { let i = 0; while true { i += 1; } i }\n\
         fn half(n: int): float { n / 2 }\n\
         fn deep(n: int): int { deep(n + 1) }\n",
        &["fib(0)", "fib(1)", "fib(15)", "big()", "half(7)", "fib(\"x\")", "deep(0)"],
    ),
    // State: reads, writes, `op=`, and what a handler changes.
    (
        "let count = signal(0);\nlet total = signal(0.0);\nlet log = signal(\"\");\n\
         fn bump(by: int) { count += by; total = total + 2.5; log = log + `${count};`; }\n\
         fn reset() { count = 0; }\n",
        &["!bump(1)", "!bump(2)", "count", "total", "log", "!reset()", "count"],
    ),
    // Loops: ranges, break, continue, nested, a `for` over an array handed
    // back, a `while` with a condition that changes.
    (
        "fn sum(n: int): int { let s = 0; for i in 0..n { s += i; } s }\n\
         fn odd(n: int): int { let s = 0; for i in 0..=n { if i % 2 == 0 { continue; } if i > 7 { break; } s += i; } s }\n\
         fn grid(): int { let s = 0; for a in 0..3 { for b in 0..3 { if b == 2 { break; } s += a * 10 + b; } } s }\n\
         fn walk(): int { let s = 0; for x in [5, 6, 7] { s += x; } s }\n\
         fn down(n: int): int { let k = n; while k > 0 { k -= 3; } k }\n",
        &["sum(10)", "sum(0)", "odd(20)", "grid()", "walk()", "down(10)", "sum(\"x\")"],
    ),
    // Text, templates, `??`, logic, comparison, conversion.
    (
        "fn greet(name: string?): string { let n = name ?? \"you\"; `hi ${n}, ${n.length}` }\n\
         fn both(a: bool, b: bool): bool { a && b || !a }\n\
         fn widen(n: int): float { let f: float = n; f * 1.5 }\n\
         fn cmp(a: string, b: string): bool { a < b }\n\
         fn sign(n: int): string { if n < 0 { \"neg\" } else if n == 0 { \"zero\" } else { let m = n * 2; `pos ${m}` } }\n\
         fn mixed(n: int): int { let k = if n > 0 { let a = [n]; a.push(1); a.length } else { 0 }; k + 1 }\n\
         fn nothing(n: int): int { let k = if n > 0 { 5 }; k ?? 7 }\n",
        &["greet(none)", "greet(\"ada\")", "both(true, false)", "both(false, false)", "widen(3)", "cmp(\"a\", \"b\")",
          "sign(-3)", "sign(0)", "sign(4)", "mixed(2)", "mixed(0)", "nothing(1)", "nothing(0)"],
    ),
    // What is handed back: fields, indexes, methods, closures, try/catch,
    // throw, a `switch`, an `if` as a value, in one frame with compiled code.
    (
        "type Task = { id: int, title: string, done: bool };\n\
         let tasks: Task[] = signal([{ id: 1, title: \"a\", done: false }, { id: 2, title: \"b\", done: true }]);\n\
         fn open(): int { let n = 0; for t in tasks { if !t.done { n += 1; } } n }\n\
         fn titles(): string { tasks.map(t => t.title).join(\",\") }\n\
         fn pick(i: int): string { let t = tasks[i]; if t.done { \"done\" } else { t.title } }\n\
         fn safe(i: int): string { try { tasks[i].title } catch e { `no ${i}: ${e.kind}` } }\n\
         fn kind(n: int): string { switch n { 0 => \"zero\", 1 | 2 => \"few\", _ => \"many\" } }\n\
         fn boom(): int { throw \"no\"; }\n\
         fn add(title: string) { tasks.push({ id: tasks.length + 1, title: title, done: false }); }\n",
        &["open()", "titles()", "pick(0)", "pick(1)", "safe(5)", "kind(0)", "kind(2)", "kind(9)", "boom()", "!add(\"c\")", "open()", "tasks.length"],
    ),
    // `?.` chains, `is`, `switch` with ranges and guards.
    (
        "type Pet = { name: string, age: int };\n\
         type User = { name: string, pet?: Pet };\n\
         let users: User[] = signal([{ name: \"ada\", pet: { name: \"rex\", age: 3 } }, { name: \"bob\" }]);\n\
         fn pet(i: int): string { users[i]?.pet?.name ?? \"none\" }\n\
         fn loud(i: int): string { users[i]?.pet?.name.toUpperCase() ?? \"-\" }\n\
         fn nth(xs: int[]?, i: int): int { xs?[i] ?? -1 }\n\
         fn shout(s: string?): string { s?.toUpperCase() ?? \"quiet\" }\n\
         fn what(x: any): string { if x is int { \"int\" } else if x is string { \"string\" } else if x is Pet { \"pet\" } else { \"other\" } }\n\
         fn band(n: int): string { switch n { 0 => \"zero\", 42 if n > 40 => \"answer\", 1..10 => \"small\", _ => \"big\" } }\n\
         fn word(s: string): int { switch s { \"a\" | \"b\" => { let k = 1; k + 1 } \"c\" => 3, _ => 0 } }\n\
         fn guard(n: int, ok: bool): string { switch n { 1 if ok == true => \"one ok\", 1 => \"one\", _ => \"other\" } }\n",
        &["pet(0)", "pet(1)", "pet(2)", "loud(0)", "loud(1)", "nth([4, 5], 1)", "nth(none, 1)", "nth([4], 3)",
          "shout(\"hi\")", "shout(none)", "what(3)", "what(\"x\")", "what({ name: \"a\", age: 1 })", "what(2.5)",
          "band(0)", "band(5)", "band(42)", "band(99)", "word(\"b\")", "word(\"c\")", "word(\"z\")",
          "guard(1, true)", "guard(1, false)", "guard(2, true)"],
    ),
    // `try` with what leaves it: a value, a failure caught, `return`,
    // `break` and `continue` out of it; a nested `try` rethrowing.
    (
        "fn safe_div(a: int, b: int): int { try { return trunc(a / b); } catch e { return -1; } }\n\
         fn first_bad(xs: int[]): int { let n = 0; for x in xs { try { if x < 0 { break; } if x == 0 { continue; } n += trunc(10 / x); } catch { n = -100; } } n }\n\
         fn kinds(): string { let out = \"\"; try { try { throw \"inner\"; } catch e { out = out + e; throw \"outer\"; } } catch e2 { out = out + \"+\" + e2; } out }\n\
         fn missing(): string { let m = { a: 1 }; let r = \"found\"; try { m.b; } catch e { r = e.kind; } r }\n\
         fn ok(): int { let k = 0; try { k = 5; } catch { k = 9; } k }\n",
        &["safe_div(7, 2)", "safe_div(1, 0)", "first_bad([1, 0, 2, -1, 5])", "first_bad([])", "kinds()", "missing()", "ok()"],
    ),
    // Writes through fields and indexes, and methods that change their
    // receiver, on locals and on state, nested, with `op=`, and failing.
    (
        "type Item = { name: string, qty: int, tags: string[] };\n\
         let cart: Item[] = signal([{ name: \"pen\", qty: 1, tags: [] }]);\n\
         let meta = signal({ count: 0, names: [\"x\"] });\n\
         fn local(): string { let m = { a: 1, list: [1, 2] }; m.a = 5; m.a += 2; m.list[0] = 9; m.list.push(3); `${m.a} ${m.list.join(\",\")}` }\n\
         fn grid(): int { let g = [[0, 0], [0, 0]]; for i in 0..2 { g[i][i] = i + 1; g[i].push(7); } g[1][1] + g[0].length }\n\
         fn more(i: int) { cart[i].qty += 1; cart[i].tags.push(\"more\"); meta.count += 1; }\n\
         fn add(n: string) { cart.push({ name: n, qty: 1, tags: [] }); meta.names.push(n); }\n\
         fn drop_last(): string { let last = cart.pop(); meta.names.pop(); last?.name ?? \"none\" }\n\
         fn bad(): int { let xs = [1]; xs[5] = 2; 0 }\n\
         fn bad_key(): int { let m = { a: 1 }; m.b.c = 2; 0 }\n\
         fn sorted(): string { let xs = [3, 1, 2]; xs.sort(); xs.reverse(); xs.join(\"\") }\n",
        &["local()", "grid()", "!more(0)", "cart[0].qty", "cart[0].tags", "meta.count", "!add(\"ink\")", "cart.length",
          "meta.names", "drop_last()", "drop_last()", "cart.length", "!more(4)", "bad()", "bad_key()", "sorted()"],
    ),
    // Closures created in compiled code: capturing locals, parameters and
    // state, several in one function, inside a loop, and kept for later.
    (
        "let scale = signal(3);\n\
         let saved = signal([]);\n\
         fn scaled(xs: int[]): int[] { xs.map(x => x * scale) }\n\
         fn offset(xs: int[], by: int): int[] { let k = by * 2; xs.map(x => x + k).filter(x => x > k + 1) }\n\
         fn adders(): int { let total = 0; for i in 0..3 { let f = (x: int) => x + i; total += f(10); } total }\n\
         fn keep(n: int) { saved.push((x: int) => x * n); }\n\
         fn run_saved(x: int): int { let s = 0; for f in saved { s += f(x); } s }\n\
         fn nested(): int { let a = 2; let g = (x: int) => [x].map(y => y * a)[0]; g(5) }\n",
        &["scaled([1, 2])", "!scale = 4", "scaled([1, 2])", "offset([1, 5, 9], 2)", "adders()", "!keep(2)", "!keep(5)",
          "run_saved(3)", "nested()"],
    ),
    // Closure bodies compiled: every callback method, a comparator, closures
    // three deep reading captures of captures, a closure writing its own
    // copy of a capture, `return`, loops and `try` inside one, callbacks
    // kept in state (one made by a handler, whose body stays interpreted),
    // and a body with a statement handed back, which runs in a frame.
    (
        "type Row = { name: string, n: int };\n\
         let rows: Row[] = signal([{ name: \"b\", n: 2 }, { name: \"a\", n: 5 }, { name: \"c\", n: 1 }]);\n\
         let hooks = signal([]);\n\
         let hits = signal(0);\n\
         let log = signal(\"\");\n\
         fn total(): int { rows.reduce((acc, r) => acc + r.n, 0) }\n\
         fn names(): string { rows.filter(r => r.n > 1).map(r => r.name.toUpperCase()).join(\"\") }\n\
         fn by_n(): string { let xs = rows.map(r => r); xs.sort((a, b) => a.n - b.n); xs.map(r => r.name).join(\"\") }\n\
         fn finder(k: string): int { let found = rows.find(r => r.name == k); found?.n ?? -1 }\n\
         fn each(): int { let s = 0; rows.forEach(r => { s += r.n; }); s }\n\
         fn counter(): int { let c = 0; let inc = () => { c += 1; c }; inc(); inc(); inc() + c }\n\
         fn deep(): int { let a = 1; let f = (x: int) => { let g = (y: int) => (z: int) => x + y + z + a; let h = g(10); h(100) }; f(1000) }\n\
         fn looped(): int { let f = (n: int) => { let s = 0; for i in 0..n { if i == 3 { continue; } s += i; } s }; f(6) }\n\
         fn guarded(): string { let f = (i: int) => { try { return rows[i].name; } catch e { return \"none\"; } }; f(0) + f(9) }\n\
         fn early(): int { let f = (n: int) => { if n > 2 { return 1; } 0 }; f(3) + f(1) }\n\
         fn arm(n: int) { hooks.push((x: int) => x * n + hits); }\n\
         fn fire(x: int) { for h in hooks { hits = h(x); } }\n\
         fn bad(): int { let f = (x: int) => x + \"a\".length * [1][4]; f(1) }\n\
         async fn note(n: int) { log = log + `${n};`; }\n\
         fn later(): int { let k = 7; let f = () => { note(k); k += 1; k }; f() + f() }\n",
        &["total()", "names()", "by_n()", "finder(\"a\")", "finder(\"z\")", "each()", "counter()", "deep()", "looped()",
          "guarded()", "early()", "!arm(2)", "!fire(3)", "hits", "!fire(3)", "hits",
          "!hooks.push((x: int) => x + 100)", "!fire(1)", "hits", "bad()", "later()", "log"],
    ),
    // `setInterval` started by compiled code (9.7): the body kept as text,
    // the delay worked out first, in a loop, inside a closure body, cleared,
    // kept in state, and with a delay that is not a number.
    (
        "let n = signal(0);\n\
         let timer = signal(0);\n\
         fn start(ms: int): bool { let h = setInterval(ms * 2) { n += 1; }; h > 0 }\n\
         fn twice(): int { let c = 0; for i in 0..2 { let h = setInterval(100 + i) { n += 10; }; if h > 0 { c += 1; } } c }\n\
         fn stop(): bool { let h = setInterval(50) { n = 0; }; clearInterval(h); true }\n\
         fn inner(): bool { let f = () => setInterval(5) { n += 2; }; f() > 0 }\n\
         fn keep() { timer = setInterval(1000) { n += 1; if n >= 5 { clearInterval(timer); } }; }\n\
         fn odd(): bool { let h = setInterval(\"soon\") { n = 1; }; h > 0 }\n",
        &["start(3)", "twice()", "stop()", "inner()", "!keep()", "timer > 0", "odd()", "n"],
    ),
    // Recursion recording its depth: `tests/stack.rs` runs it until it is
    // stopped, to measure what one call costs of Rust's stack. The engines
    // stop at different depths in a debug build, so only a call that
    // returns at once is compared here.
    (
        "let d = signal(0);\n\
         fn deep(n: int): int { d = n; if n < 0 { return 0; } deep(n + 1) }\n",
        &["deep(-1)", "d"],
    ),
    // Typed bodies (step 10): `int`, `float` and `bool` as Rust's own.
    // Failures where Rust's operation stops (overflow, `% 0`, a negative
    // power), placed at a statement and at a caller; typed calling typed,
    // and untyped calling typed; NaN compared; `while` with `break` and
    // `continue`; blocks and `if` as values; and arguments of other kinds,
    // which run the untyped body.
    (
        "fn add(a: int, b: int): int { a + b }\n\
         fn twice(n: int): int { add(n, n) }\n\
         fn placed(n: int): int { let x = add(n, n); x }\n\
         fn over(n: int): int { let x = n; x *= 1000000000; x * 1000000000 }\n\
         fn least(): int { -9223372036854775807 - 1 }\n\
         fn neg(n: int): int { -n }\n\
         fn rem(a: int, b: int): int { a % b }\n\
         fn powi(a: int, b: int): int { a ** b }\n\
         fn fdiv(a: float, b: float): float { a / b }\n\
         fn fmod(a: float, b: float): float { a % b }\n\
         fn idiv(a: int, b: int): float { a / b }\n\
         fn nan_cmp(): bool { let n = 0.0 / 0.0; n < 1.0 || n >= 1.0 || n == n }\n\
         fn nan_ne(): bool { let n = 0.0 / 0.0; n != n && !(n > 0.0) }\n\
         fn mix(a: int, b: float): float { a * 2 + b }\n\
         fn pick(c: bool, a: int, b: int): int { if c { a } else { b } }\n\
         fn loopy(n: int): int { let s = 0; let i = 0; while i < n { i += 1; if i % 3 == 0 { continue; } if i > 50 { break; } s += i; } s }\n\
         fn blocky(n: int): int { let k = { let a = n * 2; a + 1 }; k }\n\
         fn chain(n: int): int { if n <= 0 { return 0; } chain(n - 1) + twice(1) }\n\
         fn ranged(a: int, b: int): int { let s = 0; for i in a..=b { s -= i; } s }\n\
         fn show(n: int): string { `${add(n, 1)}` }\n\
         fn powers(n: int): int { let s = 0; for i in 0..n { s += powi(3, i % 30) % 1000; } s }\n\
         let spent = signal(0);\n\
         fn burn() { let k = chain(3) + powers(50) + loopy(100); while true { spent += 1; } }\n\
         fn burn_deep() { let k = forever(); }\n\
         fn forever(): int { let i = 0; while true { i += 1; } i }\n",
        &["add(2, 3)", "twice(4611686018427387904)", "placed(4611686018427387904)", "over(5)", "over(5000000000)",
          "neg(least())", "neg(4)", "rem(7, 3)", "rem(7, 0)", "rem(least(), -1)", "powi(2, 10)", "powi(2, -1)",
          "powi(2, 70)", "powi(3, 34)", "powi(7, 20)", "powi(2, 62)", "powi(2, 63)", "powi(-2, 63)", "powi(-3, 3)",
          "powi(0, 0)", "fdiv(1.0, 0.0)", "fdiv(0.0, 0.0)", "fmod(7.5, 2.0)", "idiv(7, 2)", "idiv(1, 0)", "nan_cmp()",
          "nan_ne()", "mix(3, 0.5)", "pick(true, 1, 2)", "pick(false, 1, 2)", "loopy(100)", "blocky(3)", "chain(5)",
          "ranged(-2, 3)", "show(41)", "add(1.5, 2)", "add(\"a\", 1)", "pick(1, 2, 3)", "mix(3, 1)", "!burn()", "spent", "burn_deep()", "powers(100)"],
    ),
    // Records (step 10, track (c)): the script-cost apps' sum over 10 000
    // and filter of 2 000, which the cost bench times, and records built,
    // read, written, compared and shown.
    (
        "type Item = { id: int, name: string, price: float, qty: int };\n\
         type User = { name: string, pet?: string };\n\
         let items: Item[] = signal([]);\n\
         let few: Item[] = signal([]);\n\
         fn make(n: int): Item[] { let out: Item[] = []; for i in 0..n { out.push({ id: i, name: `item ${i}`, price: (i % 97) + 0.5, qty: (i % 7) + 1 }); } out }\n\
         fn fill() { items = make(10000); few = make(2000); }\n\
         fn sum(list: Item[]): float { let t = 0.0; for item in list { t += item.price * item.qty; } t }\n\
         fn over(list: Item[], q: int): int { list.filter(item => item.price > q).length }\n\
         fn one(): Item { { qty: 2, name: \"pen\", id: 7, price: 1.5 } }\n\
         fn bumped(): Item { let it = one(); it.qty += 3; it.price = it.price * 2.0; it }\n\
         fn same(): bool { let a: Item = { id: 1, name: \"a\", price: 1.0, qty: 1 }; let b: Item = { qty: 1, price: 1.0, name: \"a\", id: 1 }; a == b }\n\
         fn differ(): bool { let a = one(); let b = bumped(); a == b }\n\
         fn bob(): User { { name: \"bob\" } }\n\
         fn cat(): User { { pet: \"tom\", name: \"ann\" } }\n\
         fn walk(): string { let s = \"\"; for k in keys(one()) { s += k + \",\"; } s }\n\
         type Pt = { y: int, x: int, label?: string };\n\
         fn pt(): Pt { { x: 1, y: 2 } }\n\
         fn labelled(): string { let p = pt(); p.label = \"a\"; p.x += 10; `${p} | ${keys(p)} | ${values(p)}` }\n\
         fn caught(): string { let r = \"\"; try { let n = [1][5]; } catch e { r = `${e} | ${keys(e)}`; } r }\n\
         fn undeclared(): string { let p: any = pt(); let r = \"wrote\"; try { p.z = 1; } catch e { r = `${e.kind}: ${e.message}`; } r }\n\
         fn open_set(): string { let m: any = { b: 1 }; m.set(\"a\", 2); `${m}` }\n\
         fn closed_set(): string { let p: any = pt(); let r = \"\"; try { p.set(\"x\", 5); r = `${p}`; p.set(\"z\", 1); } catch e { r = `${r} ${e.kind}`; } r }\n\
         fn open_order(): string { let m: any = { b: 1, a: 2 }; m.b = 3; `${m}` }\n\
         fn eq_order(): bool { let a: Pt = { x: 1, y: 2 }; let b: Pt = { y: 2, x: 1 }; a == b }\n\
         fn eq_open(): bool { let a: Pt = { x: 1, y: 2 }; let b: any = { x: 1, y: 2 }; a == b }\n\
         fn has(): string { let p = pt(); `${\"x\" in p} ${\"label\" in p} ${\"z\" in p}` }\n\
         fn dict(): string { let d: { [string]: int } = { b: 1, a: 2 }; d[\"c\"] = 3; `${d}` }\n\
         fn results(): string { `${Ok(1)} | ${Err(\"no\")}` }\n\
         fn restock(n: int): int { let it = one(); for i in 0..n { it.qty += 1; it.price = it.price + 0.5; } it.qty }\n\
         fn relabel(): string { let p = pt(); p.label = \"b\"; p.label += \"c\"; let r = \"\"; try { p.x += \"no\"; } catch e { r = e.kind; } `${p} ${r}` }\n\
         fn takes(p: Pt): string { `${p}` }\n\
         fn contexts(): string { let ps: Pt[] = []; ps.push({ x: 1, y: 2 }); let qs: Pt[] = [{ x: 3, y: 4 }]; let n: { p: Pt } = { p: { x: 5, y: 6 } }; `${ps[0]} | ${qs[0]} | ${n.p} | ${takes({ x: 7, y: 8 })}` }\n",
        &["!fill()", "sum(items)", "over(few, 40)", "items.length", "one()", "`${one()}`", "bumped()", "same()", "differ()",
          "one().name", "keys(one())", "values(one())", "walk()", "bob()", "cat()", "`${bob()}`", "keys(bob())", "keys(cat())",
          "bob().name", "cat().pet", "bob().pet", "bob().pet ?? \"no pet\"", "bob() == { name: \"bob\" }", "pt()", "`${pt()}`", "keys(pt())", "pt().label", "labelled()", "caught()",
          "undeclared()", "open_set()", "closed_set()", "open_order()", "eq_order()", "eq_open()", "has()", "dict()", "results()", "contexts()",
          "restock(1000)", "relabel()"],
    ),
];
