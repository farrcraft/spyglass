//! The query parser: lines into tokens, tokens into constraints (`docs/specs/search.md`).

use crate::query::{
    Anchor, Constraint, Count, Importance, Kind, MAX_COORDINATE, MAX_PLACE_SIDE, MAX_SAMPLES,
    Place, Region, Sampling,
};

/// Why a query's text didn't parse, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}, column {column}: {message}")]
pub struct ParseError {
    /// The line, from 1.
    pub line: usize,
    /// The column in characters, from 1.
    pub column: usize,
    /// What's wrong.
    pub message: String,
}

const KEYWORDS: [&str; 18] = [
    "require", "prefer", "weight", "no", "cluster", "within", "of", "each", "other", "in", "to",
    "spawn", "slime", "biome", "covers", "at", "y", "step",
];

/// A word of a line, and its column.
#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    column: usize,
}

/// Splits a line into tokens, dropping its comment.
fn tokens(line: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start: Option<(usize, usize)> = None; // byte offset and column of the current token
    let mut chars = line.char_indices().enumerate().peekable();
    while let Some((i, (at, c))) = chars.next() {
        let blank = c == ' ' || c == '\t';
        if blank {
            if let Some((from, column)) = start.take() {
                out.push(Token {
                    text: &line[from..at],
                    column,
                });
            }
            continue;
        }
        if c == '#' && start.is_none() {
            let next_blank = chars
                .peek()
                .is_none_or(|&(_, (_, n))| n == ' ' || n == '\t');
            if out.is_empty() || next_blank {
                return out;
            }
        }
        if start.is_none() {
            start = Some((at, i + 1));
        }
    }
    if let Some((from, column)) = start {
        out.push(Token {
            text: &line[from..],
            column,
        });
    }
    out
}

/// Parses a whole query into its constraints, each with its line.
pub(crate) fn query(text: &str) -> Result<Vec<(usize, Constraint)>, ParseError> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let tokens = tokens(line);
        if tokens.is_empty() {
            continue;
        }
        let mut parser = Parser {
            line: i + 1,
            tokens: &tokens,
            next: 0,
            end_column: tokens
                .last()
                .map_or(1, |t| t.column + t.text.chars().count()),
        };
        out.push((i + 1, parser.constraint()?));
    }
    if out.is_empty() {
        return Err(ParseError {
            line: 1,
            column: 1,
            message: "a query needs at least one constraint".to_string(),
        });
    }
    Ok(out)
}

/// One line's tokens, read left to right.
struct Parser<'t, 'a> {
    line: usize,
    tokens: &'t [Token<'a>],
    next: usize,
    /// The column just past the last token, where "end of line" errors point.
    end_column: usize,
}

type Parsed<T> = Result<T, ParseError>;

