//! A class's methods in its normalized disassembly, and which changed between two versions.

use std::collections::{BTreeMap, BTreeSet};

/// A class's methods in its normalized disassembly, by [`Method::key`]. Members are the blocks
/// between blank lines, each opening with its declaration; a field's block has no parameter list.
/// Lambda bodies (`lambda$sample$N`) are kept apart, under the method they're named for, since
/// their numbers are normalized away and several may share a name.
fn methods(body: &str) -> BTreeMap<String, (Method, String)> {
    let mut out: BTreeMap<String, (Method, String)> = BTreeMap::new();
    for block in body.split("\n\n") {
        let Some(declaration) = block.lines().find(|l| !l.is_empty()) else {
            continue;
        };
        let Some(method) = Method::declared(declaration) else {
            continue;
        };
        let entry = out
            .entry(method.key())
            .or_insert_with(|| (method, String::new()));
        entry.1.push_str(block);
        entry.1.push('\n');
    }
    out
}

/// A method as a class's disassembly declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// Its name: `sin`, `<init>` for a constructor, `<clinit>` for the static initializer.
    pub name: String,
    /// Its parameter types without their packages, or `None` for the lambdas of `name`.
    pub params: Option<String>,
}

impl Method {
    /// The method a declaration line declares, if it's a method's.
    fn declared(line: &str) -> Option<Method> {
        if line == "static {};" {
            return Some(Method {
                name: "<clinit>".into(),
                params: Some(String::new()),
            });
        }
        let (head, rest) = line.split_once('(')?;
        let params = rest.rsplit_once(')')?.0;
        let name = head.split_whitespace().last()?;
        if let Some(owner) = name
            .strip_prefix("lambda$")
            .and_then(|n| n.split_once('$'))
            .map(|(owner, _)| owner)
        {
            return Some(Method {
                name: owner.to_string(),
                params: None,
            });
        }
        // A constructor is declared by its class's full name.
        let name = if name.contains('.') { "<init>" } else { name };
        Some(Method {
            name: name.to_string(),
            params: Some(without_packages(params)),
        })
    }

    /// `sin(double)`, or `sample (lambdas)`.
    pub fn key(&self) -> String {
        match &self.params {
            Some(params) => format!("{}({params})", self.name),
            None => format!("{} (lambdas)", self.name),
        }
    }
}

/// Types with their packages cut: `java.util.function.Function<T, R>` → `Function<T, R>`.
fn without_packages(types: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in types.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || matches!(c, '_' | '$' | '.') {
            word.push(c);
        } else {
            out.push_str(word.rsplit('.').next().unwrap_or(&word));
            word.clear();
            out.push(c);
        }
    }
    out.pop();
    out
}

/// How one method changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Lines removed and added, normalized.
    Changed(usize, usize),
    /// The same name and parameter count with other types (`was`, the old ones): a refactor
    /// more often than not, with the lines its body changed.
    Signature {
        was: String,
        minus: usize,
        plus: usize,
    },
    /// A new method, of this many lines.
    Added(usize),
    /// A method gone, of this many lines.
    Removed(usize),
}

impl Change {
    /// How many lines it touches, to put the largest changes first.
    pub fn lines(&self) -> usize {
        match *self {
            Change::Changed(minus, plus) | Change::Signature { minus, plus, .. } => minus + plus,
            Change::Added(lines) | Change::Removed(lines) => lines,
        }
    }
}

