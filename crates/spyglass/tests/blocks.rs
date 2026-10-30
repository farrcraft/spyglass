//! `Session::chunk_blocks`, `block_at` and `block_column` against the oracle's block golden files
//! without and with carvers (`docs/specs/material.md`, `docs/specs/carvers.md`): one chunk of each
//! dimension's first file.

use spyglass::{Carvers, Dimension, World};
use spyglass_oracle::blocks::BlockFile;

#[test]
#[ignore = "needs oracle-out/<version>/blocks and bundles/<version>: cargo xtask oracle blocks"]
fn blocks_through_the_api_are_the_game_s() {
    for (carvers, setting) in [(false, Carvers::Off), (true, Carvers::On)] {
        let files = spyglass_oracle::blocks::files(carvers);
        assert!(
            !files.is_empty(),
            "no block files; run cargo xtask oracle blocks"
        );
        for dimension in [Dimension::Overworld, Dimension::Nether, Dimension::End] {
            let Some(path) = files.iter().find(|p| {
                p.parent()
                    .is_some_and(|d| d.ends_with(dimension.id().trim_start_matches("minecraft:")))
            }) else {
                continue;
            };
            let file = BlockFile::read(path).unwrap();
            let world = World::open_dimension(spyglass_oracle::bundle_dir(), dimension).unwrap();
            let session = world.session(file.seed).unwrap();
            let golden = &file.chunks[0];
            let [cx, cz] = golden.chunk;
            let chunk = session.chunk_blocks([cx, cz], setting).unwrap();
            assert_eq!((chunk.chunk(), chunk.min_y()), ([cx, cz], file.min_y));
            let (x, z) = (cx * 16 + 5, cz * 16 + 11);
            let column = session.block_column([x, z], setting).unwrap();
            assert_eq!(column.min_y, file.min_y);
            assert_eq!(column.blocks.len(), file.height as usize);
            for (y, block) in (file.min_y..).zip(&column.blocks) {
                if block.certain {
                    assert_eq!(
                        block.block,
                        golden.block([x, y, z]),
                        "{dimension:?} {setting:?} {x} {y} {z}"
                    );
                }
                assert_eq!(chunk.block([x, y, z]), block.block);
            }
            let y = file.min_y + 70;
            let at = session.block_at([x, y, z], setting).unwrap();
            assert_eq!(at, column.blocks[70]);
        }
    }
}
