//! Football Manager save (`.fm`) parser.
//!
//! Pipeline: [`container`] (zstd virtual filesystem) → [`version`] (detect FM
//! release) → [`layout`] (per-version record offsets, dispatched on the major
//! version) → [`names`] + [`players`] (scan `game_db.dat`). See FORMAT.md.

pub mod bytes;
pub mod clubs;
pub mod container;
pub mod contracts;
pub mod error;
pub mod layout;
pub mod names;
pub mod nations;
pub mod players;
pub mod probe;
pub mod version;

use container::FmContainer;
use error::{FmError, Result};
use layout::PlayerLayout;
use names::{NameTableStats, NameTables};
use players::Player;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct SaveData {
    pub title: String,
    pub version: version::SaveVersion,
    pub layout: PlayerLayout,
    /// Present when the layout was probed (no registered layout for this version).
    pub probe: Option<probe::ProbeReport>,
    /// Human-readable warnings, e.g. for unverified versions.
    pub warnings: Vec<String>,
    pub names: Option<NameTableStats>,
    pub clubs: Option<clubs::ClubStats>,
    pub nations: Option<nations::NationStats>,
    pub players: Vec<Player>,
}

pub const UNVERIFIED_WARNING: &str = "no verified layout for this version: offsets were probed statistically \
     (see `probe` in the output); values may be wrong. FORMAT.md §9 lists the saves that would verify it.";

#[derive(Debug, Default, Clone)]
pub struct Options {
    /// `uid,name` text with nation names.
    pub nation_names: Option<String>,
    /// Ignore the registered layout and probe (to compare the probe with a known version).
    pub force_probe: bool,
}

/// Picks the layout for a version: the registered one, or a statistical probe.
pub fn resolve_layout(
    v: &version::SaveVersion,
    game_db: &[u8],
    names: Option<&NameTables>,
    clubs: Option<&clubs::ClubTable>,
    force_probe: bool,
) -> Result<(PlayerLayout, Option<probe::ProbeReport>)> {
    if let Some(l) = layout::layout_for(v.layout_major()).filter(|_| !force_probe) {
        return Ok((l.clone(), None));
    }
    probe::probe(game_db, names, clubs).map(|(l, r)| (l, Some(r))).ok_or_else(|| {
        FmError::UnsupportedVersion(format!("FM{} (no registered layout and probing found no player records)", v.major))
    })
}

/// Full analysis of an opened save.
pub fn analyze(c: &FmContainer, opts: &Options) -> Result<SaveData> {
    let version = version::detect(c)?;
    let db = c.read("game_db.dat")?;
    let names = names::locate(&db);
    let shape = layout::layout_for(version.layout_major()).filter(|_| !opts.force_probe).and_then(|l| l.club_entry);
    let club_table = clubs::locate(&db, shape);
    let (layout, probe) = resolve_layout(&version, &db, names.as_ref(), club_table.as_ref(), opts.force_probe)?;
    let mut nations = nations::Nations::load(c, &db);
    if let Some(text) = &opts.nation_names {
        nations.add_user_names(text);
    }
    let limits = players::NameLimits {
        nations: if nations.uids.is_empty() { players::NameLimits::default().nations } else { nations.uids.len() as u16 },
        latest_birth: version.current_date.clone(),
    };
    let mut players = players::extract_players(&db, &layout, names.as_ref(), club_table.as_ref(), &limits);
    players::attach_nations(&mut players, &nations);
    let mut warnings = Vec::new();
    if layout.confidence != layout::Confidence::Verified {
        warnings.push(format!("FM{}: {UNVERIFIED_WARNING}", version.major));
    }
    if club_table.is_none() {
        warnings.push("club table not found: club names unavailable".into());
    }
    Ok(SaveData {
        title: c.title.clone(),
        version,
        layout,
        probe,
        warnings,
        names: names.as_ref().map(NameTables::stats),
        clubs: club_table.as_ref().map(clubs::ClubTable::stats),
        nations: Some(nations.stats()),
        players,
    })
}

/// Opens a save, detects its version and extracts all players.
pub fn parse_save(path: &Path) -> Result<SaveData> {
    analyze(&FmContainer::open(path)?, &Options::default())
}
