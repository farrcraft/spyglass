//! The query parser: lines into tokens, tokens into constraints (`docs/specs/search.md`).

use spyglass_data::dimension::Dimension;

use crate::query::{
    Anchor, Constraint, Count, DEFAULT_STEP, Find, Importance, Kind, LootItem, MAX_COORDINATE,
    MAX_ENCHANTMENT_LEVEL, MAX_FEATURE_SIDE, MAX_HEIGHT, MAX_HEIGHT_SAMPLES, MAX_LOOT_SIDE,
    MAX_PLACE_SIDE, MAX_SAMPLES, Place, Region, Sampling, Target, keyword,
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

/// The query language's words, which a bare id can't be (`minecraft:end` can).
const KEYWORDS: [&str; 30] = [
    "find",
    "feature",
    "loot",
    "with",
    "here",
    "apart",
    "point",
    "height",
    "flat",
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
    "overworld",
    "nether",
    "end",
];

/// A word of a line, and its column.
#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    /// The word.
    text: &'a str,
    /// Its first character's column, from 1.
    column: usize,
}

/// Splits a line into tokens, dropping its comment. A `#` at the start of a word starts a
/// comment when it's the line's first word or a word on its own; otherwise it's a tag, as in
/// `#minecraft:village`.
fn tokens(line: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start: Option<(usize, usize)> = None; // byte offset and column of the current token
    let mut chars = line.char_indices().enumerate().peekable();
    // `index` counts characters, for columns; `at` is the byte offset, for slicing.
    while let Some((index, (at, c))) = chars.next() {
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
            start = Some((at, index + 1));
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

/// A parsed query: its `find` line and its constraints, each with its line.
pub(crate) struct ParsedQuery {
    /// The `find` line's number and what it asks for, if there is one.
    pub find: Option<(usize, Find)>,
    /// Each constraint with its line's number, in order.
    pub constraints: Vec<(usize, Constraint)>,
}

/// Parses a whole query: an optional `find` line before its constraints.
pub(crate) fn query(text: &str) -> Result<ParsedQuery, ParseError> {
    let mut out = ParsedQuery {
        find: None,
        constraints: Vec::new(),
    };
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
            here: out.find.is_some(),
        };
        if parser.peek_is("find") {
            if out.find.is_some() {
                return Err(parser.error("a query has at most one `find` line"));
            }
            if !out.constraints.is_empty() {
                return Err(parser.error("the `find` line comes before every constraint"));
            }
            out.find = Some((i + 1, parser.find()?));
        } else {
            out.constraints.push((i + 1, parser.constraint()?));
        }
    }
    if out.find.is_none() && out.constraints.is_empty() {
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
    /// The line's number, from 1, for errors.
    line: usize,
    /// The line's tokens.
    tokens: &'t [Token<'a>],
    /// The index of the next token to read.
    next: usize,
    /// The column just past the last token, where "end of line" errors point.
    end_column: usize,
    /// Whether `here` may be an anchor: a `find` line came first.
    here: bool,
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

    /// `find TARGET [place] [sampling] [apart D]`.
    fn find(&mut self) -> Parsed<Find> {
        self.keyword("find")?;
        let target = if self.eat("biome") {
            Target::Biome(self.biome()?)
        } else if self.eat("feature") {
            Target::Feature(self.id("a feature")?)
        } else if self.eat("loot") {
            Target::Loot(self.loot_item()?)
        } else if self.eat("point") {
            Target::Point
        } else {
            Target::Structure(self.structure()?)
        };
        // `within R` alone is around the origin the search is given.
        let origin = self.peek_is("within")
            && !self
                .tokens
                .get(self.next + 2)
                .is_some_and(|t| t.text == "of");
        let start = self.peek();
        let place = if origin {
            self.next += 1;
            Place::Within {
                radius: self.radius()?,
                anchor: Anchor::Origin,
            }
        } else {
            // The target isn't its own anchor.
            let here = std::mem::replace(&mut self.here, false);
            let place = self.place(true);
            self.here = here;
            place?
        };
        if matches!(target, Target::Feature(_)) && !place.fits_features() {
            return Err(self.too_wide(start, Limited::Features));
        }
        if matches!(target, Target::Loot(_)) && !place.fits_loot() {
            return Err(self.too_wide(start, Limited::Loot));
        }
        let sampling = match target {
            Target::Structure(_) | Target::Feature(_) | Target::Loot(_) => Sampling::default(),
            Target::Biome(_) | Target::Point => self.sampling(&place)?,
        };
        let apart = if self.eat("apart") {
            self.distance()?
        } else {
            target.default_apart()
        };
        if self.peek().is_some() {
            return Err(self.expected("the end of the line"));
        }
        Ok(Find {
            target,
            place,
            sampling,
            apart,
        })
    }

    fn constraint(&mut self) -> Parsed<Constraint> {
        let required = match self.peek().map(|t| t.text) {
            Some("require") => true,
            Some("prefer") => false,
            _ => return Err(self.expected("`require` or `prefer`")),
        };
        self.next += 1;
        let dimension = Dimension::ALL
            .into_iter()
            .find(|&d| self.peek_is(keyword(d)));
        if dimension.is_some() {
            self.next += 1;
        }
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
        Ok(Constraint {
            importance,
            dimension,
            kind,
        })
    }

    fn kind(&mut self) -> Parsed<Kind> {
        let first = self.peek().ok_or_else(|| self.expected("a constraint"))?;
        match first.text {
            "cluster" => {
                self.next += 1;
                let size = self.number("a cluster size", 2, 64)?;
                let feature = self.eat("feature");
                let target = if feature {
                    self.id("a feature")?
                } else {
                    self.structure()?
                };
                self.keyword("within")?;
                let apart = self.distance()?;
                for word in ["of", "each", "other"] {
                    self.keyword(word)?;
                }
                let place = if feature {
                    self.limited_place(Limited::Features)?
                } else {
                    self.place(true)?
                };
                if feature {
                    return Ok(Kind::FeatureCluster {
                        size,
                        feature: target,
                        apart,
                        place,
                    });
                }
                let structure = target;
                Ok(Kind::Cluster {
                    size,
                    structure,
                    apart,
                    place,
                })
            }
            "biome" => {
                self.next += 1;
                let biome = self.biome()?;
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
            "height" => {
                self.next += 1;
                let one = self.signed("a height", MAX_HEIGHT)?;
                self.keyword("to")?;
                let other = self.signed("a height", MAX_HEIGHT)?;
                let place = self.place(true)?;
                let step = self.height_step(&place)?;
                Ok(Kind::Height {
                    min: one.min(other),
                    max: one.max(other),
                    place,
                    step,
                })
            }
            "flat" => {
                self.next += 1;
                let range = self.number("a height difference", 0, MAX_HEIGHT.unsigned_abs())?;
                let place = self.place(true)?;
                let step = self.height_step(&place)?;
                Ok(Kind::Flat { range, place, step })
            }
            _ => {
                let count = self.count()?;
                if count == Count::No && self.eat("biome") {
                    let biome = self.biome()?;
                    return self.biome_presence(false, biome);
                }
                if self.eat("slime") {
                    let region = self.region()?;
                    return Ok(Kind::Slime { count, region });
                }
                if self.eat("feature") {
                    let feature = self.id("a feature")?;
                    let place = self.limited_place(Limited::Features)?;
                    return Ok(Kind::Features {
                        count,
                        feature,
                        place,
                    });
                }
                if self.eat("loot") {
                    let item = self.loot_item()?;
                    let place = self.limited_place(Limited::Loot)?;
                    return Ok(Kind::Loot { count, item, place });
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
        // `within R of` starts both forms: the word after `of` tells them apart, and for a
        // place the parser goes back to read it whole.
        let start = self.next;
        if self.eat("within") {
            let radius = self.radius()?;
            self.keyword("of")?;
            let anchor = self.peek().ok_or_else(|| self.expected("an anchor"))?;
            if !matches!(anchor.text, "spawn" | "here") && point(anchor.text).is_none() {
                let structure = self.structure()?;
                let place = self.place(true)?;
                return Ok(Region::NearStructure {
                    radius,
                    structure,
                    place,
                });
            }
            self.next = start;
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
                } else if self.peek_is("here") {
                    if !self.here {
                        return Err(self.error(
                            "`here` is the target of a `find` line, which this query doesn't have \
                             before this line",
                        ));
                    }
                    self.next += 1;
                    Anchor::Here
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
                let corner = self.point("a point `x,z`")?;
                self.keyword("to")?;
                let other = self.point("a point `x,z`")?;
                if (0..2).any(|i| corner[i].abs_diff(other[i]) >= MAX_PLACE_SIDE) {
                    let message = format!("a box may be at most {MAX_PLACE_SIDE} blocks on a side");
                    return Err(match first {
                        Some(t) => self.error_at(t, message),
                        None => self.error(message),
                    });
                }
                Ok(Place::Box {
                    min: [corner[0].min(other[0]), corner[1].min(other[1])],
                    max: [corner[0].max(other[0]), corner[1].max(other[1])],
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

    /// The place of a feature count or cluster, or of a loot count, which is at most
    /// [`MAX_FEATURE_SIDE`] or [`MAX_LOOT_SIDE`] blocks on a side.
    fn limited_place(&mut self, limited: Limited) -> Parsed<Place> {
        let start = self.peek();
        let place = self.place(true)?;
        let fits = match limited {
            Limited::Features => place.fits_features(),
            Limited::Loot => place.fits_loot(),
        };
        if fits {
            Ok(place)
        } else {
            Err(self.too_wide(start, limited))
        }
    }

    /// A feature or loot place too wide to place, starting at `start`.
    fn too_wide(&self, start: Option<Token<'_>>, limited: Limited) -> ParseError {
        let (what, side) = match limited {
            Limited::Features => ("features are", MAX_FEATURE_SIDE),
            Limited::Loot => ("containers are", MAX_LOOT_SIDE),
        };
        let message =
            format!("{what} placed over at most {side} blocks on a side: use a smaller place");
        match start {
            Some(t) => self.error_at(t, message),
            None => self.error(message),
        }
    }

    /// `[step S]` for a terrain condition, checking the grid's size.
    fn height_step(&mut self, place: &Place) -> Parsed<u32> {
        let start = self.peek();
        let step = if self.eat("step") {
            self.number("a step", 1, 4096)?
        } else {
            DEFAULT_STEP
        };
        if place.grid_bound(step) > MAX_HEIGHT_SAMPLES {
            let message = format!(
                "this height grid has over {MAX_HEIGHT_SAMPLES} points: use a larger `step` or a \
                 smaller place"
            );
            return Err(match start {
                Some(t) => self.error_at(t, message),
                None => self.error(message),
            });
        }
        Ok(step)
    }

    /// A biome id, or `#` and a biome tag.
    fn biome(&mut self) -> Parsed<String> {
        self.id_or_tag("a biome")
    }

    fn structure(&mut self) -> Parsed<String> {
        self.id_or_tag("a structure")
    }

    /// `ITEM [with ENCHANTMENT [LEVEL]]`: an item id or `#` and an item tag.
    fn loot_item(&mut self) -> Parsed<LootItem> {
        let item = self.id_or_tag("an item")?;
        let enchantment = if self.eat("with") {
            let enchantment = self.id("an enchantment")?;
            let level = match self.peek() {
                Some(t) if t.text.starts_with(|c: char| c.is_ascii_digit()) => {
                    Some(self.number("an enchantment level", 1, MAX_ENCHANTMENT_LEVEL)?)
                }
                _ => None,
            };
            Some((enchantment, level))
        } else {
            None
        };
        Ok(LootItem { item, enchantment })
    }

    /// An id, or `#` and a tag id; `what` names it in errors.
    fn id_or_tag(&mut self, what: &str) -> Parsed<String> {
        let token = self.peek().ok_or_else(|| self.expected(what))?;
        match token.text.strip_prefix('#') {
            Some(tag) => {
                self.next += 1;
                let id = full_id(tag)
                    .ok_or_else(|| self.error_at(token, format!("`{}` isn't a tag", token.text)))?;
                Ok(format!("#{id}"))
            }
            None => self.id(what),
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
        let xz = point(token.text).ok_or_else(|| {
            self.error_at(token, format!("expected {what}, found `{}`", token.text))
        })?;
        if xz
            .iter()
            .any(|c| c.unsigned_abs() > MAX_COORDINATE.unsigned_abs())
        {
            return Err(self.error_at(
                token,
                format!("coordinates must be -{MAX_COORDINATE} to {MAX_COORDINATE}"),
            ));
        }
        Ok(xz)
    }
}

/// What a place is limited for: features or loot, each placing chunks.
#[derive(Clone, Copy)]
enum Limited {
    Features,
    Loot,
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
    let valid = |part: &str, extra: &[u8]| {
        !part.is_empty()
            && part.bytes().all(|b| {
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
                dimension: None,
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
                dimension: None,
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

    #[test]
    fn dimensions() {
        let c = parse(
            "require nether fortress within 100 of spawn\nprefer end end_city in 0,0 to 9,9 weight 3",
        );
        assert_eq!(c[0].dimension, Some(Dimension::Nether));
        assert_eq!(c[1].dimension, Some(Dimension::End));
        assert_eq!(
            c[1].to_string(),
            "prefer end minecraft:end_city in 0,0 to 9,9 weight 3"
        );
        assert_eq!(parse("prefer igloo within 5 of spawn")[0].dimension, None);
        let mut query = Query::parse("require overworld igloo within 5 of spawn").unwrap();
        assert_eq!(
            query.to_string(),
            "require overworld minecraft:igloo within 5 of spawn\n"
        );
        assert!(query.forget_dimension(0));
        assert!(!query.forget_dimension(1));
        assert_eq!(
            query.to_string(),
            "require minecraft:igloo within 5 of spawn\n"
        );
        // The dimension comes first, once, and is never an id.
        for (text, column) in [
            ("require igloo nether within 5 of spawn", 15),
            ("require nether nether igloo within 5 of spawn", 16),
            ("require nether end within 5 of spawn", 16),
        ] {
            assert_eq!(error(text).column, column, "{text}");
        }
        assert_eq!(
            parse("require minecraft:end within 5 of spawn")[0].kind,
            Kind::Structures {
                count: Count::AtLeast(1),
                structure: "minecraft:end".to_string(),
                region: Region::Place(Place::Within {
                    radius: 5,
                    anchor: Anchor::Spawn
                }),
            }
        );
    }

    #[test]
    fn find_lines() {
        let q = Query::parse(
            "# Two huts close enough to share an AFK spot.\n\
             find swamp_hut within 4096\n\
             require swamp_hut within 200 of here",
        )
        .unwrap();
        assert_eq!(
            q.find(),
            Some(&Find {
                target: Target::Structure("minecraft:swamp_hut".to_string()),
                place: Place::Within {
                    radius: 4096,
                    anchor: Anchor::Origin
                },
                sampling: Sampling::default(),
                apart: 0,
            })
        );
        assert_eq!(q.find_line(), Some(2));
        assert_eq!(q.lines(), [3]);
        assert_eq!(
            q.to_string(),
            "find minecraft:swamp_hut within 4096\n\
             require minecraft:swamp_hut within 200 of here\n"
        );
        // A grid target, its sampling and `apart`; an explicit place.
        let q = Query::parse("find biome #is_ocean within 500 of spawn at y 40 step 8 apart 64")
            .unwrap();
        let find = q.find().unwrap();
        assert_eq!(
            find.target,
            Target::Biome("#minecraft:is_ocean".to_string())
        );
        assert_eq!(find.sampling, Sampling { y: 40, step: 8 });
        assert_eq!(find.apart, 64);
        assert!(q.constraints().is_empty());
        assert_eq!(
            q.to_string(),
            "find biome #minecraft:is_ocean within 500 of spawn at y 40 step 8 apart 64\n"
        );
        // `apart` is printed only when it isn't the target's default.
        assert_eq!(
            Query::parse("find point in 0,0 to 99,99 apart 256")
                .unwrap()
                .to_string(),
            "find point in 0,0 to 99,99\n"
        );
        assert_eq!(
            Query::parse("find village within 9 of 1,2 apart 0")
                .unwrap()
                .to_string(),
            "find minecraft:village within 9 of 1,2\n"
        );
        // `here` anchors anything that takes a place.
        let c = parse(
            "find point within 100\n\
             require spawn within 50 of here\n\
             prefer igloo within 10 of village within 64 of here\n\
             require no biome desert within 0 of here",
        );
        assert!(matches!(
            c[0].kind,
            Kind::Spawn {
                place: Place::Within {
                    anchor: Anchor::Here,
                    ..
                }
            }
        ));
        assert!(matches!(
            &c[1].kind,
            Kind::Structures {
                region: Region::NearStructure {
                    place: Place::Within {
                        anchor: Anchor::Here,
                        ..
                    },
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn features() {
        let q = Query::parse(
            "find feature monster_room within 256\n\
             require cluster 2 feature minecraft:monster_room within 16 of each other within 100 of spawn\n\
             prefer no feature desert_well in 0,0 to 1023,1023 weight 3\n\
             require 2 feature monster_room_deep within 32 of here",
        )
        .unwrap();
        assert_eq!(
            q.find().unwrap().target,
            Target::Feature("minecraft:monster_room".to_string())
        );
        assert_eq!(
            q.constraints()[0].kind,
            Kind::FeatureCluster {
                size: 2,
                feature: "minecraft:monster_room".to_string(),
                apart: 16,
                place: Place::Within {
                    radius: 100,
                    anchor: Anchor::Spawn
                },
            }
        );
        assert_eq!(
            q.constraints()[1].kind,
            Kind::Features {
                count: Count::No,
                feature: "minecraft:desert_well".to_string(),
                place: Place::Box {
                    min: [0, 0],
                    max: [1023, 1023]
                },
            }
        );
        assert_eq!(
            q.to_string(),
            "find feature minecraft:monster_room within 256\n\
             require cluster 2 feature minecraft:monster_room within 16 of each other within 100 of spawn\n\
             prefer no feature minecraft:desert_well in 0,0 to 1023,1023 weight 3\n\
             require 2 feature minecraft:monster_room_deep within 32 of here\n"
        );
        // Places are at most 1,024 blocks on a side, and features have no structure anchors.
        assert!(Query::parse("require feature monster_room within 511 of 0,0").is_ok());
        for (text, column) in [
            ("require feature monster_room within 512 of 0,0", 30),
            ("require feature monster_room in 0,0 to 1024,5", 30),
            ("find feature lake_lava_surface within 600", 32),
            (
                "require cluster 2 feature monster_room within 9 of each other within 600 of spawn",
                63,
            ),
        ] {
            let e = error(text);
            assert_eq!(e.column, column, "{text}: {e}");
            assert!(e.message.contains("1024"), "{text}: {e}");
        }
        assert_eq!(
            error("require feature monster_room within 9 of village within 9 of spawn").column,
            42
        );
        assert_eq!(error("require feature within 9 of spawn").column, 17);
    }

    #[test]
    fn loot() {
        let q = Query::parse(
            "find loot #minecraft:planks within 1000\n\
             require loot enchanted_golden_apple within 1000 of spawn\n\
             prefer 2 loot minecraft:enchanted_book with mending in 0,0 to 2047,9 weight 4\n\
             require no loot enchanted_book with minecraft:sharpness 5 within 64 of here",
        )
        .unwrap();
        let planks = LootItem {
            item: "#minecraft:planks".to_string(),
            enchantment: None,
        };
        assert_eq!(q.find().unwrap().target, Target::Loot(planks));
        assert_eq!(q.find().unwrap().apart, 0);
        assert_eq!(
            q.constraints()[0].kind,
            Kind::Loot {
                count: Count::AtLeast(1),
                item: LootItem {
                    item: "minecraft:enchanted_golden_apple".to_string(),
                    enchantment: None,
                },
                place: Place::Within {
                    radius: 1000,
                    anchor: Anchor::Spawn
                },
            }
        );
        assert_eq!(
            q.constraints()[2].kind,
            Kind::Loot {
                count: Count::No,
                item: LootItem {
                    item: "minecraft:enchanted_book".to_string(),
                    enchantment: Some(("minecraft:sharpness".to_string(), Some(5))),
                },
                place: Place::Within {
                    radius: 64,
                    anchor: Anchor::Here
                },
            }
        );
        assert_eq!(
            q.to_string(),
            "find loot #minecraft:planks within 1000\n\
             require loot minecraft:enchanted_golden_apple within 1000 of spawn\n\
             prefer 2 loot minecraft:enchanted_book with minecraft:mending in 0,0 to 2047,9 weight 4\n\
             require no loot minecraft:enchanted_book with minecraft:sharpness 5 within 64 of here\n"
        );
        // Places are at most 2,048 blocks on a side.
        assert!(Query::parse("require loot apple within 1023 of 0,0").is_ok());
        for (text, column) in [
            ("require loot apple within 1024 of 0,0", 20),
            ("require loot apple in 0,0 to 2048,5", 20),
            ("find loot apple within 1024", 17),
        ] {
            let e = error(text);
            assert_eq!(e.column, column, "{text}: {e}");
            assert!(e.message.contains("2048"), "{text}: {e}");
        }
        for (text, column) in [
            // No structure anchors, no clusters, and levels from 1 to 255.
            (
                "require loot apple within 9 of village within 9 of spawn",
                32,
            ),
            (
                "require cluster 2 loot apple within 9 of each other within 9 of spawn",
                19,
            ),
            ("require loot within 9 of spawn", 14),
            ("require loot book with within 9 of spawn", 24),
            ("require loot book with mending 0 within 9 of spawn", 32),
            ("require loot book with mending 256 within 9 of spawn", 32),
            ("require loot #Bad within 9 of spawn", 14),
        ] {
            let e = error(text);
            assert_eq!(e.column, column, "{text}: {e}");
        }
    }

    #[test]
    fn biome_tags_and_terrain() {
        let c = parse(
            "require biome #required_ocean_monument_surrounding covers 100% within 128 of 0,0\n\
             prefer no biome #c:is_dry within 5 of spawn\n\
             require height 80 to 62 within 48 of spawn step 4\n\
             prefer flat 3 in 0,0 to 31,31",
        );
        assert!(matches!(
            &c[0].kind,
            Kind::Coverage { biome, .. } if biome == "#minecraft:required_ocean_monument_surrounding"
        ));
        assert!(matches!(&c[1].kind, Kind::Biome { biome, .. } if biome == "#c:is_dry"));
        assert_eq!(
            c[2].kind,
            Kind::Height {
                min: 62,
                max: 80,
                place: Place::Within {
                    radius: 48,
                    anchor: Anchor::Spawn
                },
                step: 4,
            }
        );
        assert_eq!(
            c[2].to_string(),
            "require height 62 to 80 within 48 of spawn step 4"
        );
        assert_eq!(c[3].to_string(), "prefer flat 3 in 0,0 to 31,31");
    }

    #[test]
    fn find_errors_have_positions() {
        let cases = [
            // `here` needs a `find` line before it.
            ("require igloo within 5 of here", (1, 27)),
            (
                "require igloo within 5 of spawn\nfind igloo within 5",
                (2, 1),
            ),
            ("find igloo within 5\nfind igloo within 5", (2, 1)),
            // The target isn't its own anchor.
            ("find igloo within 5 of here", (1, 24)),
            ("find igloo within 5 step 4", (1, 21)),
            ("find biome within 5", (1, 12)),
            ("find biome #Bad within 5", (1, 12)),
            ("find point within 4096 step 4", (1, 24)),
            ("find point within 5 apart -1", (1, 27)),
            ("find", (1, 5)),
            ("find point", (1, 11)),
            ("require height 5 within 5 of spawn", (1, 18)),
            ("require height 1 to 5000 within 5 of spawn", (1, 21)),
            ("require flat -1 within 5 of spawn", (1, 14)),
            ("require flat 3 within 4096 of spawn step 8", (1, 37)),
            ("require flat 3 within 64 of spawn at y 3", (1, 35)),
        ];
        for (text, at) in cases {
            let e = error(text);
            assert_eq!((e.line, e.column), at, "{text:?}: {e}");
        }
    }

    /// The spec's query examples: its `text` blocks that start like a query.
    fn spec_examples() -> Vec<String> {
        let spec = include_str!("../../../docs/specs/search.md");
        spec.split("```text\n")
            .skip(1)
            .filter_map(|block| block.split_once("```").map(|(body, _)| body.to_string()))
            .filter(|body| {
                let first = body.trim_start();
                ["#", "require", "prefer", "find"]
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
            let query = Query::parse(text).unwrap_or_else(|e| panic!("{text}\n{e}"));
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
            "overworld",
            "nether",
            "end",
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
            "find",
            "here",
            "apart",
            "point",
            "height",
            "flat",
            "#is_ocean",
            "feature",
            "monster_room",
            "loot",
            "with",
            "mending",
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
                    assert_eq!(again.find(), q.find(), "{tokens:?}");
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
