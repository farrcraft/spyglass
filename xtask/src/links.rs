//! `cargo xtask links`: every relative link in the repo's Markdown points at a file that exists,
//! and every anchor at a heading in it, as GitHub renders them. A commit hook runs it
//! (.pre-commit-config.yaml), so moving a document can't silently break links to it
//! (docs/development-process.md).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{command, repo_root};

/// Whether `file`'s links are placeholders for an author to fill in, not links: document
/// templates, and the skills that write documents from them.
fn placeholders(file: &str) -> bool {
    file == "template.md" || file.ends_with("/template.md") || file.starts_with(".claude/skills/")
}

pub(crate) fn run() -> Result<()> {
    let root = repo_root();
    let output = command("git")
        .args(["ls-files", "-z", "--", "*.md"])
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!("git ls-files failed");
    }
    let listed = String::from_utf8(output.stdout).context("git listed a non-UTF-8 path")?;
    let files: Vec<&str> = listed
        .split('\0')
        .filter(|f| !f.is_empty() && !placeholders(f))
        .collect();
    let broken = check(root, &files)?;
    for problem in &broken {
        println!("{problem}");
    }
    if !broken.is_empty() {
        bail!("{} broken links", broken.len());
    }
    Ok(())
}

/// Every broken link in `files` (paths relative to `root`), as `file:line: problem`.
fn check(root: &Path, files: &[&str]) -> Result<Vec<String>> {
    let mut anchors = Anchors::default();
    let mut broken = Vec::new();
    for file in files {
        let path = root.join(file);
        let text = fs::read_to_string(&path).with_context(|| format!("failed to read {file}"))?;
        for (line, target) in links(&text) {
            if let Some(problem) = check_link(root, &path, target, &mut anchors)? {
                broken.push(format!("{file}:{line}: {problem}"));
            }
        }
    }
    Ok(broken)
}

/// What's wrong with the link to `target` in `file`, if anything.
fn check_link(
    root: &Path,
    file: &Path,
    target: &str,
    anchors: &mut Anchors,
) -> Result<Option<String>> {
    if has_scheme(target) {
        return Ok(None);
    }
    let (path, anchor) = target.split_once('#').unwrap_or((target, ""));
    let path = percent_decode(path);
    let resolved = if path.is_empty() {
        file.to_path_buf()
    } else if let Some(from_root) = path.strip_prefix('/') {
        root.join(from_root)
    } else {
        file.parent().unwrap_or(root).join(&path)
    };
    if !resolved.exists() {
        return Ok(Some(format!("no file {target}")));
    }
    let markdown = resolved.extension().is_some_and(|e| e == "md");
    if anchor.is_empty() || !markdown {
        return Ok(None);
    }
    if anchors.of(&resolved)?.contains(anchor) {
        Ok(None)
    } else {
        Ok(Some(format!("no heading for {target}")))
    }
}

/// Whether `target` names a scheme (`https:`, `mailto:`), so isn't a path in the repo.
fn has_scheme(target: &str) -> bool {
    target.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// `%XX` escapes decoded, as GitHub does before looking up a path.
fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The target of every inline link and image in `text`, with its 1-based line, outside code
/// blocks and code spans.
fn links(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    for (index, line) in prose_lines(text) {
        // Code spans: the text between backticks. Splitting on them keeps every other piece.
        for piece in line.split('`').step_by(2) {
            let mut rest = piece;
            while let Some(start) = rest.find("](") {
                rest = &rest[start + 2..];
                let target = match rest.strip_prefix('<') {
                    Some(inner) => inner.split('>').next().unwrap_or(""),
                    None => rest
                        .split(|c: char| c == ')' || c.is_whitespace())
                        .next()
                        .unwrap_or(""),
                };
                if !target.is_empty() {
                    found.push((index + 1, target));
                }
            }
        }
    }
    found
}

/// The lines of `text` outside fenced code blocks, with their 0-based indexes.
fn prose_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut fence: Option<&str> = None;
    text.lines().enumerate().filter(move |(_, line)| {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                false
            }
            (Some(open), Some(m)) if open == m => {
                fence = None;
                false
            }
            (Some(_), _) => false,
            (None, None) => true,
        }
    })
}

/// Each Markdown file's anchors, read once.
#[derive(Default)]
struct Anchors(HashMap<PathBuf, HashSet<String>>);

impl Anchors {
    fn of(&mut self, path: &Path) -> Result<&HashSet<String>> {
        if !self.0.contains_key(path) {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            self.0.insert(path.to_path_buf(), anchors(&text));
        }
        Ok(&self.0[path])
    }
}

