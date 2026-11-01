//! Integration tests against real saves. Paths come from `FM24_SAVE` / `FM26_SAVE` /
//! `FM27_SAVE` / `FM27_RESAVED` (and `FM26_SAVE_DIR` for the all-saves test); by default
//! they are read from the git-ignored `saves/` folder at the repo root (`fm24.fm`,
//! `fm26.fm`, `fm27.fm`, `fm27_resaved.fm`, `fm26/`). A missing save skips its test.
//! Use your own saves: copies or symlinks are fine, saves are only read.

use fm_save_parser::container::FmContainer;
use fm_save_parser::players::Player;
use fm_save_parser::{parse_save, version, SaveData};
use std::path::PathBuf;
use std::sync::OnceLock;

const SAVES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../saves");
const FM26_DIR_DEFAULT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../saves/fm26");

fn save_path(var: &str, default: &str) -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var(var).unwrap_or_else(|_| default.to_string()));
    if p.is_file() {
        Some(p)
    } else {
        eprintln!("skipping: {} not found (set {var})", p.display());
        None
    }
}

fn fm24() -> Option<PathBuf> {
    save_path("FM24_SAVE", &format!("{SAVES}/fm24.fm"))
}

fn fm26() -> Option<PathBuf> {
    save_path("FM26_SAVE", &format!("{SAVES}/fm26.fm"))
}

/// Native FM27 career (created with 27.0.0).
fn fm27() -> Option<PathBuf> {
    save_path("FM27_SAVE", &format!("{SAVES}/fm27.fm"))
}

/// FM24 career re-saved by FM27.
fn fm27_resaved() -> Option<PathBuf> {
    save_path("FM27_RESAVED", &format!("{SAVES}/fm27_resaved.fm"))
}

fn parsed(cell: &'static OnceLock<Option<SaveData>>, path: Option<PathBuf>) -> Option<&'static SaveData> {
    cell.get_or_init(|| path.map(|p| parse_save(&p).expect("parse save"))).as_ref()
}

fn fm24_data() -> Option<&'static SaveData> {
    static CELL: OnceLock<Option<SaveData>> = OnceLock::new();
    parsed(&CELL, fm24())
}

fn fm26_data() -> Option<&'static SaveData> {
    static CELL: OnceLock<Option<SaveData>> = OnceLock::new();
    parsed(&CELL, fm26())
}

/// False-positive name blocks (zero-filled ids) show up as one name repeated
/// thousands of times; real duplicates (Vitinho, Paulinho, ...) stay well below 100.
fn assert_no_mass_duplicate_names(s: &SaveData) {
    let mut counts = std::collections::HashMap::new();
    for p in s.players.iter().filter_map(|p| p.name.as_deref()) {
        *counts.entry(p).or_insert(0usize) += 1;
    }
    let (name, n) = counts.into_iter().max_by_key(|(_, n)| *n).unwrap();
    assert!(n < 100, "{name} appears {n} times");
}

fn by_name<'a>(s: &'a SaveData, name: &str) -> &'a Player {
    s.players.iter().find(|p| p.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("{name} not found"))
}

fn club_name(p: &Player) -> Option<&str> {
    p.club.as_ref().and_then(|c| c.name.as_deref())
}

/// Share of players with a club id whose club is resolved, in percent.
fn club_resolution_pct(s: &SaveData) -> f64 {
    let with_id: Vec<_> = s.players.iter().filter(|p| p.club_id.is_some()).collect();
    100.0 * with_id.iter().filter(|p| p.club.is_some()).count() as f64 / with_id.len() as f64
}

fn by_uid(s: &SaveData, uid: u32) -> &Player {
    s.players.iter().find(|p| p.uid == uid).unwrap_or_else(|| panic!("uid {uid} not found"))
}

const HAALAND: u32 = 29_179_063;
const MBAPPE: u32 = 85_138_874;
const RODRI: u32 = 67_217_522;
const FODEN: u32 = 28_108_493;
const VINICIUS: u32 = 19_301_965;

