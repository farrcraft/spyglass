//! Map calls a caller can stop from another thread (ADR 0078): [`Session::stopping`] gives a
//! view of the session whose map calls check a [`Stop`] as they go, and return
//! [`Error::Stopped`] soon after it's raised. Calls made on the session itself never stop.

use crate::{
    BiomeMap, CarvingMap, Error, FeatureOptions, FeatureQuery, FlowerMap, FoundContainer,
    FoundFeature, HeightMap, ItemQuery, Layers, MapArea, MapFeatures, Result, Session, Stop,
    StructureQuery, StructureStart, UndergroundMap,
};

/// Whether a map call should give up: the stop it was made with, if any. Checked between
/// units of work (a chunk placed or carved, a band of rows), never inside one, so the answers
/// a call gives don't depend on it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Halt<'s>(Option<&'s Stop>);

impl Halt<'_> {
    /// For calls that never stop.
    pub(crate) const NEVER: Halt<'static> = Halt(None);

    /// `Err(Error::Stopped)` once the stop has been raised.
    pub(crate) fn check(self) -> Result<()> {
        match self.0 {
            Some(stop) if stop.is_stopped() => Err(Error::Stopped),
            _ => Ok(()),
        }
    }
}

/// A [`Session`] whose map calls stop when `stop` is raised, made by [`Session::stopping`].
/// Each call gives the same answer as the session's call of the same name, or
/// [`Error::Stopped`].
#[derive(Clone, Copy)]
pub struct Stopping<'s, 'w> {
    session: &'s Session<'w>,
    halt: Halt<'s>,
}

impl<'w> Session<'w> {
    /// This session's map calls, stopped when `stop` is raised from another thread: they then
    /// return [`Error::Stopped`] within a fraction of a second (ADR 0078). For a front end that
    /// computes the map in the background and stops what it no longer needs.
    pub fn stopping<'s>(&'s self, stop: &'s Stop) -> Stopping<'s, 'w> {
        Stopping {
            session: self,
            halt: Halt(Some(stop)),
        }
    }
}

impl<'w> Stopping<'_, 'w> {
    /// The session the calls are made on.
    pub fn session(&self) -> &Session<'w> {
        self.session
    }

    /// [`Session::biome_map`], stopped when the stop is raised.
    pub fn biome_map(&self, area: &MapArea) -> Result<BiomeMap> {
        self.session.biome_map_halting(area, self.halt)
    }

    /// [`Session::height_map`], stopped when the stop is raised.
    pub fn height_map(&self, area: &MapArea) -> Result<HeightMap> {
        self.session.height_map_halting(area, self.halt)
    }

    /// [`Session::surface_biome_map`], stopped when the stop is raised.
    pub fn surface_biome_map(&self, heights: &HeightMap) -> Result<BiomeMap> {
        self.session.surface_biome_map_halting(heights, self.halt)
    }

    /// [`Session::carving_map`], stopped when the stop is raised.
    pub fn carving_map(&self, area: &MapArea) -> Result<CarvingMap> {
        self.session.carving_map_halting(area, self.halt)
    }

    /// [`Session::underground_map`], stopped when the stop is raised.
    pub fn underground_map(
        &self,
        area: &MapArea,
        layers: &Layers,
        carvings: Option<&CarvingMap>,
    ) -> Result<UndergroundMap> {
        self.session
            .underground_map_halting(area, layers, carvings, self.halt)
    }

    /// [`Session::flower_map`], stopped when the stop is raised.
    pub fn flower_map(&self, area: &MapArea) -> Result<FlowerMap> {
        self.session.flower_map_halting(area, self.halt)
    }

    /// [`Session::map_features`], stopped when the stop is raised.
    pub fn map_features(&self, area: &MapArea, layers: &Layers) -> Result<MapFeatures> {
        self.session.map_features_halting(area, layers, self.halt)
    }

    /// [`Session::structures_in`], stopped when the stop is raised.
    pub fn structures_in(
        &self,
        query: &StructureQuery,
        min: [i32; 2],
        max: [i32; 2],
        confirm: bool,
    ) -> Result<Vec<StructureStart>> {
        self.session
            .structures_in_halting(query, min, max, confirm, self.halt)
    }

    /// [`Session::features_in`], stopped when the stop is raised.
    pub fn features_in(
        &self,
        query: &FeatureQuery,
        min: [i32; 2],
        max: [i32; 2],
        options: FeatureOptions,
    ) -> Result<Vec<FoundFeature>> {
        self.session
            .features_in_halting(query, min, max, options, self.halt)
    }

    /// [`Session::containers_in`], stopped when the stop is raised.
    pub fn containers_in(&self, min: [i32; 2], max: [i32; 2]) -> Result<Vec<FoundContainer>> {
        self.session.containers_where(min, max, None, self.halt)
    }

    /// [`Session::containers_holding`], stopped when the stop is raised.
    pub fn containers_holding(
        &self,
        query: &ItemQuery,
        min: [i32; 2],
        max: [i32; 2],
    ) -> Result<Vec<FoundContainer>> {
        self.session
            .containers_holding_halting(query, min, max, self.halt)
    }
}
