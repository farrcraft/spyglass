//! A whole bundle's dimensions (ADR 0023): each dimension's [`World`] opened the first time it's
//! asked for, and sessions over every dimension for one seed.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use spyglass_data::bundle::Bundle;

use crate::{Dimension, DimensionType, Error, Result, Session, World, dimension_type};

/// The Default preset's dimensions of one version bundle. Opening it reads only the bundle's
/// manifest and dimension types; each dimension's data is loaded on first use and kept, so
/// every caller shares one load. `Send + Sync`, like [`World`].
pub struct Game {
    bundle: Bundle,
    /// Each dimension's `coordinate_scale`, if the preset has the dimension.
    scales: [Option<f64>; 3],
    worlds: [OnceLock<World>; 3],
    /// Held while a dimension's world loads, so threads asking at once load it once.
    loading: [Mutex<()>; 3],
    /// Why a dimension can't be answered for, when a save's report opened the game.
    blocked: [Option<String>; 3],
}

impl Game {
    /// The bundle in `dir` (e.g. `bundles/26.3`).
    pub fn open(dir: impl Into<PathBuf>) -> Result<Game> {
        Self::open_blocked(dir, [None, None, None])
    }

    /// The bundle in `dir`, refusing each dimension that has a reason in `blocked`.
    pub(crate) fn open_blocked(
        dir: impl Into<PathBuf>,
        blocked: [Option<String>; 3],
    ) -> Result<Game> {
        let bundle = Bundle::open(dir)?;
        let mut scales = [None; 3];
        for d in Dimension::ALL {
            scales[d.index()] = match dimension_type(&bundle, d.id()) {
                Ok(t) => Some(DimensionType::load(&bundle, &t)?.coordinate_scale),
                Err(Error::Unknown { .. }) => None,
                Err(e) => return Err(e),
            };
        }
        Ok(Game {
            bundle,
            scales,
            worlds: Default::default(),
            loading: Default::default(),
            blocked,
        })
    }

    /// The game version, e.g. `26.3`.
    pub fn version(&self) -> &str {
        &self.bundle.manifest().version
    }

    /// `dimension`'s world, opened the first time it's asked for. Fails if the dimension can't
    /// be loaded, or if the save this game was opened for can't be answered for in it.
    pub fn world(&self, dimension: Dimension) -> Result<&World> {
        let i = dimension.index();
        if let Some(reason) = &self.blocked[i] {
            return Err(Error::Unsupported(reason.clone()));
        }
        if let Some(world) = self.worlds[i].get() {
            return Ok(world);
        }
        // Another thread may have loaded it while this one waited. A failed load leaves it
        // unloaded, to try again.
        let _loading = self.loading[i]
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(world) = self.worlds[i].get() {
            return Ok(world);
        }
        let world = World::open_dimension(self.bundle.dir(), dimension)?;
        Ok(self.worlds[i].get_or_init(|| world))
    }

    /// `dimension`'s coordinate scale: 8 for the Nether, 1 for the others.
    pub fn coordinate_scale(&self, dimension: Dimension) -> Result<f64> {
        self.scales[dimension.index()].ok_or_else(|| Error::Unknown {
            what: "dimension",
            id: dimension.id().to_string(),
        })
    }

    /// Block column `(x, z)` in `from` converted to `to`'s coordinates, as the game scales a
    /// teleport (`DimensionType.getTeleportationScale`): each coordinate times `from`'s scale
    /// over `to`'s, in `double`, rounded down, and kept in `i32`. The End isn't linked to the
    /// other dimensions by coordinates, so converting between it and another is an error
    /// (`docs/specs/search.md#dimensions`).
    pub fn convert(&self, [x, z]: [i32; 2], from: Dimension, to: Dimension) -> Result<[i32; 2]> {
        if from == to {
            return Ok([x, z]);
        }
        if from == Dimension::End || to == Dimension::End {
            return Err(Error::NotLinked { from, to });
        }
        let scale = self.coordinate_scale(from)? / self.coordinate_scale(to)?;
        Ok([x, z].map(|v| (f64::from(v) * scale).floor() as i32))
    }

    /// Every dimension for `seed`, each bound the first time it's asked for.
    pub fn session(&self, seed: i64) -> GameSession<'_> {
        GameSession {
            game: self,
            seed,
            sessions: Default::default(),
        }
    }
}

/// A [`Game`] bound to a seed: a [`Session`] per dimension, each bound on first use.
pub struct GameSession<'g> {
    game: &'g Game,
    seed: i64,
    sessions: [OnceLock<Session<'g>>; 3],
}

impl<'g> GameSession<'g> {
    /// The game.
    pub fn game(&self) -> &'g Game {
        self.game
    }

    /// The seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `dimension`'s session, opening its world and binding it the first time it's asked for.
    pub fn dimension(&self, dimension: Dimension) -> Result<&Session<'g>> {
        let i = dimension.index();
        if let Some(session) = self.sessions[i].get() {
            return Ok(session);
        }
        let session = self.game.world(dimension)?.session(self.seed)?;
        Ok(self.sessions[i].get_or_init(|| session))
    }
}
