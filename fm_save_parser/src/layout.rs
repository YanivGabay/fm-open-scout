//! Per-version record layouts and the dispatch table.
//!
//! All player offsets are relative to `r`, the position of the player's Current
//! Ability inside the decompressed `game_db.dat`. A player block looks like:
//!
//! ```text
//! r-19  u32   row index (person table position; not used)
//! r-15  u32   person UID           } stored twice; the repeat is the scan anchor
//! r-11  u32   person UID           }
//! r-7   u8    (02 in every observed record)
//! r-6   u16×3 reputations (0..10000): home, current, world (world verified, see FORMAT.md)
//! r+0   u16   Current Ability (1..200)
//! r+2   i16   Potential Ability (1..200; negative = FM's "range" PA)
//! r+4   ...   header: `header_len` bytes (FM24 19, FM26 20), contains
//!             international team id and club id (u16 each, 0xFFFF = none)
//! p     u8×15 position ratings 1..20       p = 4 + header_len
//! a     u8×54 attributes, 1..100 scale     a = p + 15 (display value = raw / 5)
//! a+54  u32   money amount, meaning unverified (capped at 300 000 000), u8 unknown
//! a+59  date  date joined current club
//! a+63  date  second date (null for ~99% of players)
//! a+67  ...   `tail`: CA/PA byte copies at +12/+13, weight and height
//! ```
//!
//! Further on (a few hundred to ~2000 bytes later) comes the person name block,
//! see `players.rs`. Offsets inside the name block are relative to `after`, the byte
//! right after the inline full-name string, where the date of birth starts.
//!
//! Adding a version = adding one `PlayerLayout` constant and one `layout_for` arm.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Confidence {
    /// Checked field-by-field against known players in a local save of this version.
    Verified,
    /// Derived from evidence (other versions, tools) but not checked against a save.
    Inferred,
    /// Core offsets found by `probe_layout` at runtime; optional fields unknown.
    Probed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerLayout {
    pub name: &'static str,
    pub confidence: Confidence,
    /// Bytes between PA and the first position rating.
    pub header_len: usize,
    /// Offset (from r) of the u16 international team id, if known.
    pub intl_team: Option<usize>,
    /// Offset (from r) of the u16 club id, if known.
    pub club: Option<usize>,
    /// Offset from the tail start (a+67) of the weight byte in kg, if stored.
    pub weight_in_tail: Option<usize>,
    /// Offset from the tail start of the height byte in cm.
    pub height_in_tail: Option<usize>,
    /// Offset from `after` (name block) of the u16 nationality index.
    pub nation_after_name: Option<usize>,
    /// Offset from `after` of the 8 personality bytes (1..20 scale, not ×5).
    pub personality_after_name: Option<usize>,
    /// Shape of the dated entries in club objects (see `clubs.rs`); `None` = try all.
    pub club_entry: Option<ClubEntryShape>,
    /// Bytes inserted after the first date of the last-transfer record
    /// (`contracts.rs`); `None` = try 0 and 1.
    pub transfer_extra: Option<usize>,
    /// Bytes between the last-transfer record's third date and its string
    /// (FM27 2); `None` = try 0 and 2.
    pub transfer_gap: Option<usize>,
}

/// Club objects contain a counted run of fixed-size dated entries before their
/// reference block; the entry's first byte is a per-version marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClubEntryShape {
    pub marker: u8,
    pub len: usize,
}

pub const CLUB_ENTRY_FM24: ClubEntryShape = ClubEntryShape { marker: 1, len: 24 };
pub const CLUB_ENTRY_FM26: ClubEntryShape = ClubEntryShape { marker: 2, len: 25 };

pub const UID_OFFSET: isize = -15;
pub const UID_REPEAT_OFFSET: isize = -11;
pub const REPUTATION_OFFSET: isize = -6;
pub const POSITION_COUNT: usize = 15;
pub const ATTRIBUTE_COUNT: usize = 54;
pub const PERSONALITY_COUNT: usize = 8;

impl PlayerLayout {
    pub fn positions(&self) -> usize {
        4 + self.header_len
    }
    pub fn attributes(&self) -> usize {
        self.positions() + POSITION_COUNT
    }
    /// u32 right after the attributes: a money amount (see `Player::valuation_unverified`).
    pub fn valuation(&self) -> usize {
        self.attributes() + ATTRIBUTE_COUNT
    }
    pub fn joined_date(&self) -> usize {
        self.attributes() + ATTRIBUTE_COUNT + 5
    }
    pub fn second_date(&self) -> usize {
        self.joined_date() + 4
    }
    pub fn tail(&self) -> usize {
        self.second_date() + 4
    }
}

/// FM24 (verified: 24.3.0 save; DB tag "2430", schema 3569).
pub const FM24: PlayerLayout = PlayerLayout {
    name: "FM24",
    confidence: Confidence::Verified,
    header_len: 19,
    intl_team: Some(11),
    club: Some(15),
    weight_in_tail: Some(15),
    height_in_tail: Some(17),
    nation_after_name: Some(9),
    personality_after_name: Some(17),
    club_entry: Some(CLUB_ENTRY_FM24),
    transfer_extra: Some(0),
    transfer_gap: Some(0),
};