impl<'a> Parser<'_, 'a> {
    fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.next).copied()
    }

    fn peek_is(&self, word: &str) -> bool {
        self.peek().is_some_and(|t| t.text == word)
    }

    /// An error at the next token, or at the end of the line.
    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            line: self.line,
            column: self.peek().map_or(self.end_column, |t| t.column),
            message: message.into(),
        }
    }

    /// An error at `token`.
    fn error_at(&self, token: Token<'_>, message: impl Into<String>) -> ParseError {
        ParseError {
            line: self.line,
            column: token.column,
            message: message.into(),
        }
    }

    /// "expected X, found Y" at the next token.
    fn expected(&self, what: &str) -> ParseError {
        match self.peek() {
            Some(t) => self.error(format!("expected {what}, found `{}`", t.text)),
            None => self.error(format!("expected {what} at the end of the line")),
        }
    }

    fn take(&mut self, what: &str) -> Parsed<Token<'a>> {
        let token = self.peek().ok_or_else(|| self.expected(what))?;
        self.next += 1;
        Ok(token)
    }

    fn keyword(&mut self, word: &str) -> Parsed<()> {
        if self.peek_is(word) {
            self.next += 1;
            Ok(())
        } else {
            Err(self.expected(&format!("`{word}`")))
        }
    }

    /// Takes `word` if it's next.
    fn eat(&mut self, word: &str) -> bool {
        let found = self.peek_is(word);
        if found {
            self.next += 1;
        }
        found
    }

    fn constraint(&mut self) -> Parsed<Constraint> {
        let required = match self.peek().map(|t| t.text) {
            Some("require") => true,
            Some("prefer") => false,
            _ => return Err(self.expected("`require` or `prefer`")),
        };
        self.next += 1;
        let kind = self.kind()?;
        let importance = if self.peek_is("weight") {
            if required {
                return Err(self.error("only preferred constraints have a weight"));
            }
            self.next += 1;
            Importance::Preferred {
                weight: self.number("a weight", 1, 1000)?,
            }
        } else if required {
            Importance::Required
        } else {
            Importance::Preferred { weight: 1 }
        };
        if self.peek().is_some() {
            return Err(self.expected("the end of the line"));
        }
        Ok(Constraint { importance, kind })
    }

    fn kind(&mut self) -> Parsed<Kind> {
        let first = self.peek().ok_or_else(|| self.expected("a constraint"))?;
        match first.text {
            "cluster" => {
                self.next += 1;
                let size = self.number("a cluster size", 2, 64)?;
                let structure = self.structure()?;
                self.keyword("within")?;
                let apart = self.distance()?;
                for word in ["of", "each", "other"] {
                    self.keyword(word)?;
                }
                let place = self.place(true)?;
                Ok(Kind::Cluster {
                    size,
                    structure,
                    apart,
                    place,
                })
            }
            "biome" => {
                self.next += 1;
                let biome = self.id("a biome")?;
                if self.eat("covers") {
                    let percent = self.percentage()?;
                    let place = self.place(true)?;
                    let sampling = self.sampling(&place)?;
                    Ok(Kind::Coverage {
                        biome,
                        percent,
                        place,
                        sampling,
                    })
                } else {
                    self.biome_presence(true, biome)
                }
            }
            "spawn" => {
                self.next += 1;
                Ok(Kind::Spawn {
                    place: self.place(false)?,
                })
            }
            _ => {
                let count = self.count()?;
                if count == Count::No && self.eat("biome") {
                    let biome = self.id("a biome")?;
                    return self.biome_presence(false, biome);
                }
                if self.eat("slime") {
                    let region = self.region()?;
                    return Ok(Kind::Slime { count, region });
                }
                let structure = self.structure()?;
                let region = self.region()?;
                Ok(Kind::Structures {
                    count,
                    structure,
                    region,
                })
            }
        }
    }

    fn biome_presence(&mut self, present: bool, biome: String) -> Parsed<Kind> {
        let place = self.place(true)?;
        let sampling = self.sampling(&place)?;
        Ok(Kind::Biome {
            present,
            biome,
            place,
            sampling,
        })
    }

    /// An optional count: `no`, a number, or nothing for 1.
    fn count(&mut self) -> Parsed<Count> {
        if self.eat("no") {
            return Ok(Count::No);
        }
        match self.peek() {
            Some(t) if t.text.starts_with(|c: char| c.is_ascii_digit()) => {
                if t.text == "0" {
                    return Err(self.error("a count of 0: write `no` instead"));
                }
                Ok(Count::AtLeast(self.number("a count", 1, 10_000)?))
            }
            _ => Ok(Count::AtLeast(1)),
        }
    }

    /// `place`, or `within R of STRUCTURE place`.
    fn region(&mut self) -> Parsed<Region> {
        let at = self.next;
        if self.eat("within") {
            let radius = self.radius()?;
            self.keyword("of")?;
            let anchor = self.peek().ok_or_else(|| self.expected("an anchor"))?;
            if anchor.text != "spawn" && point(anchor.text).is_none() {
                let structure = self.structure()?;
                let place = self.place(true)?;
                return Ok(Region::NearStructure {
                    radius,
                    structure,
                    place,
                });
            }
            self.next = at;
        }
        Ok(Region::Place(self.place(true)?))
    }

    /// `within R of spawn|POINT` (spawn only if `spawn_anchor`) or `in POINT to POINT`.
    fn place(&mut self, spawn_anchor: bool) -> Parsed<Place> {
        match self.peek().map(|t| t.text) {
            Some("within") => {
                self.next += 1;
                let radius = self.radius()?;
                self.keyword("of")?;
                let anchor = if self.peek_is("spawn") {
                    if !spawn_anchor {
                        return Err(self.error("the spawn can't be its own anchor"));
                    }
                    self.next += 1;
                    Anchor::Spawn
                } else {
                    let what = if spawn_anchor {
                        "`spawn` or a point `x,z`"
                    } else {
                        "a point `x,z`"
                    };
                    Anchor::Point(self.point(what)?)
                };
                Ok(Place::Within { radius, anchor })
            }
            Some("in") => {
                self.next += 1;
                let first = self.peek();
                let a = self.point("a point `x,z`")?;
                self.keyword("to")?;
                let b = self.point("a point `x,z`")?;
                if (0..2).any(|i| a[i].abs_diff(b[i]) >= MAX_PLACE_SIDE) {
                    let message = format!("a box may be at most {MAX_PLACE_SIDE} blocks on a side");
                    return Err(match first {
                        Some(t) => self.error_at(t, message),
                        None => self.error(message),
                    });
                }
                Ok(Place::Box {
                    min: [a[0].min(b[0]), a[1].min(b[1])],
                    max: [a[0].max(b[0]), a[1].max(b[1])],
                })
            }
            _ => Err(self.expected("`within` or `in`")),
        }
    }

    /// `[at y Y] [step S]`, checking the grid's size.
    fn sampling(&mut self, place: &Place) -> Parsed<Sampling> {
        let mut sampling = Sampling::default();
        let start = self.peek();
        if self.eat("at") {
            self.keyword("y")?;
            sampling.y = self.signed("a height", MAX_COORDINATE)?;
        }
        if self.eat("step") {
            sampling.step = self.number("a step", 1, 4096)?;
        }
        if place.grid_bound(sampling.step) > MAX_SAMPLES {
            let message = format!(
                "this biome grid has over {MAX_SAMPLES} points: use a larger `step` or a smaller place"
            );
            return Err(match start {
                Some(t) => self.error_at(t, message),
                None => self.error(message),
            });
        }
        Ok(sampling)
    }

    fn structure(&mut self) -> Parsed<String> {
        let token = self.peek().ok_or_else(|| self.expected("a structure"))?;
        match token.text.strip_prefix('#') {
            Some(tag) => {
                self.next += 1;
                let id = full_id(tag)
                    .ok_or_else(|| self.error_at(token, format!("`{}` isn't a tag", token.text)))?;
                Ok(format!("#{id}"))
            }
            None => self.id("a structure"),
        }
    }

    fn id(&mut self, what: &str) -> Parsed<String> {
        let token = self.peek().ok_or_else(|| self.expected(what))?;
        if KEYWORDS.contains(&token.text) {
            return Err(self.error(format!(
                "expected {what}, found the keyword `{0}` (write `minecraft:{0}` for an id)",
                token.text
            )));
        }
        let id = full_id(token.text).ok_or_else(|| self.expected(what))?;
        self.next += 1;
        Ok(id)
    }

    fn number(&mut self, what: &str, min: u32, max: u32) -> Parsed<u32> {
        let token = self.take(what)?;
        let value = token
            .text
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| token.text.parse::<u32>().ok())
            .flatten()
            .ok_or_else(|| {
                self.error_at(token, format!("expected {what}, found `{}`", token.text))
            })?;
        if !(min..=max).contains(&value) {
            return Err(self.error_at(token, format!("{what} must be {min} to {max}")));
        }
        Ok(value)
    }

    /// A disc's radius: its bounding box is at most [`MAX_PLACE_SIDE`] on a side.
    fn radius(&mut self) -> Parsed<u32> {
        self.number("a radius", 0, (MAX_PLACE_SIDE - 1) / 2)
    }

    fn distance(&mut self) -> Parsed<u32> {
        self.number("a distance", 0, MAX_COORDINATE.unsigned_abs())
    }

    fn signed(&mut self, what: &str, limit: i32) -> Parsed<i32> {
        let token = self.take(what)?;
        let value = integer(token.text).ok_or_else(|| {
            self.error_at(token, format!("expected {what}, found `{}`", token.text))
        })?;
        if value.unsigned_abs() > limit.unsigned_abs() {
            return Err(self.error_at(token, format!("{what} must be -{limit} to {limit}")));
        }
        Ok(value)
    }

    fn percentage(&mut self) -> Parsed<u32> {
        let token = self.take("a percentage")?;
        let value = token
            .text
            .strip_suffix('%')
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse::<u32>().ok())
            .ok_or_else(|| {
                self.error_at(
                    token,
                    format!("expected a percentage like `25%`, found `{}`", token.text),
                )
            })?;
        if !(1..=100).contains(&value) {
            return Err(self.error_at(token, "a percentage must be 1% to 100%"));
        }
        Ok(value)
    }

    fn point(&mut self, what: &str) -> Parsed<[i32; 2]> {
        let token = self.take(what)?;
        let p = point(token.text).ok_or_else(|| {
            self.error_at(token, format!("expected {what}, found `{}`", token.text))
        })?;
        if p.iter()
            .any(|c| c.unsigned_abs() > MAX_COORDINATE.unsigned_abs())
        {
            return Err(self.error_at(
                token,
                format!("coordinates must be -{MAX_COORDINATE} to {MAX_COORDINATE}"),
            ));
        }
        Ok(p)
    }
}

