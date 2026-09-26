//! The language's functions and methods.
//!
//! The names and answers are the ones the fork gave: JavaScript's where Rux
//! took them (`length`, `includes`, `slice`, `forEach`), and rhai's own where
//! the fork kept them (`len`, `contains`, `index_of`). Both spellings stay
//! until `docs/11-next.md`'s "Names that change" retires one.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::value::V;
use super::{dynamic, fail, Flow, Interp, R};
use crate::{ElementAction, ElementHandle, Nav};

/// Whether a method changes its receiver, so a call on a place writes the
/// place: `items.push(x)`, `list.sort()`.
pub(super) fn mutates(name: &str) -> bool {
    matches!(
        name,
        "push" | "append" | "pop" | "shift" | "insert" | "remove" | "clear" | "truncate" | "reverse" | "sort"
            | "splice" | "retain" | "dedup" | "drain" | "pad" | "replace" | "make_upper" | "make_lower"
            | "unshift" | "fill"
    )
}

fn args_of<const N: usize>(name: &str, argv: &[V]) -> R<[V; N]> {
    match <[V; N]>::try_from(argv.to_vec()) {
        Ok(a) => Ok(a),
        Err(_) => {
            let types: Vec<&str> = argv.iter().map(V::type_name).collect();
            fail(format!("Function not found: {name} ({})", types.join(", ")))
        }
    }
}

fn text(v: &V) -> R<Rc<str>> {
    match v {
        V::Str(s) => Ok(Rc::clone(s)),
        other => fail(format!("expected text, not a {}", other.type_name())),
    }
}

fn number(v: &V) -> R<f64> {
    v.number().ok_or(()).or_else(|_| fail(format!("expected a number, not a {}", v.type_name())))
}

/// `slice`'s bounds, JavaScript's: out of range clamps, inverted is empty.
fn bounds(len: usize, start: f64, end: Option<f64>) -> (usize, usize) {
    let resolve = |v: f64| -> usize {
        if v < 0.0 {
            (len as f64 + v).max(0.0) as usize
        } else {
            (v as usize).min(len)
        }
    };
    let from = resolve(start);
    let to = end.map_or(len, resolve);
    (from, to.max(from))
}

/// A whole-number answer where the type says `int`: an `int` in stays one.
fn whole_of(v: &V, f: impl Fn(f64) -> f64) -> R<V> {
    match v {
        V::Int(i) => Ok(V::Int(*i)),
        V::Float(x) => {
            let y = f(*x);
            if y.is_finite() && y.abs() < 9.2e18 {
                Ok(V::Int(y as i64))
            } else {
                Ok(V::Float(y))
            }
        }
        other => fail(format!("expected a number, not a {}", other.type_name())),
    }
}

pub(super) fn element_field(el: &ElementHandle, name: &str) -> R<V> {
    let b = el.facts.bounds;
    let dim = |f: fn(&crate::ElementBox) -> f32| b.map_or(V::None, |b| V::Float(f(&b) as f64));
    Ok(match name {
        "tag" => V::str(el.facts.tag.as_str()),
        "id" => el.facts.id.as_deref().map_or(V::None, V::str),
        "classes" => V::array(el.facts.classes.iter().map(|c| V::str(c.as_str())).collect()),
        "x" => dim(|b| b.x),
        "y" => dim(|b| b.y),
        "width" => dim(|b| b.width),
        "height" => dim(|b| b.height),
        _ => return fail(format!("Property not found: {name} on an Element")),
    })
}