/// The anchors GitHub gives `text`: one per heading, with `-1`, `-2`, ... on repeats, and the
/// `id` or `name` of each `<a>` tag.
fn anchors(text: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (_, line) in prose_lines(text) {
        if let Some(heading) = heading(line) {
            let slug = slug(heading);
            let count = seen.entry(slug.clone()).or_default();
            found.insert(match *count {
                0 => slug,
                n => format!("{slug}-{n}"),
            });
            *count += 1;
        }
        for tag in line.split("<a ").skip(1) {
            let tag = tag.split('>').next().unwrap_or("");
            for attribute in ["id=\"", "name=\""] {
                if let Some(value) = tag.split(attribute).nth(1) {
                    found.insert(value.split('"').next().unwrap_or("").to_string());
                }
            }
        }
    }
    found
}

/// An ATX heading's text (`## Text ##`), or `None` if `line` isn't one.
fn heading(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let line = &line[indent..];
    let level = line.len() - line.trim_start_matches('#').len();
    let text = &line[level..];
    if !(1..=6).contains(&level) || !(text.is_empty() || text.starts_with([' ', '\t'])) {
        return None;
    }
    let text = text.trim();
    // A closing run of #s, if a space separates it from the text.
    let unclosed = text.trim_end_matches('#');
    Some(if unclosed.is_empty() || unclosed.ends_with([' ', '\t']) {
        unclosed.trim_end()
    } else {
        text
    })
}

/// GitHub's anchor for a heading: the rendered text lowercased, with everything but letters,
/// digits, spaces, `-` and `_` dropped, then spaces as `-`.
fn slug(heading: &str) -> String {
    let mut rendered = String::new();
    let mut rest = heading;
    // A link renders as its text: drop each `(target)` that follows a `]`.
    while let Some(start) = rest.find("](") {
        rendered.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .split_once(')')
            .map_or("", |(_, after)| after);
    }
    rendered.push_str(rest);
    rendered
        .to_lowercase()
        .chars()
        .filter(|&c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_match_github() {
        assert_eq!(
            slug("Normal noise (`NormalNoise`)"),
            "normal-noise-normalnoise"
        );
        assert_eq!(
            slug("Point and volume evaluation"),
            "point-and-volume-evaluation"
        );
        assert_eq!(slug("[Front ends](../x.md#y) & more"), "front-ends--more");
        assert_eq!(slug("`spyglass_data::versions`"), "spyglass_dataversions");
        assert_eq!(
            slug("0026. How versions coexist"),
            "0026-how-versions-coexist"
        );
    }

    #[test]
    fn headings() {
        assert_eq!(heading("## Text"), Some("Text"));
        assert_eq!(heading("### Text ###"), Some("Text"));
        assert_eq!(heading("# C#"), Some("C#"));
        assert_eq!(heading("#hashtag"), None);
        assert_eq!(heading("    # indented code"), None);
        assert_eq!(heading("####### seven"), None);
    }

    #[test]
    fn repeated_headings_and_html_anchors() {
        let found = anchors("# A\n## A\n```\n# Not a heading\n```\n<a id=\"here\"></a>\n# A\n");
        let mut found: Vec<_> = found.into_iter().collect();
        found.sort();
        assert_eq!(found, ["a", "a-1", "a-2", "here"]);
    }

    #[test]
    fn links_skip_code() {
        let text = "[a](a.md) and `[b](b.md)` and ![c](<c d.png> \"title\")\n\
                    ```\n[e](e.md)\n```\n\
                    [f](f.md#g) [h](https://example.com)\n";
        assert_eq!(
            links(text),
            [
                (1, "a.md"),
                (1, "c d.png"),
                (5, "f.md#g"),
                (5, "https://example.com")
            ]
        );
    }

    #[test]
    fn schemes() {
        assert!(has_scheme("https://example.com"));
        assert!(has_scheme("mailto:someone@example.com"));
        assert!(!has_scheme("../docs/a.md"));
        assert!(!has_scheme("a.md#b:c"));
    }

    #[test]
    fn percent_escapes() {
        assert_eq!(percent_decode("a%20b.md"), "a b.md");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn finds_broken_links_and_anchors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/a.md"),
            "# Top\n\
             [ok](b.md#yes) [ok](b.md) [ok](#top) [ok](/docs/b.md) [ok](c.rs#L3)\n\
             [ok](https://example.com/missing.md) [ok](../docs)\n\
             [bad](b.md#no) [bad](c.md) [bad](#bottom)\n",
        )
        .unwrap();
        fs::write(root.join("docs/b.md"), "## Yes\n").unwrap();
        fs::write(root.join("docs/c.rs"), "").unwrap();
        assert_eq!(
            check(root, &["docs/a.md", "docs/b.md"]).unwrap(),
            [
                "docs/a.md:4: no heading for b.md#no",
                "docs/a.md:4: no file c.md",
                "docs/a.md:4: no heading for #bottom",
            ]
        );
    }
}