#[test]
fn fm24_container_and_version() {
    let Some(path) = fm24() else { return };
    let c = FmContainer::open(&path).unwrap();
    assert_eq!(c.header.data_start, 26);
    assert_eq!(c.entries.len(), 1334);
    assert_eq!(c.entries[0].file_name(), "game_info.dat");
    // Entries tile the data area exactly, in offset order, with no gaps.
    let mut sorted: Vec<_> = c.entries.iter().collect();
    sorted.sort_by_key(|e| e.offset);
    let mut next = c.header.data_start;
    for e in sorted {
        assert_eq!(e.offset, next, "{}", e.path);
        next += e.compressed_size;
    }
    assert_eq!(next, c.header.directory_offset);

    let v = version::detect(&c).unwrap();
    assert_eq!(v.major, 24);
    assert_eq!(v.game_version.as_deref(), Some("24.3.0+0"));
    assert_eq!(v.db_version, Some(3569));
    assert_eq!(v.db_tag.as_deref(), Some("2430"));
}

#[test]
fn fm24_players() {
    let Some(s) = fm24_data() else { return };
    assert_eq!(s.layout.name, "FM24");
    assert!(s.players.len() > 85_000, "{}", s.players.len());
    let named = s.players.iter().filter(|p| p.name.is_some()).count();
    assert!(named * 100 / s.players.len() >= 98, "named {named}");
    assert_no_mass_duplicate_names(s);

    let h = by_uid(s, HAALAND);
    assert_eq!(h.name.as_deref(), Some("Erling Haaland"));
    assert_eq!(h.full_name.as_deref(), Some("Erling Braut Haaland"));
    assert_eq!((h.current_ability, h.potential_ability), (194, 195));
    assert_eq!(h.date_of_birth.as_deref(), Some("2000-07-21"));
    assert_eq!(h.nation_index, Some(160)); // Norway
    assert_eq!((h.height_cm, h.weight_kg), (Some(195), Some(94)));
    assert_eq!(h.best_position, "ST");
    assert_eq!(h.positions.get("ST"), Some(20));
    assert_eq!(h.preferred_foot, "left");
    assert_eq!(h.attributes.get("left_foot"), Some(20));
    assert_eq!(h.attributes.get("finishing"), Some(18));
    assert_eq!(h.attributes.get("determination"), Some(20));
    assert!(h.attributes.get("handling").unwrap() <= 3);
    assert_eq!(h.joined_club.as_deref(), Some("2022-07-01"));
    assert_eq!(h.club_id, Some(540));
    // Teammates share the club id.
    assert_eq!(by_uid(s, RODRI).club_id, h.club_id);
    assert_eq!(by_uid(s, FODEN).club_id, h.club_id);
    assert_eq!(by_uid(s, MBAPPE).club_id, by_uid(s, VINICIUS).club_id);
    assert_ne!(by_uid(s, MBAPPE).club_id, h.club_id);
}

#[test]
fn fm24_clubs() {
    let Some(s) = fm24_data() else { return };
    let stats = s.clubs.as_ref().expect("club table");
    assert_eq!(stats.clubs, 33_842);
    assert!(club_resolution_pct(s) > 99.5, "{}", club_resolution_pct(s));
    assert_eq!(club_name(by_uid(s, HAALAND)), Some("Manchester City"));
    assert_eq!(club_name(by_uid(s, MBAPPE)), Some("R. Madrid"));
    // The human manager's club in this save.
    assert_eq!(club_name(by_name(s, "Marc Guéhi")), Some("Crystal Palace"));
    assert_eq!(club_name(by_name(s, "Lionel Messi")), Some("Inter Miami CF"));
    assert_eq!(club_name(by_name(s, "Lamine Yamal")), Some("Barcelona"));
    assert_eq!(club_name(by_name(s, "Viktor Gyökeres")), Some("Arsenal"));
    assert_eq!(by_uid(s, HAALAND).club.as_ref().unwrap().team_slot, 0);
    // Youth/reserve slots exist and are younger on average.
    assert!(s.players.iter().any(|p| p.club.as_ref().is_some_and(|c| c.team_slot == 2)));
}

