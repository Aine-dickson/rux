//! Imports in one spelling.
//!
//! `use` and `import` mean the same (`docs/11-next.md`, "Modules"), and a
//! project may pick one in `rux.toml`:
//!
//! ```toml
//! [fmt]
//! imports = "use"      # or "import"
//! ```
//!
//! Without the setting both are left as written. Either way, a type imported
//! the way it was before `type` marked one (`use types::Task;`, told by its
//! capital letter) becomes `use type types::Task;`, which the runtime has
//! warned about since the change.

use rux_syntax::ast::{Import, ImportName, Imported, StmtKind};

/// Which spelling `rux fmt` writes imports in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Imports {
    /// As written.
    #[default]
    Keep,
    Use,
    Import,
}

impl Imports {
    /// `"use"` or `"import"`, the values `rux.toml` takes.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "use" => Ok(Self::Use),
            "import" => Ok(Self::Import),
            other => Err(format!("`imports = \"{other}\"`: the spellings are \"use\" and \"import\"")),
        }
    }
}

/// `script` with each import written in `style`. Script that does not parse
/// is left alone, as every other rewrite leaves it.
pub(crate) fn rewrite(script: &str, style: Imports) -> String {
    if !script.contains("use") && !script.contains("import") {
        return script.to_string();
    }
    let opts = rux_syntax::Options { declarations: true };
    let Ok(parsed) = rux_syntax::parse(script, opts) else { return script.to_string() };
    let mut edits = Vec::new();
    for stmt in &parsed.stmts {
        let StmtKind::Import(i) = &stmt.kind else { continue };
        let mut i = i.clone();
        let mut changed = false;
        // `use types::Task;`: a type told by its capital letter.
        if let (true, false, Imported::Whole(local)) = (i.is_use, i.is_type, &i.what) {
            let last = i.path.last().map(|p| p.name.clone()).unwrap_or_default();
            if i.path.len() > 1 && local.name == last && last.starts_with(|c: char| c.is_uppercase()) {
                let name = i.path.pop().expect("more than one segment");
                i.is_type = true;
                i.what = Imported::Names(vec![ImportName { name, alias: None }]);
                changed = true;
            }
        }
        match style {
            Imports::Use if !i.is_use => (i.is_use, changed) = (true, true),
            Imports::Import if i.is_use => (i.is_use, changed) = (false, true),
            _ => {}
        }
        if changed {
            edits.push((stmt.span.start as usize, stmt.span.end as usize, spelled(&i)));
        }
    }
    let mut out = script.to_string();
    for (start, end, text) in edits.into_iter().rev() {
        out.replace_range(start..end, &text);
    }
    out
}

/// An import written out in its own spelling, ending in `;`.
fn spelled(i: &Import) -> String {
    let names = |names: &[ImportName]| {
        let list: Vec<String> = names
            .iter()
            .map(|n| match &n.alias {
                Some(a) => format!("{} as {}", n.name.name, a.name),
                None => n.name.name.clone(),
            })
            .collect();
        list.join(", ")
    };
    let segments: Vec<&str> = i.path.iter().map(|p| p.name.as_str()).collect();
    let ty = if i.is_type { "type " } else { "" };
    if i.is_use {
        let path = segments.join("::");
        match &i.what {
            Imported::Whole(local) if segments.last() == Some(&local.name.as_str()) => format!("use {path};"),
            Imported::Whole(local) => format!("use {path} as {};", local.name),
            Imported::Names(n) if n.len() == 1 && n[0].alias.is_none() && i.is_type => {
                format!("use {ty}{path}::{};", n[0].name.name)
            }
            Imported::Names(n) => format!("use {ty}{path}::{{{}}};", names(n)),
        }
    } else {
        let from = format!("\"./{}\"", segments.join("/"));
        match &i.what {
            Imported::Whole(local) => format!("import {} from {from};", local.name),
            Imported::Names(n) => format!("import {ty}{{ {} }} from {from};", names(n)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_told_by_its_capital_letter_is_marked() {
        assert_eq!(rewrite("use types::Task;\nlet a = 1;", Imports::Keep), "use type types::Task;\nlet a = 1;");
        assert_eq!(rewrite("use components::row::Row\n", Imports::Keep), "use type components::row::Row;\n");
        // A component, and what is already right, stay as they are.
        for same in ["use components::card;", "use type types::Task;", "import type { Task } from \"./types\";"] {
            assert_eq!(rewrite(same, Imports::Keep), same);
        }
    }

    #[test]
    fn imports_move_to_the_chosen_spelling_and_back() {
        let pairs = [
            ("use stores::cart;", "import cart from \"./stores/cart\";"),
            ("use stores::cart as basket;", "import basket from \"./stores/cart\";"),
            ("use utils::money::{format, tax as t};", "import { format, tax as t } from \"./utils/money\";"),
            ("use type types::Task;", "import type { Task } from \"./types\";"),
            ("use type types::{Task, Page};", "import type { Task, Page } from \"./types\";"),
        ];
        for (u, i) in pairs {
            assert_eq!(rewrite(u, Imports::Import), i);
            assert_eq!(rewrite(i, Imports::Use), u);
            assert_eq!(rewrite(u, Imports::Use), u);
            assert_eq!(rewrite(i, Imports::Import), i);
        }
        assert_eq!(rewrite("use types::Task;", Imports::Import), "import type { Task } from \"./types\";");
        // Script that does not parse is not touched.
        assert_eq!(rewrite("use stores::cart;\nlet = ;", Imports::Import), "use stores::cart;\nlet = ;");
    }
}
