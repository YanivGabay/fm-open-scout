//! Contracts, wages and the last-transfer record inside a player's block.
//!
//! **Current contract** (verified on FM24 + FM26, see FORMAT.md §4.6):
//!
//! ```text
//! k+0   FF×8
//! k+8   date  contract end            (Haaland 2034-06-30)
//! k+12  date  contract start / joined (Haaland 2022-07-01, Rodri 2019-07-04)
//! k+16  6 bytes
//! k+22  u32   n = number of clause entries
//! k+26  n × 29-byte entries, each starting with 07
//! ...   20 bytes
//! w+0   date (usually null), u8 kind (0/1/2; 2 seen on loans), u32 (unknown),
//! w+9   u32 team id of the contract's club, u32 0,
//! w+17  u32 weekly wage, 01, u8, 00, FF FF FF ..
//! ```
//!
//! The wage pattern `w` is found first: its team id must be the player's team or
//! another team of the same club (youth players are contracted to the club). The
//! contract header is then accepted only at the one offset where the clause count
//! and entry markers make the grammar line up exactly.
//!
//! **Last transfer** (FM24 verified against public fees; FM26 same fields):
//! three dates (null in all observed records; FM26 adds one byte after the first),
//! `u32 0`, `u32` (unknown; -1 in FM26), `i32 fee` (-1 = none), `u32 wage at signing`,
//! `01`. Money is in pounds sterling (Bellingham's €103m fee is stored as 88 318 856).
//! The `u32 0` is a length-prefixed string that is empty in FM24/FM26; FM27 puts
//! two bytes (`FF`, u8) before it and sometimes fills it (Bellingham: "Bellingham").

use crate::bytes::{i32_at, u32_at, FmDate};
use crate::clubs::ClubTable;
use serde::Serialize;

const CLAUSE_LEN: usize = 29;
const BASE_LEN: usize = 20;
const HEADER_LEN: usize = 26;

#[derive(Debug, Clone, Serialize)]
pub struct Contract {
    /// Team id named in the contract (the club's first team for youth players).
    pub team_id: u32,
    /// Weekly wage in GBP.
    pub weekly_wage: u32,
    /// Raw contract kind byte (0/1 regular, 2 observed on loans; meaning inferred).
    pub kind: u8,
    pub starts: Option<String>,
    /// `None` also for non-contract deals, which store 1900-01-02 (amateur players
    /// on no wage; ~9 % of FM27 players).
    pub ends: Option<String>,
    /// Number of clause entries (bonuses, rises, ...); content not decoded.
    pub clauses: Option<u32>,
    /// A loan deal: the player is on loan at this club (wage-record byte 27 is
    /// not FF). His own contract is then `Player::loan_from`.
    pub loan: bool,
}