/// FM26 (verified: 26.0.0 saves; DB tag "2600", schema 3717). One byte was inserted
/// in the ability header (at r+9) and four in the name block (at after+9); the
/// weight byte is gone from the tail and height moved two bytes earlier.
pub const FM26: PlayerLayout = PlayerLayout {
    name: "FM26",
    confidence: Confidence::Verified,
    header_len: 20,
    intl_team: Some(12),
    club: Some(16),
    weight_in_tail: None,
    height_in_tail: Some(15),
    nation_after_name: Some(13),
    personality_after_name: Some(21),
    club_entry: Some(CLUB_ENTRY_FM26),
    transfer_extra: Some(1),
    transfer_gap: Some(0),
};

/// FM27 (verified: 27.0.0 saves, native and an FM24 career re-saved by FM27).
/// Same ability block as FM26; one more byte in the name block before the
/// nationality, and two in the last-transfer record.
pub const FM27: PlayerLayout = PlayerLayout {
    name: "FM27",
    confidence: Confidence::Verified,
    header_len: 20,
    intl_team: Some(12),
    club: Some(16),
    weight_in_tail: None,
    height_in_tail: Some(15),
    nation_after_name: Some(14),
    personality_after_name: Some(22),
    club_entry: Some(CLUB_ENTRY_FM26),
    transfer_extra: Some(1),
    transfer_gap: Some(2),
};

/// Layout for a detected major version. `None` means "no evidence-backed layout":
/// callers should fall back to [`crate::players::probe_layout`].
///
/// FM25 was never released (cancelled in 2025), so 24 → 26 is a direct step.
/// FM20–FM23: no local saves and no public evidence of their record layouts, so
/// they are deliberately left to the probe.
pub fn layout_for(major: u16) -> Option<&'static PlayerLayout> {
    match major {
        24 => Some(&FM24),
        26 => Some(&FM26),
        27 => Some(&FM27),
        _ => None,
    }
}

/// Core-only layout used by the probe for unknown versions.
pub fn probed(header_len: usize) -> PlayerLayout {
    PlayerLayout {
        name: "probed",
        confidence: Confidence::Probed,
        header_len,
        intl_team: None,
        club: None,
        weight_in_tail: None,
        height_in_tail: None,
        nation_after_name: None,
        personality_after_name: None,
        club_entry: None,
        transfer_extra: None,
        transfer_gap: None,
    }
}

/// Position ratings in storage order.
pub const POSITION_NAMES: [&str; POSITION_COUNT] = [
    "GK", "SW", "DL", "DC", "DR", "DM", "ML", "MC", "MR", "AML", "AMC", "AMR", "ST", "WBL", "WBR",
];

/// The 54 attributes in storage order (FM24, FM26 and FM27 share it). Established
/// from the saves: keeper-only slots, slots that peak in one position group, and
/// age curves (`scripts/verify_fields.js`, FORMAT.md §4.3); slots 14/33
/// (communication vs. tendency to punch) by correlation over 1 445 FM24 / 2 328
/// FM26 keepers. Look-alike pairs (pace/acceleration, ...) await an in-game check.
pub const ATTRIBUTE_NAMES: [&str; ATTRIBUTE_COUNT] = [
    "crossing", "dribbling", "finishing", "heading", "long_shots", "marking",
    "off_the_ball", "passing", "penalty_taking", "tackling", "vision", "handling",
    "aerial_reach", "command_of_area", "communication", "kicking", "throwing", "anticipation",
    "decisions", "one_on_ones", "positioning", "reflexes", "first_touch", "technique",
    "left_foot", "right_foot", "flair", "corners", "teamwork", "work_rate",
    "long_throws", "eccentricity", "rushing_out", "tendency_to_punch", "acceleration", "free_kicks",
    "strength", "stamina", "pace", "jumping_reach", "leadership", "dirtiness",
    "balance", "bravery", "consistency", "aggression", "agility", "important_matches",
    "injury_proneness", "versatility", "natural_fitness", "determination", "composure", "concentration",
];

/// Personality (hidden) attributes in storage order. Inferred: correlations with
/// attributes fit this order (professionalism ~ determination, sportsmanship ~
/// -dirtiness, FORMAT.md §4.5), but the values are hidden in the game UI.
pub const PERSONALITY_NAMES: [&str; PERSONALITY_COUNT] = [
    "adaptability", "ambition", "loyalty", "pressure", "professionalism", "sportsmanship",
    "temperament", "controversy",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_offsets_match_observed_saves() {
        // FM24: positions at r+23, attributes at r+38, null date at r+101.
        assert_eq!((FM24.positions(), FM24.attributes(), FM24.second_date()), (23, 38, 101));
        // FM26: everything after the header shifted by one byte.
        assert_eq!((FM26.positions(), FM26.attributes(), FM26.second_date()), (24, 39, 102));
        assert_eq!(FM24.joined_date(), 97);
    }

    #[test]
    fn dispatch() {
        assert_eq!(layout_for(24).unwrap().name, "FM24");
        assert_eq!(layout_for(26).unwrap().name, "FM26");
        assert!(layout_for(23).is_none());
        assert!(layout_for(25).is_none());
        assert_eq!(layout_for(27).unwrap().name, "FM27");
    }
}