#[test]
fn fm24_nations() {
    let Some(s) = fm24_data() else { return };
    let n = s.nations.as_ref().unwrap();
    assert_eq!(n.nations, 251);
    // Names exist only for nations whose national team appears in a match/scout file.
    assert!(n.named_from_save >= 20, "{}", n.named_from_save);
    let kane = by_uid(s, 28_049_318);
    assert_eq!(kane.nation_uid, Some(765));
    assert_eq!(kane.nation_name.as_deref(), Some("England"));
    assert_eq!(kane.international_team_name.as_deref(), Some("England"));
    let h = by_uid(s, HAALAND);
    assert_eq!(h.nation_name.as_deref(), Some("Norway"));
    // No name is invented: Brazil has no national-team record in this save.
    assert_eq!(by_uid(s, VINICIUS).nation_name, None);
    assert!(by_uid(s, VINICIUS).nation_uid.is_some());
}

#[test]
fn fm24_contracts_and_date() {
    let Some(s) = fm24_data() else { return };
    assert_eq!(s.version.current_date.as_deref(), Some("2025-08-05"));
    assert_eq!(s.version.saved_with.as_deref(), Some("24.4.2"));
    let h = by_uid(s, HAALAND);
    let c = h.contract.as_ref().unwrap();
    assert_eq!((c.team_id, c.weekly_wage), (540, 340_000));
    assert_eq!((c.starts.as_deref(), c.ends.as_deref()), (Some("2022-07-01"), Some("2034-06-30")));
    // Public transfer fees, stored in GBP.
    assert_eq!(h.last_transfer.as_ref().unwrap().fee, Some(51_000_000));
    let rice = by_name(s, "Declan Rice").last_transfer.clone().unwrap();
    assert_eq!((rice.fee, rice.signing_wage), (Some(100_000_000), Some(250_000)));
    assert_eq!(by_name(s, "Jude Bellingham").last_transfer.as_ref().unwrap().fee, Some(88_318_856));
    assert_eq!(by_name(s, "Bukayo Saka").last_transfer.as_ref().unwrap().fee, None); // academy
    assert_eq!(by_uid(s, RODRI).contract.as_ref().unwrap().starts.as_deref(), Some("2019-07-04"));
    assert_eq!(by_name(s, "Lamine Yamal").contract.as_ref().unwrap().ends.as_deref(), Some("2031-06-30"));
    // Contract data exists for nearly all strong players.
    let strong: Vec<_> = s.players.iter().filter(|p| p.current_ability >= 160 && p.club_id.is_some()).collect();
    assert!(strong.iter().filter(|p| p.contract.is_some()).count() * 100 / strong.len() >= 90);
}

#[test]
fn fm24_goalkeeper_attributes() {
    let Some(s) = fm24_data() else { return };
    let courtois = by_uid(s, 18_026_085);
    assert_eq!(courtois.best_position, "GK");
    for a in ["handling", "reflexes", "one_on_ones", "aerial_reach"] {
        assert!(courtois.attributes.get(a).unwrap() >= 14, "{a}");
    }
    assert!(courtois.attributes.get("finishing").unwrap() <= 5);
}

#[test]
fn fm26_container_and_version() {
    let Some(path) = fm26() else { return };
    let c = FmContainer::open(&path).unwrap();
    let v = version::detect(&c).unwrap();
    assert_eq!(v.major, 26);
    assert_eq!(v.game_version.as_deref(), Some("26.0.0+0"));
    assert_eq!(v.db_version, Some(3717));
    assert_eq!(v.db_tag.as_deref(), Some("2600"));
}