/// A decimal integer with an optional leading `-`.
fn integer(text: &str) -> Option<i32> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `X,Z`.
fn point(text: &str) -> Option<[i32; 2]> {
    let (x, z) = text.split_once(',')?;
    Some([integer(x)?, integer(z)?])
}

/// A resource location with its namespace, `minecraft` if none is written; `None` if it isn't
/// one.
fn full_id(text: &str) -> Option<String> {
    let (namespace, path) = text.split_once(':').unwrap_or(("minecraft", text));
    let valid = |s: &str, extra: &[u8]| {
        !s.is_empty()
            && s.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b"_.-".contains(&b)
                    || extra.contains(&b)
            })
    };
    (valid(namespace, b"") && valid(path, b"/")).then(|| format!("{namespace}:{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Query;

    fn parse(text: &str) -> Vec<Constraint> {
        Query::parse(text)
            .unwrap_or_else(|e| panic!("{text:?}: {e}"))
            .constraints()
            .to_vec()
    }

    fn error(text: &str) -> ParseError {
        Query::parse(text).expect_err(text)
    }

    #[test]
    fn comments_and_tags() {
        let t: Vec<_> = tokens("require #minecraft:village within 1 of spawn # a comment")
            .iter()
            .map(|t| (t.text, t.column))
            .collect();
        assert_eq!(
            t,
            [
                ("require", 1),
                ("#minecraft:village", 9),
                ("within", 28),
                ("1", 35),
                ("of", 37),
                ("spawn", 40)
            ]
        );
        assert!(tokens("  #comment").is_empty());
        assert!(tokens("#").is_empty());
        assert_eq!(tokens("a #").len(), 1);
        assert_eq!(tokens("a\t#\tb").len(), 1);
        // Columns count characters, not bytes.
        assert_eq!(tokens("é b")[1].column, 3);
    }

    #[test]
    fn structures() {
        let c = parse("require 2 swamp_hut within 600 of spawn");
        assert_eq!(
            c,
            [Constraint {
                importance: Importance::Required,
                kind: Kind::Structures {
                    count: Count::AtLeast(2),
                    structure: "minecraft:swamp_hut".to_string(),
                    region: Region::Place(Place::Within {
                        radius: 600,
                        anchor: Anchor::Spawn
                    }),
                },
            }]
        );
        let c = parse("prefer no #minecraft:village in 10,-5 to -10,5 weight 7");
        assert_eq!(
            c[0],
            Constraint {
                importance: Importance::Preferred { weight: 7 },
                kind: Kind::Structures {
                    count: Count::No,
                    structure: "#minecraft:village".to_string(),
                    region: Region::Place(Place::Box {
                        min: [-10, -5],
                        max: [10, 5]
                    }),
                },
            }
        );
    }

    #[test]
    fn structure_anchors() {
        let c = parse("prefer village within 300 of ancient_city within 3000 of 5,6");
        assert_eq!(
            c[0].kind,
            Kind::Structures {
                count: Count::AtLeast(1),
                structure: "minecraft:village".to_string(),
                region: Region::NearStructure {
                    radius: 300,
                    structure: "minecraft:ancient_city".to_string(),
                    place: Place::Within {
                        radius: 3000,
                        anchor: Anchor::Point([5, 6])
                    },
                },
            }
        );
        let c = parse("require 4 slime within 64 of #a:b in 0,0 to 1,1");
        assert!(matches!(
            c[0].kind,
            Kind::Slime {
                count: Count::AtLeast(4),
                region: Region::NearStructure { .. }
            }
        ));
        // Anchors don't nest.
        let e = error("require village within 1 of mansion within 2 of igloo within 3 of spawn");
        assert_eq!((e.line, e.column), (1, 49));
    }

    #[test]
    fn biomes_and_spawn() {
        let c = parse(
            "prefer biome jungle covers 25% within 1000 of 0,0 at y -10 step 32\n\
             require no biome minecraft:desert within 0 of spawn\n\
             prefer spawn in 1,2 to 3,4",
        );
        assert_eq!(
            c[0].kind,
            Kind::Coverage {
                biome: "minecraft:jungle".to_string(),
                percent: 25,
                place: Place::Within {
                    radius: 1000,
                    anchor: Anchor::Point([0, 0])
                },
                sampling: Sampling { y: -10, step: 32 },
            }
        );
        assert!(matches!(c[1].kind, Kind::Biome { present: false, .. }));
        assert!(matches!(c[2].kind, Kind::Spawn { .. }));
    }

    /// The spec's query examples: its `text` blocks that start like a query.
    fn spec_examples() -> Vec<String> {
        let spec = include_str!("../../../docs/specs/search.md");
        spec.split(
            "```text
",
        )
        .skip(1)
        .filter_map(|block| block.split_once("```").map(|(body, _)| body.to_string()))
        .filter(|body| {
            let first = body.trim_start();
            ["#", "require", "prefer"]
                .iter()
                .any(|p| first.starts_with(p))
        })
        .collect()
    }

    #[test]
    fn spec_examples_parse_and_round_trip() {
        let examples = spec_examples();
        assert!(!examples.is_empty());
        for text in &examples {
            let query = Query::parse(text).unwrap_or_else(|e| {
                panic!(
                    "{text}
{e}"
                )
            });
            let canonical = query.to_string();
            let again = Query::parse(&canonical).unwrap();
            assert_eq!(again.constraints(), query.constraints());
            assert_eq!(again.to_string(), canonical);
        }
        let query = Query::parse(&examples[0]).unwrap();
        assert_eq!(query.constraints().len(), 10);
        assert_eq!(query.lines()[0], 2);
        let canonical = query.to_string();
        assert!(canonical.contains("prefer 3 #minecraft:village within 400 of spawn weight 2\n"));
        assert!(
            canonical
                .contains("prefer biome minecraft:jungle covers 25% within 1000 of 0,0 step 32\n")
        );
    }

    #[test]
    fn errors_have_positions() {
        let cases = [
            ("", (1, 1)),
            ("  # only a comment", (1, 1)),
            ("need 2 igloo within 5 of spawn", (1, 1)),
            ("require 0 igloo within 5 of spawn", (1, 9)),
            ("require igloo within 5", (1, 23)),
            ("require igloo within 5 of", (1, 26)),
            ("require igloo near spawn", (1, 15)),
            ("require spawn within 5 of spawn", (1, 27)),
            ("require igloo within 5 of spawn weight 2", (1, 33)),
            ("prefer igloo within 5 of spawn weight 0", (1, 39)),
            ("prefer igloo within 5 of spawn extra", (1, 32)),
            ("require Igloo within 5 of spawn", (1, 9)),
            ("require # within 5 of spawn", (1, 8)),
            ("require #Bad within 5 of spawn", (1, 9)),
            ("require biome slime within 5 of spawn", (1, 15)),
            ("require biome a covers 0% within 5 of spawn", (1, 24)),
            ("require biome a covers 25 within 5 of spawn", (1, 24)),
            ("require biome a within 32767 of spawn", (1, 38)),
            ("require biome a within 4096 of spawn step 4", (1, 38)),
            ("require igloo within 32768 of spawn", (1, 22)),
            (
                "require igloo within 1 of mansion within 32768 of spawn",
                (1, 42),
            ),
            (
                "require igloo within 32768 of mansion in 0,0 to 1,1",
                (1, 22),
            ),
            ("require igloo in 0,0 to 65536,0", (1, 18)),
            ("require biome a within 5 of spawn step 0", (1, 40)),
            ("require igloo in 1,2 to 3", (1, 25)),
            ("require igloo in 1,2 to 30000001,0", (1, 25)),
            (
                "require cluster 1 igloo within 5 of each other within 5 of spawn",
                (1, 17),
            ),
            (
                "require cluster 2 igloo within 5 of other within 5 of spawn",
                (1, 37),
            ),
            ("\n\nrequire 99999999999 igloo within 5 of spawn", (3, 9)),
        ];
        for (text, at) in cases {
            let e = error(text);
            assert_eq!((e.line, e.column), at, "{text:?}: {e}");
        }
        assert_eq!(
            error("require igloo within 5").to_string(),
            "line 1, column 23: expected `of` at the end of the line"
        );
    }

    #[test]
    fn mutated_queries_never_panic() {
        // Valid lines with tokens deleted, repeated or replaced, by a fixed xorshift sequence.
        let lines: Vec<String> = spec_examples()
            .iter()
            .flat_map(|q| q.lines().map(str::to_string).collect::<Vec<_>>())
            .filter(|l| !l.trim().is_empty())
            .collect();
        let words = [
            "require",
            "prefer",
            "weight",
            "no",
            "cluster",
            "within",
            "of",
            "each",
            "other",
            "in",
            "to",
            "spawn",
            "slime",
            "biome",
            "covers",
            "at",
            "y",
            "step",
            "0",
            "-5",
            "2,3",
            "25%",
            "#",
            "#a:b",
            "igloo",
            "a:b:c",
            ",",
            "%",
            "é",
            "99999999999",
            "-",
        ];
        let mut state = 0x2545_f491_u32;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as usize % n
        };
        let (mut parsed, mut failed) = (0, 0);
        for _ in 0..20_000 {
            let mut tokens: Vec<&str> = lines[next(lines.len())].split(' ').collect();
            for _ in 0..=next(3) {
                let at = next(tokens.len() + 1);
                match next(3) {
                    0 if at < tokens.len() => {
                        tokens.remove(at);
                    }
                    1 if at < tokens.len() => tokens.insert(at, tokens[at]),
                    _ => tokens.insert(at, words[next(words.len())]),
                }
            }
            match Query::parse(&tokens.join(" ")) {
                Ok(q) => {
                    parsed += 1;
                    let again = Query::parse(&q.to_string()).unwrap();
                    assert_eq!(again.constraints(), q.constraints(), "{tokens:?}");
                }
                Err(_) => failed += 1,
            }
        }
        assert!(
            parsed > 100 && failed > 100,
            "{parsed} parsed, {failed} failed"
        );
    }
}
