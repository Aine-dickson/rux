//! A prop typed with a native record, given that record from Rust
//! (Urbandrop's UD-011): the record's fields may be of the module's other
//! types, which the component never named.

use std::collections::BTreeMap;

use rux_runtime::native as rn;
use rux_runtime::Document;

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rux_native_props_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn map(fields: &[(&str, rn::Any)]) -> rn::Any {
    rn::Any::Map(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

#[test]
fn a_list_of_native_records_fits_a_prop_typed_with_that_record() {
    let vehicle = || {
        map(&[
            ("record", map(&[("id", rn::Any::Str("v1".into()))])),
            ("label", rn::Any::Str("UBA 123A".into())),
        ])
    };
    rn::Module::new("fleetprops")
        .export(rn::Export::record("Record", &[("id", "string")]))
        .export(rn::Export::record("VehicleView", &[("record", "Record"), ("label", "string")]))
        .export(rn::Export::record("Fleet", &[("vehicles", "VehicleView[]")]))
        .export(rn::Export::function(
            "fleet",
            &[],
            "Fleet",
            rn::Call::sync(move |_| Ok(map(&[("vehicles", rn::Any::Array(vec![vehicle()]))]))),
        ))
        .install();
    let picker = "<template><view><text r-for=\"v in vehicles\">{{ v.label }}/{{ v.record.id }}</text></view></template>\n\
<script>\n  use type native::fleetprops::VehicleView;\n  prop vehicles: VehicleView[] = [];\n</script>";
    let app = "<template><screen><picker :vehicles=\"vehicles\" /></screen></template>\n\
<script>\nuse native::fleetprops;\nuse components::picker;\ncomputed vehicles = fleetprops.fleet().vehicles;\n</script>";
    let dir = project(&[("app.rux", app), ("components/picker.rux", picker)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let mut shown = Vec::new();
    texts(&doc.root, &mut shown);
    assert!(shown.iter().any(|t| t == "UBA 123A/v1"), "{shown:?}");
}