/// The methods that differ between two versions of a class's normalized disassembly. A removed
/// method and an added one with the same name and parameter count are taken for one method whose
/// signature changed.
pub(super) fn method_changes(a: &str, b: &str) -> Vec<(Method, Change)> {
    let (before, after) = (methods(a), methods(b));
    let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let mut out = Vec::new();
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    for key in keys {
        match (before.get(key), after.get(key)) {
            (None, Some(new)) => added.push(new),
            (Some(old), None) => removed.push(old),
            (Some((method, x)), Some((_, y))) if x != y => {
                let (minus, plus) = line_changes(x, y);
                out.push((method.clone(), Change::Changed(minus, plus)));
            }
            _ => {}
        }
    }
    for (old, old_body) in removed {
        let same = added.iter().position(|(new, _)| {
            new.name == old.name
                && new.params.as_deref().map(arity) == old.params.as_deref().map(arity)
        });
        match same {
            Some(i) => {
                let (new, new_body) = added.remove(i);
                let (minus, plus) = line_changes(old_body, new_body);
                let was = old.params.clone().unwrap_or_default();
                out.push((new.clone(), Change::Signature { was, minus, plus }));
            }
            None => out.push((old.clone(), Change::Removed(old_body.lines().count()))),
        }
    }
    for (new, new_body) in added {
        out.push((new.clone(), Change::Added(new_body.lines().count())));
    }
    out.sort_by_key(|(method, _)| method.key());
    out
}

/// How many parameters a parameter list has: its commas outside `<>`, plus one.
fn arity(params: &str) -> usize {
    if params.is_empty() {
        return 0;
    }
    let mut depth = 0;
    let mut commas = 0;
    for c in params.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    commas + 1
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Change::Changed(minus, plus) => write!(f, "-{minus} +{plus}"),
            Change::Signature { was, minus, plus } => write!(f, "-{minus} +{plus}, was ({was})"),
            Change::Added(_) => f.write_str("added"),
            Change::Removed(_) => f.write_str("removed"),
        }
    }
}

/// Lines only in `a` and lines only in `b`, counted as multisets.
pub(super) fn line_changes(a: &str, b: &str) -> (usize, usize) {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for line in a.lines() {
        *counts.entry(line).or_default() += 1;
    }
    for line in b.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    let minus = counts
        .values()
        .filter(|&&c| c > 0)
        .map(|&c| c as usize)
        .sum();
    let plus = counts
        .values()
        .filter(|&&c| c < 0)
        .map(|&c| -c as usize)
        .sum();
    (minus, plus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_class_into_methods() {
        let body = "\
public class a.Mth {
private static final float[] SIN;

public a.Mth();
Code:
return

public static float sin(double);
Code:
dload_0
freturn

private static double lambda$sample$N(java.util.function.DoubleSupplier);
Code:
dconst_0
dreturn

static {};
Code:
return
";
        let keys: Vec<String> = methods(body).into_keys().collect();
        assert_eq!(
            keys,
            ["<clinit>()", "<init>()", "sample (lambdas)", "sin(double)"]
        );
        assert_eq!(
            without_packages("java.util.function.Function<a.B, c.D>, int[]"),
            "Function<B, D>, int[]"
        );
    }

    #[test]
    fn lists_changed_added_and_removed_methods() {
        let a = "public class a.A {\n\npublic int f(int);\nCode:\niload_1\nireturn\n\n\
                 public void g();\nCode:\nreturn\n\npublic void k(a.Registry);\nCode:\nreturn\n";
        let b = "public class a.A {\n\npublic int f(int);\nCode:\niconst_0\nireturn\n\n\
                 public void h();\nCode:\nreturn\n\npublic void k(a.Provider);\nCode:\nreturn\n";
        let changes: Vec<String> = method_changes(a, b)
            .iter()
            .map(|(m, c)| format!("{} {c}", m.key()))
            .collect();
        assert_eq!(
            changes,
            [
                "f(int) -1 +1",
                "g() removed",
                "h() added",
                "k(Provider) -1 +1, was (Registry)"
            ]
        );
        assert_eq!(arity("Map<K, V>, int"), 2);
    }

    #[test]
    fn counts_line_changes() {
        assert_eq!(line_changes("a\nb\nb\nc\n", "a\nb\nd\n"), (2, 1));
        assert_eq!(line_changes("x\n", "x\n"), (0, 0));
    }
}