#[test]
fn fm26_players() {
    let Some(s) = fm26_data() else { return };
    assert_eq!(s.layout.name, "FM26");
    assert!(s.players.len() > 100_000, "{}", s.players.len());
    let named = s.players.iter().filter(|p| p.name.is_some()).count();
    assert!(named * 100 / s.players.len() >= 99, "named {named}");
    assert_no_mass_duplicate_names(s);

    let m = by_uid(s, MBAPPE);
    assert_eq!(m.name.as_deref(), Some("Kylian Mbappé"));
    assert_eq!((m.current_ability, m.potential_ability), (191, 197));
    assert_eq!(m.date_of_birth.as_deref(), Some("1998-12-20"));
    assert_eq!(m.nation_index, Some(143)); // France
    assert_eq!(m.joined_club.as_deref(), Some("2024-07-01"));
    assert_eq!(m.club_id, by_uid(s, VINICIUS).club_id);

    let h = by_uid(s, HAALAND);
    assert_eq!((h.current_ability, h.potential_ability), (184, 195));
    assert_eq!(h.date_of_birth.as_deref(), Some("2000-07-21"));
    assert_eq!(h.nation_index, Some(160));
    assert_eq!(h.height_cm, Some(195));
    assert_eq!(h.weight_kg, None); // FM26 layout has no weight byte
    assert_eq!(h.preferred_foot, "left");
    assert!(h.personality.is_some());
}

#[test]
fn fm26_clubs() {
    let Some(s) = fm26_data() else { return };
    assert!(club_resolution_pct(s) > 98.5, "{}", club_resolution_pct(s));
    assert_eq!(club_name(by_uid(s, MBAPPE)), Some("R. Madrid"));
    assert_eq!(club_name(by_name(s, "Harry Kane")), Some("FC Bayern München"));
    // Women's clubs are separate club objects with their own UIDs.
    let bonmati = by_name(s, "Aitana Bonmatí").club.as_ref().unwrap();
    let yamal = by_name(s, "Lamine Yamal").club.as_ref().unwrap();
    assert_eq!((bonmati.name.as_deref(), yamal.name.as_deref()), (Some("Barcelona"), Some("Barcelona")));
    assert_ne!(bonmati.uid, yamal.uid);
    assert_eq!(club_name(by_name(s, "Sam Kerr")), Some("Chelsea"));
    assert_eq!(club_name(by_name(s, "Bunny Shaw")), Some("Manchester City"));
    assert_eq!(s.version.current_date.as_deref(), Some("2025-08-31"));
    let hc = by_uid(s, HAALAND).contract.clone().unwrap();
    assert_eq!((hc.weekly_wage, hc.ends.as_deref()), (450_000, Some("2034-06-30")));
    assert_eq!(by_uid(s, MBAPPE).contract.as_ref().unwrap().starts.as_deref(), Some("2024-07-01"));
    assert_eq!(by_name(s, "Declan Rice").last_transfer.as_ref().unwrap().fee, Some(100_000_000));
    // Nation table present (UIDs stable across versions), names absent in this save.
    assert_eq!(s.nations.as_ref().unwrap().nations, 251);
    assert_eq!(by_uid(s, HAALAND).nation_uid, Some(786));
}

#[test]
fn probe_reproduces_verified_layouts() {
    use fm_save_parser::{analyze, Options};
    use fm_save_parser::layout::{FM24, FM26, FM27};
    for (path, expected) in [(fm24(), &FM24), (fm26(), &FM26), (fm27(), &FM27), (fm27_resaved(), &FM27)] {
        let Some(path) = path else { continue };
        let c = FmContainer::open(&path).unwrap();
        let s = analyze(&c, &Options { force_probe: true, ..Default::default() }).unwrap();
        let l = &s.layout;
        assert_eq!(l.header_len, expected.header_len, "{}", path.display());
        assert_eq!(l.club, expected.club);
        assert_eq!(l.nation_after_name, expected.nation_after_name);
        assert_eq!(l.personality_after_name, expected.personality_after_name);
        assert_eq!(l.height_in_tail, expected.height_in_tail);
        assert_eq!(l.weight_in_tail, expected.weight_in_tail);
        assert!(s.warnings.iter().any(|w| w.contains("no verified layout")));
        assert!(s.probe.is_some());
    }
}

#[test]
fn all_fm26_saves_parse() {
    let dir = PathBuf::from(std::env::var("FM26_SAVE_DIR").unwrap_or_else(|_| FM26_DIR_DEFAULT.into()));
    let Ok(rd) = std::fs::read_dir(&dir) else {
        eprintln!("skipping: {} not found", dir.display());
        return;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "fm") {
            let s = parse_save(&p).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
            assert_eq!(s.version.major, 26);
            assert!(s.players.len() > 100_000, "{}: {}", p.display(), s.players.len());
            n += 1;
        }
    }
    eprintln!("parsed {n} FM26 saves");
}

