//! A data pack zip on any bytes, read as `spyglass_world::open_pack` reads a save's pack, then
//! applied for a format and its every file looked up through the layer's filter, and layered
//! over a small bundle, through which every registry entry, tag and template it names is
//! resolved. A zip it can't read, or a `pack.mcmeta` the game would skip, is an error, not a
//! panic; panics, aborts and hangs aren't fine.
#![no_main]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use spyglass_data::bundle::{Bundle, FORMAT, MANIFEST, sha1_hex_of};
use spyglass_data::nbt;
use spyglass_data::pack::PackFormat;
use spyglass_world::open_pack;

/// This process's own folder, for the input's zip and the bundle.
fn dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("spyglass-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    })
}

/// A bundle of a few files a pack can replace, add to or hide: a noise, a biome, a tag and a
/// template, recording 26.3's data pack format so packs apply.
fn bundle() -> &'static Bundle {
    static BUNDLE: OnceLock<Bundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let root = dir().join("bundle");
        let files: [(&str, &[u8]); 4] = [
            (
                "data/minecraft/worldgen/noise/a.json",
                br#"{"firstOctave": -7, "amplitudes": [1.0]}"#,
            ),
            ("data/minecraft/worldgen/biome/plains.json", b"{}"),
            (
                "data/minecraft/tags/worldgen/biome/is_hot.json",
                br#"{"values": ["minecraft:plains"]}"#,
            ),
            ("data/minecraft/structure/igloo/top.nbt", b"not gzip"),
        ];
        let mut listed = String::new();
        for (i, (path, bytes)) in files.iter().enumerate() {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, bytes).unwrap();
            let comma = if i == 0 { "" } else { "," };
            write!(listed, r#"{comma}"{path}": "{}""#, sha1_hex_of(bytes)).unwrap();
        }
        let manifest = format!(
            r#"{{"format": {FORMAT}, "version": "26.3", "data_version": 5023, "jar_sha1": "",
              "java_version": "", "extractor": "", "pack_format": [121, 0],
              "files": {{{listed}}}}}"#
        );
        std::fs::write(root.join(MANIFEST), manifest).unwrap();
        Bundle::open(&root).unwrap()
    })
}

/// The registries a pack is looked up through: one of each kind of lookup.
const REGISTRIES: [&str; 4] = [
    "worldgen/noise",
    "worldgen/biome",
    "tags/worldgen/biome",
    "worldgen/structure",
];

fuzz_target!(|data: &[u8]| {
    let path = dir().join("pack.zip");
    std::fs::write(&path, data).unwrap();
    let Ok(pack) = open_pack(&path) else {
        return;
    };
    for format in [[107, 1], [121, 0], [127, 0]] {
        let layer = pack.layer(PackFormat::from(format));
        for file in layer.files.keys() {
            let _ = layer.hides(file);
        }
    }
    let _ = pack.worldgen_files();

    let Ok(layered) = bundle().with_packs([&pack]) else {
        return;
    };
    for registry in REGISTRIES {
        for id in layered.registry_ids(registry) {
            let _ = layered.has_resource(registry, &id);
            let _ = layered.resource(registry, &id);
        }
    }
    let _ = layered.resource("tags/worldgen/biome", "is_hot");
    // Every template the pack holds, read as structures read them: within the template limit.
    for file in pack.files() {
        let Some(rest) = file.strip_prefix("data/") else {
            continue;
        };
        let Some((namespace, template)) = rest.split_once("/structure/") else {
            continue;
        };
        let Some(template) = template.strip_suffix(".nbt") else {
            continue;
        };
        if let Ok(bytes) = layered.structure_template(&format!("{namespace}:{template}")) {
            let _ = nbt::read_template(&bytes);
        }
    }
});
