//! Generate Rust for every script of the corpus, as a release build would.

include!("src/corpus_data.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/corpus_data.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let mut code = String::new();
    let mut install = String::from("pub fn install_all() {\n");
    let mut coverage = String::new();
    for (i, (script, _)) in CORPUS.iter().enumerate() {
        let engine = rux_script::Builder::new()
            .build(script)
            .unwrap_or_else(|e| panic!("corpus script {i} does not build: {e:?}"));
        let g = rux_codegen::generate(engine.ir_unit(), engine.source_hash(), "rux_script::aot");
        code.push_str(&format!("pub mod u{i} {{\n{}\n}}\n", g.code));
        install.push_str(&format!("    u{i}::install();\n"));
        coverage.push_str(&format!(
            "({i}, {}, {}, {}, {}, {}),\n",
            g.functions, g.skipped, g.closures, g.compiled, g.handed_back
        ));
    }
    install.push_str("}\n");
    install.push_str(&format!(
        "pub const MUTATING_IN_CODEGEN: [&str; {}] = {:?};\n",
        rux_codegen::MUTATING.len(),
        rux_codegen::MUTATING
    ));
    code.push_str(&install);
    code.push_str(&format!(
        "/// Per script: functions compiled, skipped, closure bodies compiled,\n\
         /// statements compiled, handed back.\n\
         pub const COVERAGE: &[(usize, usize, usize, usize, usize, usize)] = &[\n{coverage}];\n"
    ));
    std::fs::write(out.join("aot.rs"), code).expect("writing the generated code");
}