fn map_of(entries: impl IntoIterator<Item = (&'static str, V)>) -> V {
    V::Map(Rc::new(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>()))
}

impl Interp {
    /// A function of the language's, by name.
    pub(super) fn builtin(&mut self, name: &str, argv: Vec<V>) -> R<V> {
        let n = argv.len();
        Ok(match (name, n) {
            ("print", _) => {
                let line = argv.iter().map(V::display).collect::<Vec<_>>().join(" ");
                crate::log_line(line);
                V::None
            }
            ("debug", _) => {
                let line = argv.iter().map(V::display).collect::<Vec<_>>().join(" ");
                crate::log_line(line);
                V::None
            }
            ("signal", 1) => argv.into_iter().next().unwrap_or(V::None),
            ("emit", 1 | 2) => {
                let mut a = argv.into_iter();
                let name = text(&a.next().unwrap_or(V::None))?;
                let payload = a.next().map(|p| p.to_value());
                crate::EMISSIONS.with(|e| e.borrow_mut().push((name.to_string(), payload)));
                V::None
            }
            ("navigate", 1) => {
                let path = text(&argv[0])?;
                crate::NAVIGATIONS.with(|v| v.borrow_mut().push(Nav::To(path.to_string())));
                V::None
            }
            ("replace", 1) => {
                let path = text(&argv[0])?;
                crate::NAVIGATIONS.with(|v| v.borrow_mut().push(Nav::Replace(path.to_string())));
                V::None
            }
            ("back", 0) => {
                crate::NAVIGATIONS.with(|v| v.borrow_mut().push(Nav::Back));
                V::None
            }
            ("forward", 0) => {
                crate::NAVIGATIONS.with(|v| v.borrow_mut().push(Nav::Forward));
                V::None
            }
            ("path_for" | "pathFor", 1 | 2) => {
                let name = text(&argv[0])?;
                let values: Vec<(String, rux_reactive::Value)> = match argv.get(1) {
                    Some(V::Map(m)) => m.iter().map(|(k, v)| (k.clone(), v.to_value())).collect(),
                    Some(other) => return fail(format!("path_for takes a map of values, not a {}", other.type_name())),
                    None => Vec::new(),
                };
                V::str(crate::build_named_path(&name, &values))
            }
            ("blur", 0) => {
                crate::ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Blur));
                V::None
            }
            (crate::SUBMIT_FN, 1) => {
                let form = text(&argv[0])?;
                let path = form.split('.').filter_map(|i| i.parse().ok()).collect();
                crate::ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(ElementAction::Submit(path)));
                V::None
            }
            ("query", 1) => {
                let selector = text(&argv[0])?;
                let resolver = crate::ELEMENTS.with(|e| e.borrow().clone());
                let Some(resolver) = resolver else {
                    return fail(
                        "query() is only available inside a handler, because a binding that reads the tree \
                         would rebuild the tree it read",
                    );
                };
                let Some(found) = resolver(&selector) else {
                    return fail(format!("`{selector}` is not a selector this can match"));
                };
                V::array(found.into_iter().map(|facts| V::Element(Rc::new(ElementHandle { facts }))).collect())
            }
            ("clearInterval", 1) => {
                let id = argv[0].number().unwrap_or(0.0);
                crate::TIMER_REQUESTS.with(|t| t.borrow_mut().push(crate::TimerRequest::Cancel(id)));
                V::None
            }
            ("Ok", 1) => map_of([("ok", V::Bool(true)), ("value", argv.into_iter().next().unwrap_or(V::None))]),
            ("Err", 1) => map_of([("ok", V::Bool(false)), ("error", argv.into_iter().next().unwrap_or(V::None))]),
            ("Number", 1) => match &argv[0] {
                V::Str(s) => V::Float(crate::js_number(s)),
                V::Bool(b) => V::Float(if *b { 1.0 } else { 0.0 }),
                V::Int(i) => V::Float(*i as f64),
                V::Float(f) => V::Float(*f),
                other => return fail(format!("Function not found: Number ({})", other.type_name())),
            },
            ("parseInt", 1 | 2) => match &argv[0] {
                V::Str(s) => {
                    let radix = argv.get(1).and_then(V::number).unwrap_or(10.0) as u32;
                    let n = crate::js_parse_int(s, radix);
                    if n.is_nan() {
                        V::None
                    } else {
                        V::Int(n as i64)
                    }
                }
                V::Int(i) => V::Int(*i),
                V::Float(f) => V::Int(f.trunc() as i64),
                other => return fail(format!("Function not found: parseInt ({})", other.type_name())),
            },
            ("parseFloat", 1) => match &argv[0] {
                V::Str(s) => {
                    let n = crate::js_parse_float(s);
                    if n.is_nan() {
                        V::None
                    } else {
                        V::Float(n)
                    }
                }
                V::Int(i) => V::Float(*i as f64),
                V::Float(f) => V::Float(*f),
                other => return fail(format!("Function not found: parseFloat ({})", other.type_name())),
            },
            ("parse_int" | "to_int", 1) => match &argv[0] {
                V::Str(s) => match s.trim().parse::<i64>() {
                    Ok(i) => V::Int(i),
                    Err(_) => return fail(format!("Error parsing integer: '{s}'")),
                },
                v => whole_of(v, f64::trunc)?,
            },
            ("parse_float" | "to_float", 1) => match &argv[0] {
                V::Str(s) => match s.trim().parse::<f64>() {
                    Ok(f) => V::Float(f),
                    Err(_) => return fail(format!("Error parsing floating-point number: '{s}'")),
                },
                v => V::Float(number(v)?),
            },
            ("String" | "to_string", 1) => V::str(argv[0].display()),
            ("isNaN", 1) => V::Bool(argv[0].number().is_some_and(f64::is_nan)),
            ("type_of", 1) => V::str(argv[0].type_name()),
            ("intDiv", 2) => {
                let (a, b) = (number(&argv[0])?.trunc(), number(&argv[1])?.trunc());
                if b == 0.0 {
                    return fail("intDiv(a, 0): an `int` cannot be divided by zero");
                }
                V::Int((a / b).trunc() as i64)
            }
            ("toFloat", 1) => V::Float(number(&argv[0])?),
            ("trunc", 1) => whole_of(&argv[0], f64::trunc)?,
            ("floor", 1) => whole_of(&argv[0], f64::floor)?,
            ("ceil", 1) => whole_of(&argv[0], f64::ceil)?,
            ("round", 1) => whole_of(&argv[0], f64::round)?,
            ("abs", 1) => match &argv[0] {
                V::Int(i) => V::Int(i.checked_abs().ok_or_else(super::overflow)?),
                v => V::Float(number(v)?.abs()),
            },
            ("min" | "max", _) if n >= 1 => {
                let mut best = argv[0].clone();
                for v in &argv[1..] {
                    let better = dynamic(if name == "min" { "<" } else { ">" }, v.clone(), best.clone())?;
                    if better.truthy() {
                        best = v.clone();
                    }
                }
                if argv.iter().all(|v| matches!(v, V::Int(_))) {
                    best
                } else {
                    V::Float(number(&best)?)
                }
            }
            ("sqrt", 1) => V::Float(number(&argv[0])?.sqrt()),
            ("sin", 1) => V::Float(number(&argv[0])?.sin()),
            ("cos", 1) => V::Float(number(&argv[0])?.cos()),
            ("tan", 1) => V::Float(number(&argv[0])?.tan()),
            ("atan2", 2) | ("atan", 2) => V::Float(number(&argv[0])?.atan2(number(&argv[1])?)),
            ("exp", 1) => V::Float(number(&argv[0])?.exp()),
            ("ln", 1) => V::Float(number(&argv[0])?.ln()),
            ("log", 1) | ("log10", 1) => V::Float(number(&argv[0])?.log10()),
            ("log", 2) => V::Float(number(&argv[0])?.log(number(&argv[1])?)),
            ("keys", 1) => match &argv[0] {
                V::Map(m) => V::array(m.keys().map(|k| V::str(k.as_str())).collect()),
                other => return fail(format!("Function not found: keys ({})", other.type_name())),
            },
            ("values", 1) => match &argv[0] {
                V::Map(m) => V::array(m.values().cloned().collect()),
                other => return fail(format!("Function not found: values ({})", other.type_name())),
            },
            ("range", 2) => match (argv[0].whole(), argv[1].whole()) {
                (Some(a), Some(b)) => V::Range(a, b),
                _ => return fail("range needs two whole numbers"),
            },
            ("is_def_fn", _) => V::Bool(match argv.first() {
                Some(V::Str(s)) => self.unit.fns.iter().any(|f| f.name.as_str() == s.as_ref()),
                _ => false,
            }),
            ("is_def_var", 1) => V::Bool(match &argv[0] {
                V::Str(s) => self.has_global(s),
                _ => false,
            }),
            ("__interval", 2) => {
                let ms = argv[0].number().unwrap_or(0.0);
                V::Float(crate::start_interval(ms, text(&argv[1])?.to_string()))
            }
            _ => {
                let types: Vec<&str> = argv.iter().map(V::type_name).collect();
                return fail(format!("Function not found: {name} ({})", types.join(", ")));
            }
        })
    }

    /// A method of the language's on `recv`, which it may change. `None` when
    /// there is no such method for that kind of value.
    pub(super) fn method(&mut self, recv: &mut V, name: &str, argv: &[V]) -> R<Option<V>> {
        let n = argv.len();
        let out = match recv {
            V::Array(items) => match (name, n) {
                ("len", 0) => V::Int(items.len() as i64),
                ("is_empty", 0) => V::Bool(items.is_empty()),
                ("push" | "append", 1) => {
                    let [x] = args_of::<1>(name, argv)?;
                    let list = Rc::make_mut(items);
                    match (name, x) {
                        ("append", V::Array(more)) => list.extend(more.iter().cloned()),
                        (_, x) => list.push(x),
                    }
                    V::None
                }
                ("pop", 0) => Rc::make_mut(items).pop().unwrap_or(V::None),
                ("shift", 0) => {
                    let list = Rc::make_mut(items);
                    if list.is_empty() {
                        V::None
                    } else {
                        list.remove(0)
                    }
                }
                ("insert", 2) => {
                    let list = Rc::make_mut(items);
                    let at = argv[0].whole().ok_or(()).or_else(|_| fail("insert at a whole number"))?;
                    let len = list.len() as i64;
                    let at = if at < 0 { (len + at).max(0) } else { at.min(len) } as usize;
                    list.insert(at, argv[1].clone());
                    V::None
                }
                ("remove", 1) => {
                    let list = Rc::make_mut(items);
                    let len = list.len() as i64;
                    match argv[0].whole() {
                        Some(i) if (-len..len).contains(&i) => {
                            let at = if i < 0 { len + i } else { i } as usize;
                            list.remove(at)
                        }
                        Some(_) => V::None,
                        None => return fail("remove takes a whole number"),
                    }
                }
                ("clear", 0) => {
                    Rc::make_mut(items).clear();
                    V::None
                }
                ("truncate", 1) => {
                    let to = argv[0].whole().unwrap_or(0).max(0) as usize;
                    Rc::make_mut(items).truncate(to);
                    V::None
                }
                ("reverse", 0) => {
                    Rc::make_mut(items).reverse();
                    V::None
                }
                ("sort", 0) => {
                    let list = Rc::make_mut(items);
                    let mut failed = false;
                    list.sort_by(|a, b| match (a, b) {
                        (V::Str(x), V::Str(y)) => x.cmp(y),
                        _ => match (a.number(), b.number()) {
                            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                            _ => {
                                failed = true;
                                std::cmp::Ordering::Equal
                            }
                        },
                    });
                    if failed {
                        return fail("sort() without a comparison sorts numbers or texts, not a mix");
                    }
                    V::None
                }
                ("sort", 1) => {
                    let f = argv[0].clone();
                    let mut list = items.as_ref().clone();
                    // A merge sort that can stop on the comparison's error.
                    let mut err = None;
                    let mut keyed: Vec<V> = std::mem::take(&mut list);
                    merge_sort(&mut keyed, &mut |a, b| {
                        if err.is_some() {
                            return false;
                        }
                        match self.call_value(f.clone(), vec![a.clone(), b.clone()]) {
                            Ok(d) => d.number().unwrap_or(0.0) > 0.0,
                            Err(e) => {
                                err = Some(e);
                                false
                            }
                        }
                    });
                    if let Some(e) = err {
                        return Err(e);
                    }
                    *items = Rc::new(keyed);
                    V::Array(Rc::clone(items))
                }
                ("map", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let mut out = Vec::with_capacity(list.len());
                    for (i, x) in list.iter().enumerate() {
                        out.push(self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?);
                    }
                    V::array(out)
                }
                ("filter", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let mut out = Vec::new();
                    for (i, x) in list.iter().enumerate() {
                        if self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy() {
                            out.push(x.clone());
                        }
                    }
                    V::array(out)
                }
                ("retain", 1) => {
                    let f = argv[0].clone();
                    let list = items.as_ref().clone();
                    let mut keep = Vec::new();
                    let mut removed = Vec::new();
                    for (i, x) in list.into_iter().enumerate() {
                        if self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy() {
                            keep.push(x);
                        } else {
                            removed.push(x);
                        }
                    }
                    *items = Rc::new(keep);
                    V::array(removed)
                }
                ("drain", 1) => {
                    let f = argv[0].clone();
                    let list = items.as_ref().clone();
                    let mut keep = Vec::new();
                    let mut removed = Vec::new();
                    for (i, x) in list.into_iter().enumerate() {
                        if self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy() {
                            removed.push(x);
                        } else {
                            keep.push(x);
                        }
                    }
                    *items = Rc::new(keep);
                    V::array(removed)
                }
                ("dedup", 0) => {
                    Rc::make_mut(items).dedup();
                    V::None
                }
                ("find", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let mut found = V::None;
                    for (i, x) in list.iter().enumerate() {
                        if self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy() {
                            found = x.clone();
                            break;
                        }
                    }
                    found
                }
                ("findIndex", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let mut at = -1;
                    for (i, x) in list.iter().enumerate() {
                        if self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy() {
                            at = i as i64;
                            break;
                        }
                    }
                    V::Int(at)
                }
                ("some" | "every", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let every = name == "every";
                    let mut answer = every;
                    for (i, x) in list.iter().enumerate() {
                        let t = self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?.truthy();
                        if t != every {
                            answer = !every;
                            break;
                        }
                    }
                    V::Bool(answer)
                }
                ("forEach", 1) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    for (i, x) in list.iter().enumerate() {
                        self.call_value(f.clone(), vec![x.clone(), V::Int(i as i64)])?;
                    }
                    V::None
                }
                ("reduce", 1 | 2) => {
                    let f = argv[0].clone();
                    let list = Rc::clone(items);
                    let mut acc = argv.get(1).cloned().unwrap_or(V::None);
                    for (i, x) in list.iter().enumerate() {
                        acc = self.call_value(f.clone(), vec![acc, x.clone(), V::Int(i as i64)])?;
                    }
                    acc
                }
                ("includes" | "contains", 1) => V::Bool(items.iter().any(|x| *x == argv[0])),
                ("indexOf" | "index_of", 1) => {
                    V::Int(items.iter().position(|x| *x == argv[0]).map_or(-1, |i| i as i64))
                }
                ("slice", 1 | 2) => {
                    let end = match argv.get(1) {
                        Some(e) => Some(number(e)?),
                        None => None,
                    };
                    let (from, to) = bounds(items.len(), number(&argv[0])?, end);
                    V::array(items[from..to].to_vec())
                }
                ("extract", 1 | 2) => {
                    let len = items.len() as i64;
                    let start = argv[0].whole().unwrap_or(0);
                    let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                    let end = match argv.get(1) {
                        Some(c) => (start + c.whole().unwrap_or(0).max(0) as usize).min(items.len()),
                        None => items.len(),
                    };
                    V::array(items[start..end].to_vec())
                }
                ("splice", 2 | 3) => {
                    let list = Rc::make_mut(items);
                    let len = list.len() as i64;
                    let start = argv[0].whole().unwrap_or(0);
                    let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                    let count = (argv[1].whole().unwrap_or(0).max(0) as usize).min(list.len() - start);
                    let with: Vec<V> = match argv.get(2) {
                        Some(V::Array(w)) => w.as_ref().clone(),
                        Some(x) => vec![x.clone()],
                        None => Vec::new(),
                    };
                    let removed: Vec<V> = list.splice(start..start + count, with).collect();
                    V::array(removed)
                }
                ("join", 0 | 1) => {
                    let sep = match argv.first() {
                        Some(s) => text(s)?.to_string(),
                        None => ",".to_string(),
                    };
                    V::str(items.iter().map(V::display).collect::<Vec<_>>().join(&sep))
                }
                ("get", 1) => match argv[0].whole() {
                    Some(i) => {
                        let len = items.len() as i64;
                        let at = if i < 0 { len + i } else { i };
                        if (0..len).contains(&at) {
                            items[at as usize].clone()
                        } else {
                            V::None
                        }
                    }
                    None => V::None,
                },
                ("to_string", 0) => V::str(recv.display()),
                _ => return Ok(None),
            },
            V::Str(s) => match (name, n) {
                ("len", 0) => V::Int(s.chars().count() as i64),
                ("is_empty", 0) => V::Bool(s.is_empty()),
                ("trim", 0) => V::str(s.trim()),
                ("toUpperCase" | "to_upper", 0) => V::str(s.to_uppercase()),
                ("toLowerCase" | "to_lower", 0) => V::str(s.to_lowercase()),
                ("make_upper", 0) => {
                    *s = s.to_uppercase().into();
                    V::None
                }
                ("make_lower", 0) => {
                    *s = s.to_lowercase().into();
                    V::None
                }
                ("startsWith" | "starts_with", 1) => V::Bool(s.starts_with(text(&argv[0])?.as_ref())),
                ("endsWith" | "ends_with", 1) => V::Bool(s.ends_with(text(&argv[0])?.as_ref())),
                ("includes" | "contains", 1) => V::Bool(match &argv[0] {
                    V::Str(p) => s.contains(p.as_ref()),
                    other => return fail(format!("Function not found: contains (string, {})", other.type_name())),
                }),
                ("indexOf" | "index_of", 1) => {
                    let p = text(&argv[0])?;
                    V::Int(s.find(p.as_ref()).map_or(-1, |i| s[..i].chars().count() as i64))
                }
                ("repeat", 1) => V::str(s.repeat(number(&argv[0])?.max(0.0) as usize)),
                ("charAt", 1) => {
                    let i = number(&argv[0])?.max(0.0) as usize;
                    V::str(s.chars().nth(i).map(String::from).unwrap_or_default())
                }
                ("slice" | "substring", 1 | 2) => {
                    let chars: Vec<char> = s.chars().collect();
                    let end = match argv.get(1) {
                        Some(e) => Some(number(e)?),
                        None => None,
                    };
                    let (from, to) = bounds(chars.len(), number(&argv[0])?, end);
                    V::str(chars[from..to].iter().collect::<String>())
                }
                ("sub_string", 1 | 2) => {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i64;
                    let start = argv[0].whole().unwrap_or(0);
                    let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                    let end = match argv.get(1) {
                        Some(c) => (start + c.whole().unwrap_or(0).max(0) as usize).min(chars.len()),
                        None => chars.len(),
                    };
                    V::str(chars[start..end].iter().collect::<String>())
                }
                ("split", 0) => V::array(s.split_whitespace().map(V::str).collect()),
                ("split", 1) => {
                    let sep = text(&argv[0])?;
                    V::array(s.split(sep.as_ref()).map(V::str).collect())
                }
                ("chars" | "to_chars", 0) => V::array(s.chars().map(|c| V::str(c.to_string())).collect()),
                ("replace", 2) => {
                    let (a, b) = (text(&argv[0])?, text(&argv[1])?);
                    *s = s.replace(a.as_ref(), b.as_ref()).into();
                    V::None
                }
                ("pad", 2) => {
                    let to = argv[0].whole().unwrap_or(0).max(0) as usize;
                    let with = text(&argv[1])?;
                    let mut out = s.to_string();
                    while out.chars().count() < to && !with.is_empty() {
                        out.push_str(&with);
                    }
                    *s = out.into();
                    V::None
                }
                ("clear", 0) => {
                    *s = "".into();
                    V::None
                }
                ("truncate", 1) => {
                    let to = argv[0].whole().unwrap_or(0).max(0) as usize;
                    *s = s.chars().take(to).collect::<String>().into();
                    V::None
                }
                ("to_string", 0) => V::Str(Rc::clone(s)),
                ("get", 1) => match argv[0].whole() {
                    Some(i) => s.chars().nth(i.max(0) as usize).map_or(V::None, |c| V::str(c.to_string())),
                    None => V::None,
                },
                _ => return Ok(None),
            },
            V::Int(_) | V::Float(_) => {
                let v = recv.clone();
                match (name, n) {
                    ("to_int" | "trunc", 0) => whole_of(&v, f64::trunc)?,
                    ("floor", 0) => whole_of(&v, f64::floor)?,
                    ("ceil" | "ceiling", 0) => whole_of(&v, f64::ceil)?,
                    ("round", 0) => whole_of(&v, f64::round)?,
                    ("to_float" | "toFloat", 0) => V::Float(number(&v)?),
                    ("sqrt", 0) => V::Float(number(&v)?.sqrt()),
                    ("abs", 0) => self.builtin("abs", vec![v])?,
                    ("to_string", 0) => V::str(v.display()),
                    ("is_nan", 0) => V::Bool(v.number().is_some_and(f64::is_nan)),
                    ("min" | "max", 1) => self.builtin(name, vec![v, argv[0].clone()])?,
                    _ => return Ok(None),
                }
            }
            V::Map(m) => match (name, n) {
                ("keys", 0) => V::array(m.keys().map(|k| V::str(k.as_str())).collect()),
                ("values", 0) => V::array(m.values().cloned().collect()),
                ("len", 0) => V::Int(m.len() as i64),
                ("is_empty", 0) => V::Bool(m.is_empty()),
                ("contains", 1) => V::Bool(m.contains_key(text(&argv[0])?.as_ref())),
                ("get", 1) => m.get(text(&argv[0])?.as_ref()).cloned().unwrap_or(V::None),
                ("remove", 1) => Rc::make_mut(m).remove(text(&argv[0])?.as_ref()).unwrap_or(V::None),
                ("clear", 0) => {
                    Rc::make_mut(m).clear();
                    V::None
                }
                ("to_string", 0) => V::str(recv.display()),
                ("unwrap", 0) => match m.get("ok") {
                    Some(V::Bool(true)) => m.get("value").cloned().unwrap_or(V::None),
                    Some(V::Bool(false)) => {
                        let error = m.get("error").map(V::display).unwrap_or_default();
                        return Err(Flow::Fault(super::Fault {
                            message: format!("unwrap() on an error: {error}"),
                            kind: "unwrap",
                            thrown: None,
                        }));
                    }
                    _ => return fail("unwrap() is for a `Result`, made by `Ok(…)` or `Err(…)`"),
                },
                _ => return Ok(None),
            },
            V::Range(a, b) => match (name, n) {
                ("len", 0) => V::Int((*b - *a).max(0)),
                ("contains", 1) => V::Bool(argv[0].whole().is_some_and(|x| *a <= x && x < *b)),
                _ => return Ok(None),
            },
            V::Element(el) => {
                let path = el.facts.path.clone();
                let action = match (name, n) {
                    ("focus", 0) => ElementAction::Focus(path),
                    ("scrollIntoView", 0) => ElementAction::ScrollIntoView(path),
                    ("tap", 0) => ElementAction::Tap(path),
                    _ => return Ok(None),
                };
                crate::ELEMENT_ACTIONS.with(|a| a.borrow_mut().push(action));
                V::None
            }
            V::Fn(_) => match name {
                "call" => {
                    let f = recv.clone();
                    self.call_value(f, argv.to_vec())?
                }
                _ => return Ok(None),
            },
            V::Bool(_) | V::None => match (name, n) {
                ("to_string", 0) => V::str(recv.display()),
                _ => return Ok(None),
            },
        };
        Ok(Some(out))
    }
}

/// A stable merge sort ordered by `greater(a, b)`, which a script's
/// comparison answers and may fail inside.
fn merge_sort(v: &mut Vec<V>, greater: &mut dyn FnMut(&V, &V) -> bool) {
    if v.len() <= 1 {
        return;
    }
    let right = v.split_off(v.len() / 2);
    let mut left = std::mem::take(v);
    let mut right = right;
    merge_sort(&mut left, greater);
    merge_sort(&mut right, greater);
    let (mut l, mut r) = (left.into_iter().peekable(), right.into_iter().peekable());
    while let (Some(a), Some(b)) = (l.peek(), r.peek()) {
        if greater(a, b) {
            v.push(r.next().expect("peeked"));
        } else {
            v.push(l.next().expect("peeked"));
        }
    }
    v.extend(l);
    v.extend(r);
}
