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
         fn half(n: int): float { n / 2 }\n",
        &["fib(0)", "fib(1)", "fib(15)", "big()", "half(7)", "fib(\"x\")"],
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
         fn cmp(a: string, b: string): bool { a < b }\n",
        &["greet(none)", "greet(\"ada\")", "both(true, false)", "both(false, false)", "widen(3)", "cmp(\"a\", \"b\")"],
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
];
