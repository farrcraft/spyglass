//! The view text: a line that opens the same view in another viewer, with no server
//! (`docs/specs/user-data.md#the-view-text`). Every format the viewer has written stays
//! readable; the tests keep texts of each.

use spyglass::{DEFAULT_MAP_Y, Dimension, MAX_SCALE};

/// What a text starts with.
const PREFIX: &str = "spyglass:";
/// The format this viewer writes, and the newest it reads.
const FORMAT: u32 = 1;
const MAX_LEN: usize = 4_096;
/// How far from 0 a centre may be, as the viewer clamps views.
const MAX_CENTRE: i64 = 1_000_000_000;
const NAMESPACE: &str = "minecraft:";

/// The layers a text can turn on, by the names it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub ties: bool,
    pub slime: bool,
    pub spawn: bool,
    pub confirm: bool,
    pub relief: bool,
    pub surface: bool,
    pub veins: bool,
    pub caves: bool,
    pub carvers: bool,
}

impl Default for Flags {
    /// As a text without `l` means.
    fn default() -> Self {
        Flags {
            ties: false,
            slime: false,
            spawn: true,
            confirm: true,
            relief: false,
            surface: false,
            veins: false,
            caves: false,
            carvers: false,
        }
    }
}

impl Flags {
    fn names(self) -> Vec<&'static str> {
        [
            (self.ties, "ties"),
            (self.slime, "slime"),
            (self.spawn, "spawn"),
            (self.confirm, "confirm"),
            (self.relief, "relief"),
            (self.surface, "surface"),
            (self.veins, "veins"),
            (self.caves, "caves"),
            (self.carvers, "carvers"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
}

/// A view as a text carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedView {
    pub seed: i64,
    /// The game version; `None` means the one shown.
    pub version: Option<String>,
    pub dimension: Dimension,
    /// The block at the centre.
    pub centre: [i32; 2],
    pub scale: u32,
    pub y: i32,
    /// Structure ids and `#tags`, with their namespaces.
    pub structures: Vec<String>,
    /// Biome ids, with their namespaces.
    pub highlight: Vec<String>,
    /// Placed feature ids, with their namespaces.
    pub features: Vec<String>,
    pub flags: Flags,
}

impl SharedView {
    /// The text, in the newest format.
    pub fn encode(&self) -> String {
        let dimension = match self.dimension {
            Dimension::Overworld => "overworld",
            Dimension::Nether => "nether",
            Dimension::End => "end",
        };
        let mut fields = vec![format!("s={}", self.seed)];
        if let Some(v) = &self.version {
            fields.push(format!("v={v}"));
        }
        fields.push(format!("d={dimension}"));
        fields.push(format!("c={},{}", self.centre[0], self.centre[1]));
        fields.push(format!("sc={}", self.scale));
        fields.push(format!("y={}", self.y));
        let short = |ids: &[String]| {
            ids.iter()
                .map(|id| match id.strip_prefix('#') {
                    Some(tag) => format!("#{}", tag.strip_prefix(NAMESPACE).unwrap_or(tag)),
                    None => id.strip_prefix(NAMESPACE).unwrap_or(id).to_string(),
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        if !self.structures.is_empty() {
            fields.push(format!("st={}", short(&self.structures)));
        }
        if !self.highlight.is_empty() {
            fields.push(format!("hl={}", short(&self.highlight)));
        }
        if !self.features.is_empty() {
            fields.push(format!("ft={}", short(&self.features)));
        }
        fields.push(format!("l={}", self.flags.names().join(",")));
        format!("{PREFIX}{FORMAT};{}", fields.join(";"))
    }

    /// The first view text in `pasted`, which may have other words around it.
    pub fn decode(pasted: &str) -> Result<SharedView, String> {
        let start = pasted
            .find(PREFIX)
            .ok_or("there's no view text here: it starts with spyglass:")?;
        let text = pasted[start..]
            .split(char::is_whitespace)
            .next()
            .unwrap_or_default();
        if text.len() > MAX_LEN {
            return Err(format!("a view text is at most {MAX_LEN} characters"));
        }
        let mut parts = text[PREFIX.len()..].split(';');
        let format: u32 = parts
            .next()
            .and_then(|f| f.parse().ok())
            .ok_or("the view text has no format number after spyglass:")?;
        if format == 0 {
            return Err("the view text's format is 0".into());
        }
        if format > FORMAT {
            return Err(format!(
                "the view text is format {format}, from a newer spyglass; this one reads up to \
                 {FORMAT}"
            ));
        }
        let mut seen: Vec<&str> = Vec::new();
        let mut view = SharedView {
            seed: 0,
            version: None,
            dimension: Dimension::Overworld,
            centre: [0, 0],
            scale: 4,
            y: DEFAULT_MAP_Y,
            structures: Vec::new(),
            highlight: Vec::new(),
            features: Vec::new(),
            flags: Flags::default(),
        };
        let mut seed = None;
        for part in parts.filter(|p| !p.is_empty()) {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| format!("{part:?} in the view text has no ="))?;
            if seen.contains(&key) {
                return Err(format!("{key} is in the view text twice"));
            }
            seen.push(key);
            let bad = || format!("{key}={value} in the view text isn't valid");
            match key {
                "s" => seed = Some(value.parse::<i64>().map_err(|_| bad())?),
                "v" if !value.is_empty() && value.len() <= 200 => {
                    view.version = Some(value.to_string());
                }
                "v" => return Err(bad()),
                "d" => {
                    view.dimension = match value {
                        "overworld" => Dimension::Overworld,
                        "nether" => Dimension::Nether,
                        "end" => Dimension::End,
                        _ => return Err(bad()),
                    };
                }
                "c" => {
                    let (x, z) = value.split_once(',').ok_or_else(bad)?;
                    let coord = |t: &str| {
                        t.parse::<i64>()
                            .ok()
                            .filter(|c| c.abs() <= MAX_CENTRE)
                            .map(|c| c as i32)
                            .ok_or_else(bad)
                    };
                    view.centre = [coord(x)?, coord(z)?];
                }
                "sc" => {
                    view.scale = value
                        .parse::<u32>()
                        .ok()
                        .filter(|s| s.is_power_of_two() && *s <= MAX_SCALE)
                        .ok_or_else(bad)?;
                }
                "y" => view.y = value.parse().map_err(|_| bad())?,
                "st" => view.structures = ids(value).ok_or_else(bad)?,
                "hl" => view.highlight = ids(value).ok_or_else(bad)?,
                "ft" => {
                    view.features = ids(value)
                        .filter(|ids| ids.iter().all(|id| !id.starts_with('#')))
                        .ok_or_else(bad)?;
                }
                "l" => {
                    let mut flags = Flags {
                        ties: false,
                        slime: false,
                        spawn: false,
                        confirm: false,
                        relief: false,
                        surface: false,
                        veins: false,
                        caves: false,
                        carvers: false,
                    };
                    for name in value.split(',') {
                        match name {
                            "ties" => flags.ties = true,
                            "slime" => flags.slime = true,
                            "spawn" => flags.spawn = true,
                            "confirm" => flags.confirm = true,
                            "relief" => flags.relief = true,
                            "surface" => flags.surface = true,
                            "veins" => flags.veins = true,
                            "caves" => flags.caves = true,
                            "carvers" => flags.carvers = true,
                            // Empty (no layers), or a layer from a later viewer.
                            _ => {}
                        }
                    }
                    view.flags = flags;
                }
                // A key from a later viewer.
                _ => {}
            }
        }
        view.seed = seed.ok_or("the view text has no seed (s=)")?;
        Ok(view)
    }
}

/// Ids or `#tags` separated by commas, namespaced: `None` if any isn't an id.
fn ids(value: &str) -> Option<Vec<String>> {
    value
        .split(',')
        .filter(|id| !id.is_empty())
        .map(|id| {
            let (tag, id) = match id.strip_prefix('#') {
                Some(id) => ("#", id),
                None => ("", id),
            };
            let valid = !id.is_empty()
                && id.len() <= 200
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-./:".contains(&b));
            let id = if id.contains(':') {
                id.to_string()
            } else {
                format!("{NAMESPACE}{id}")
            };
            valid.then(|| format!("{tag}{id}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SharedView {
        SharedView {
            seed: -5,
            version: Some("26.3".into()),
            dimension: Dimension::Nether,
            centre: [100, -200],
            scale: 8,
            y: 64,
            structures: vec![
                "minecraft:village_plains".into(),
                "#minecraft:village".into(),
                "pack:ruin".into(),
            ],
            highlight: vec!["minecraft:river".into()],
            features: Vec::new(),
            flags: Flags {
                ties: true,
                ..Flags::default()
            },
        }
    }

    #[test]
    fn a_view_round_trips() {
        let view = sample();
        let text = view.encode();
        assert_eq!(
            text,
            "spyglass:1;s=-5;v=26.3;d=nether;c=100,-200;sc=8;y=64;\
             st=village_plains,#village,pack:ruin;hl=river;l=ties,spawn,confirm"
        );
        assert_eq!(SharedView::decode(&text), Ok(view));
        let bare = SharedView {
            seed: i64::MIN,
            version: None,
            structures: Vec::new(),
            highlight: Vec::new(),
            features: Vec::new(),
            flags: Flags {
                spawn: false,
                confirm: false,
                ..Flags::default()
            },
            ..sample()
        };
        assert_eq!(SharedView::decode(&bare.encode()), Ok(bare));
        let deep = SharedView {
            y: -40,
            flags: Flags {
                veins: true,
                caves: true,
                carvers: true,
                ..Flags::default()
            },
            ..sample()
        };
        let text = deep.encode();
        assert!(text.ends_with(";y=-40;st=village_plains,#village,pack:ruin;hl=river;l=spawn,confirm,veins,caves,carvers"), "{text}");
        assert_eq!(SharedView::decode(&text), Ok(deep));
        let featured = SharedView {
            features: vec![
                "minecraft:monster_room".into(),
                "minecraft:monster_room_deep".into(),
            ],
            ..sample()
        };
        let text = featured.encode();
        assert!(
            text.contains(";hl=river;ft=monster_room,monster_room_deep;l="),
            "{text}"
        );
        assert_eq!(SharedView::decode(&text), Ok(featured));
        assert!(SharedView::decode("spyglass:1;s=1;ft=#minecraft:dungeons").is_err());
    }

    /// Texts each format has written. Never change these: they must open as they did.
    #[test]
    fn texts_of_every_format_still_open() {
        let format_1 = [
            (
                "spyglass:1;s=-5;v=26.3;d=nether;c=100,-200;sc=8;y=64;\
                 st=village_plains,#village;hl=river;l=spawn,confirm",
                sample_1(),
            ),
            (
                "spyglass:1;s=12345",
                SharedView {
                    seed: 12345,
                    version: None,
                    dimension: Dimension::Overworld,
                    centre: [0, 0],
                    scale: 4,
                    y: 64,
                    structures: Vec::new(),
                    highlight: Vec::new(),
                    features: Vec::new(),
                    flags: Flags::default(),
                },
            ),
            (
                "spyglass:1;s=1;v=26.4-snapshot-6;d=end;c=-1000000000,1000000000;sc=256;y=-64;l=",
                SharedView {
                    seed: 1,
                    version: Some("26.4-snapshot-6".into()),
                    dimension: Dimension::End,
                    centre: [-1_000_000_000, 1_000_000_000],
                    scale: 256,
                    y: -64,
                    structures: Vec::new(),
                    highlight: Vec::new(),
                    features: Vec::new(),
                    flags: Flags {
                        spawn: false,
                        confirm: false,
                        ..Flags::default()
                    },
                },
            ),
        ];
        let deep = (
            "spyglass:1;s=7;y=0;l=veins,caves,carvers",
            SharedView {
                seed: 7,
                version: None,
                dimension: Dimension::Overworld,
                centre: [0, 0],
                scale: 4,
                y: 0,
                structures: Vec::new(),
                highlight: Vec::new(),
                features: Vec::new(),
                flags: Flags {
                    spawn: false,
                    confirm: false,
                    veins: true,
                    caves: true,
                    carvers: true,
                    ..Flags::default()
                },
            },
        );
        for (text, view) in format_1.into_iter().chain([deep]) {
            assert_eq!(SharedView::decode(text), Ok(view), "{text}");
        }
    }

    fn sample_1() -> SharedView {
        SharedView {
            structures: vec![
                "minecraft:village_plains".into(),
                "#minecraft:village".into(),
            ],
            flags: Flags::default(),
            ..sample()
        }
    }

    #[test]
    fn texts_are_found_in_pasted_words_and_unknown_keys_ignored() {
        let pasted = "look at this: spyglass:1;s=-5;v=26.3;d=nether;c=100,-200;sc=8;y=64;\
                      st=village_plains,#village;hl=river;l=spawn,confirm,glow;zz=9 neat huh";
        assert_eq!(SharedView::decode(pasted), Ok(sample_1()));
    }

    #[test]
    fn bad_texts_say_why() {
        for (text, says) in [
            ("hello", "no view text"),
            ("spyglass:", "no format"),
            ("spyglass:0;s=1", "format is 0"),
            ("spyglass:2;s=1", "newer spyglass"),
            ("spyglass:1;v=26.3", "no seed"),
            ("spyglass:1;s=1;s=2", "twice"),
            ("spyglass:1;s=1;x", "no ="),
            ("spyglass:1;s=99999999999999999999", "s="),
            ("spyglass:1;s=1;d=moon", "d=moon"),
            ("spyglass:1;s=1;c=1", "c=1"),
            ("spyglass:1;s=1;c=1,2000000000", "c="),
            ("spyglass:1;s=1;sc=3", "sc=3"),
            ("spyglass:1;s=1;sc=512", "sc=512"),
            ("spyglass:1;s=1;st=Village", "st=Village"),
            // The text ends at whitespace, so the seed after it is lost.
            ("spyglass:1;hl=a b;s=1", "no seed"),
        ] {
            let error = SharedView::decode(text).unwrap_err();
            assert!(error.contains(says), "{text}: {error}");
        }
        let long = format!("spyglass:1;s=1;st={}", "a,".repeat(3_000));
        assert!(SharedView::decode(&long).unwrap_err().contains("at most"));
    }
}