#[test]
fn fm27_native_save() {
    let Some(path) = fm27() else { return };
    let s = parse_save(&path).unwrap();
    assert_eq!((s.version.major, s.version.db_tag.as_deref()), (27, Some("2700")));
    assert_eq!(s.layout.name, "FM27");
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert!(s.version.current_date.is_some());
    assert!(s.players.len() > 80_000);
    assert_no_mass_duplicate_names(&s);
    let bruno = by_name(&s, "Bruno Fernandes");
    assert_eq!((bruno.date_of_birth.as_deref(), bruno.nation_uid), (Some("1994-09-08"), Some(788)));
    assert_eq!(bruno.club.as_ref().map(|c| c.name.as_deref()), Some(Some("Manchester United")));
    // The career goes on, so only check that the contract is a real one (2027 or later).
    let ends = bruno.contract.as_ref().and_then(|c| c.ends.clone()).expect("contract end");
    assert!(ends.as_str() >= "2027-06-30" && ends.as_str() < "2040", "{ends}");
    // Last transfer with FM27's inline string (Bellingham, ~£89.6m from Dortmund).
    let jude = by_name(&s, "Jude Bellingham");
    assert_eq!(jude.last_transfer.as_ref().and_then(|t| t.fee), Some(89_580_488));
    // England internationals share one national team id.
    let (kane, saka) = (by_name(&s, "Harry Kane"), by_name(&s, "Bukayo Saka"));
    assert!(kane.international_team_id.is_some());
    assert_eq!(kane.international_team_id, saka.international_team_id);
}

#[test]
fn fm24_career_resaved_by_fm27() {
    let Some(path) = fm27_resaved() else { return };
    let s = parse_save(&path).unwrap();
    // Version string and DB tag still say FM24; the records are FM27's.
    assert_eq!((s.version.major, s.version.db_tag.as_deref()), (24, Some("2430")));
    assert_eq!(s.version.layout_major(), 27);
    assert_eq!(s.layout.name, "FM27");
    assert!(s.version.current_date.is_some());
    assert!(s.players.len() > 60_000);
    assert_no_mass_duplicate_names(&s);
    let caicedo = by_name(&s, "Moisés Caicedo");
    assert_eq!(caicedo.date_of_birth.as_deref(), Some("2001-11-02"));
    assert_eq!(caicedo.club.as_ref().map(|c| c.name.as_deref()), Some(Some("Chelsea")));
}

/// No name block may claim a nation outside the table or a birth after the in-game
/// date (false blocks used to give ~850 FM27 players a stranger's name).
#[test]
fn names_have_valid_nation_and_birth() {
    let fm27_data = fm27().map(|p| parse_save(&p).unwrap());
    for s in [fm24_data(), fm26_data(), fm27_data.as_ref()].into_iter().flatten() {
        let nations = s.nations.as_ref().map_or(512, |n| n.nations) as u16;
        let bad_nation = s.players.iter().filter(|p| p.nation_index.is_some_and(|n| n >= nations)).count();
        assert_eq!(bad_nation, 0, "{}: nation index outside the table", s.title);
        if let Some(date) = &s.version.current_date {
            let future = s.players.iter().filter(|p| p.date_of_birth.as_ref().is_some_and(|b| b > date)).count();
            assert_eq!(future, 0, "{}: born after {date}", s.title);
        }
    }
}

/// Single-name players (common id = first-name id) are named too: under 0.5 % of
/// players stay unnamed (it was 1-4 % before, growing with newgens).
#[test]
fn nearly_everyone_is_named() {
    let fm27_data = fm27().map(|p| parse_save(&p).unwrap());
    for s in [fm24_data(), fm26_data(), fm27_data.as_ref()].into_iter().flatten() {
        let unnamed = s.players.iter().filter(|p| p.name.is_none()).count();
        assert!(unnamed * 200 < s.players.len(), "{}: {unnamed} of {} unnamed", s.title, s.players.len());
    }
}