/// The contract with the parent club of a player on loan. In the same block as the
/// loan deal, with the same grammar, but the wage record's trailer starts `01`
/// (FM26/FM27 `01 00 00 00`, FM24 `01 00 00 03` / `01 01 01 01`) instead of `FF`.
/// Verified on real loans: FM26 Nicolas Jackson at Bayern from Chelsea (£120 000
/// to 2031), El Bilal Touré at Beşiktaş from Atalanta, Boubacar Traoré at Metz from
/// Wolves. Loan deals and parent contracts match one-to-one in number (FM27: 3 825
/// loan deals, 3 855 parent contracts).
#[derive(Debug, Clone, Serialize)]
pub struct ParentContract {
    pub team_id: u32,
    pub club_uid: Option<u32>,
    pub club_name: Option<String>,
    /// Weekly wage in the parent contract, GBP.
    pub weekly_wage: u32,
    pub starts: Option<String>,
    pub ends: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastTransfer {
    /// Fee in GBP; `None` when stored as -1 (academy, free transfer, unknown).
    pub fee: Option<u32>,
    /// Weekly wage agreed at signing, GBP (`None` when stored as -1).
    pub signing_wage: Option<u32>,
}

fn plausible_date(d: &[u8], o: usize) -> bool {
    FmDate::read(d, o).is_some_and(|x| x.is_null() || x.is_plausible())
}

fn wage_pattern_at(d: &[u8], k: usize, belongs: &dyn Fn(u32) -> bool) -> bool {
    (|| {
        if *d.get(k + 4)? > 2 || !plausible_date(d, k) || u32_at(d, k + 13)? != 0 {
            return None;
        }
        if !belongs(u32_at(d, k + 9)?) {
            return None;
        }
        (d.get(k + 21)? == &1 && d.get(k + 23)? == &0 && d.get(k + 24..k + 27)? == [0xff, 0xff, 0xff]).then_some(())
    })()
    .is_some()
}

fn contract_header(d: &[u8], w: usize, lo: usize) -> Option<(usize, u32)> {
    (0..=30u32).find_map(|n| {
        let k = w.checked_sub(BASE_LEN + CLAUSE_LEN * n as usize + HEADER_LEN)?;
        if k < lo || d.get(k..k + 8)? != [0xff; 8] || u32_at(d, k + 22)? != n {
            return None;
        }
        let entries_ok = (0..n as usize).all(|j| d.get(k + HEADER_LEN + CLAUSE_LEN * j) == Some(&7));
        let dates_ok = FmDate::read(d, k + 8)?.is_plausible();
        (entries_ok && dates_ok).then_some((k, n))
    })
}

/// The player's current contract within `[from, to)`.
pub fn find_contract(d: &[u8], from: usize, to: usize, team_id: u32, clubs: Option<&ClubTable>) -> Option<Contract> {
    let owner = |t: u32| clubs.and_then(|c| c.team(t)).map(|(club, _)| club.uid);
    let mine = owner(team_id);
    let belongs = |t: u32| t == team_id || (mine.is_some() && owner(t) == mine);
    let to = to.min(d.len().saturating_sub(30));
    let w = (from..to).find(|&k| wage_pattern_at(d, k, &belongs))?;
    let header = contract_header(d, w, from.saturating_sub(4096));
    let date = |o: usize| FmDate::read(d, o).filter(|x| x.year > 1900).and_then(|x| x.iso());
    Some(Contract {
        team_id: u32_at(d, w + 9)?,
        weekly_wage: u32_at(d, w + 17)?,
        kind: d[w + 4],
        starts: header.and_then(|(k, _)| date(k + 12)),
        ends: header.and_then(|(k, _)| date(k + 8)),
        clauses: header.map(|(_, n)| n),
        loan: d.get(w + 27).is_some_and(|&b| b != 0xff),
    })
}

/// The parent-club contract of a player on loan (see [`ParentContract`]), within
/// `[from, to)`. `today` (ISO) drops contracts that have already ended.
pub fn find_parent_contract(d: &[u8], from: usize, to: usize, team_id: u32, clubs: Option<&ClubTable>, today: Option<&str>) -> Option<ParentContract> {
    let owner = |t: u32| clubs.and_then(|c| c.team(t)).map(|(club, _)| club);
    let mine = owner(team_id).map(|c| c.uid);
    let other_club = |t: u32| match clubs {
        Some(_) => owner(t).is_some_and(|c| Some(c.uid) != mine),
        None => t != team_id,
    };
    let date = |o: usize| FmDate::read(d, o).filter(|x| x.year > 1900).and_then(|x| x.iso());
    let to = to.min(d.len().saturating_sub(30));
    (from..to).find_map(|k| {
        // `01 00 6c 07` is a null date: a different record, not a parent contract.
        if d.get(k + 21)? != &1 || d.get(k + 23)? != &0 || u32_at(d, k + 13)? != 0 || d.get(k + 24)? != &1 || d.get(k + 25..k + 28)? == [0x00, 0x6c, 0x07] {
            return None;
        }
        if !plausible_date(d, k) || !other_club(u32_at(d, k + 9)?) {
            return None;
        }
        let (h, _) = contract_header(d, k, from.saturating_sub(4096))?;
        let (starts, ends) = (date(h + 12)?, date(h + 8)?);
        if today.is_some_and(|t| ends.as_str() < t) {
            return None;
        }
        let team = u32_at(d, k + 9)?;
        Some(ParentContract {
            team_id: team,
            club_uid: owner(team).map(|c| c.uid),
            club_name: owner(team).and_then(|c| c.name.clone()),
            weekly_wage: u32_at(d, k + 17)?,
            starts: Some(starts),
            ends: Some(ends),
        })
    })
}

/// The last-transfer record within `[from, to)`. `extra` is the number of bytes
/// FM inserts after the first date (FM24 0, FM26/FM27 1); `gap` the bytes between
/// the third date and the string (FM24/FM26 0, FM27 2).
pub fn find_last_transfer(d: &[u8], from: usize, to: usize, extra: usize, gap: usize) -> Option<LastTransfer> {
    const NULL: [u8; 4] = [0x01, 0x00, 0x6c, 0x07];
    let mut pat = NULL.to_vec();
    pat.extend(std::iter::repeat(0).take(extra));
    pat.extend(NULL);
    pat.extend(NULL);
    let to = to.min(d.len());
    if from >= to {
        return None;
    }
    let s = from + d[from..to].windows(pat.len()).position(|w| w == pat.as_slice())? + pat.len() + gap;
    let len = u32_at(d, s)? as usize;
    if len > 64 {
        return None;
    }
    let k = s + 4 + len;
    if d.get(k + 12) != Some(&1) {
        return None;
    }
    let fee = i32_at(d, k + 4)?;
    let wage = i32_at(d, k + 8)?;
    Some(LastTransfer { fee: (fee >= 0).then_some(fee as u32), signing_wage: (wage >= 0).then_some(wage as u32) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_bytes(team: u32, wage: u32, clauses: u32) -> Vec<u8> {
        contract_bytes_trailer(team, wage, clauses, [0xff, 0xff, 0xff, 0xff])
    }

    fn contract_bytes_trailer(team: u32, wage: u32, clauses: u32, trailer: [u8; 4]) -> Vec<u8> {
        let mut v = vec![0xffu8; 8];
        v.extend([0xb5, 0x00, 0xf2, 0x07]); // 2034-06-30
        v.extend([0xb6, 0x00, 0xe6, 0x07]); // 2022-07-01
        v.extend([1, 1, 1, 0, 0, 0]);
        v.extend(clauses.to_le_bytes());
        for _ in 0..clauses {
            let mut e = vec![0u8; CLAUSE_LEN];
            e[0] = 7;
            v.extend(e);
        }
        v.extend([0u8; BASE_LEN]);
        v.extend([0x01, 0x00, 0x6c, 0x07, 1]);
        v.extend(0x973au32.to_le_bytes());
        v.extend(team.to_le_bytes());
        v.extend([0u8; 4]);
        v.extend(wage.to_le_bytes());
        v.extend([1, 0x0b, 0]);
        v.extend(trailer);
        v
    }

    #[test]
    fn loan_deal_and_parent_contract() {
        // Kumbulla in an FM27 save: Roma contract (team 905, £55 010 to 2028),
        // then the loan deal with Vallecano (team 1391, £0).
        let mut d = vec![0x33u8; 50];
        d.extend(contract_bytes_trailer(905, 55_010, 0, [0x01, 0x00, 0x00, 0x00]));
        d.extend([0u8; 40]);
        d.extend(contract_bytes_trailer(1391, 0, 1, [0xff, 0xff, 0xff, 0x00]));
        d.extend([0u8; 40]);
        let loan = find_contract(&d, 0, d.len(), 1391, None).unwrap();
        assert!(loan.loan);
        assert_eq!((loan.team_id, loan.weekly_wage), (1391, 0));
        let p = find_parent_contract(&d, 0, d.len(), 1391, None, Some("2026-07-19")).unwrap();
        assert_eq!((p.team_id, p.weekly_wage, p.ends.as_deref()), (905, 55_010, Some("2034-06-30")));
        // Already ended: not a parent contract any more.
        assert!(find_parent_contract(&d, 0, d.len(), 1391, None, Some("2035-01-01")).is_none());
        // A normal contract is not a loan deal.
        let mut normal = vec![0x33u8; 50];
        normal.extend(contract_bytes(540, 1, 0));
        normal.extend([0u8; 40]);
        assert!(!find_contract(&normal, 0, normal.len(), 540, None).unwrap().loan);
        // A record ending in a null date (01 00 6c 07) is something else.
        let mut odd = vec![0x33u8; 50];
        odd.extend(contract_bytes_trailer(905, 55_010, 0, [0x01, 0x00, 0x6c, 0x07]));
        assert!(find_parent_contract(&odd, 0, odd.len(), 1391, None, None).is_none());
    }

    #[test]
    fn finds_contract_and_header() {
        let mut d = vec![0x33u8; 50];
        d.extend(contract_bytes(540, 340_000, 2));
        d.extend([0u8; 40]);
        let c = find_contract(&d, 0, d.len(), 540, None).unwrap();
        assert_eq!((c.team_id, c.weekly_wage, c.kind, c.clauses), (540, 340_000, 1, Some(2)));
        // A non-contract deal ends on 1900-01-02: reported as no end date.
        let mut nc = d.clone();
        nc[50 + 8..50 + 12].copy_from_slice(&[0x02, 0x00, 0x6c, 0x07]);
        assert_eq!(find_contract(&nc, 0, nc.len(), 540, None).unwrap().ends, None);
        assert_eq!(c.ends.as_deref(), Some("2034-06-30"));
        assert_eq!(c.starts.as_deref(), Some("2022-07-01"));
        // Another club's team id does not match.
        assert!(find_contract(&d, 0, d.len(), 541, None).is_none());
    }

    #[test]
    fn finds_last_transfer_both_shapes() {
        for extra in [0usize, 1] {
            let mut d = vec![0x44u8; 20];
            d.extend([0x01, 0x00, 0x6c, 0x07]);
            d.extend(std::iter::repeat(0).take(extra));
            d.extend([0x01, 0x00, 0x6c, 0x07, 0x01, 0x00, 0x6c, 0x07, 0, 0, 0, 0]);
            d.extend(735u32.to_le_bytes());
            d.extend(100_000_000i32.to_le_bytes());
            d.extend(250_000u32.to_le_bytes());
            d.push(1);
            d.extend([0u8; 10]);
            let t = find_last_transfer(&d, 0, d.len(), extra, 0).unwrap();
            assert_eq!((t.fee, t.signing_wage), (Some(100_000_000), Some(250_000)));
        }
    }

    #[test]
    fn finds_fm27_last_transfer_with_string() {
        // Bellingham in a 27.0.0 save: FF 00, "Bellingham", -1, fee, wage, 01.
        let mut d = vec![0x44u8; 9];
        d.extend([0x01, 0x00, 0x6c, 0x07, 0x00, 0x01, 0x00, 0x6c, 0x07, 0x01, 0x00, 0x6c, 0x07, 0xff, 0x00]);
        d.extend(10u32.to_le_bytes());
        d.extend(b"Bellingham");
        d.extend([0xff; 4]);
        d.extend(0x0556_e3c8i32.to_le_bytes());
        d.extend(343_612u32.to_le_bytes());
        d.push(1);
        let t = find_last_transfer(&d, 0, d.len(), 1, 2).unwrap();
        assert_eq!((t.fee, t.signing_wage), (Some(89_580_488), Some(343_612)));
        assert!(find_last_transfer(&d, 0, d.len(), 1, 0).is_none());
    }
}
