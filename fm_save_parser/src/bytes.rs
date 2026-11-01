//! Little-endian readers over byte slices.
//!
//! Scanners call these on unvalidated offsets, so every reader returns `None`
//! instead of panicking when the read would run past the end of the buffer.

#[inline]
pub fn u8_at(d: &[u8], off: usize) -> Option<u8> {
    d.get(off).copied()
}

#[inline]
pub fn u16_at(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
pub fn i16_at(d: &[u8], off: usize) -> Option<i16> {
    u16_at(d, off).map(|v| v as i16)
}

#[inline]
pub fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn i32_at(d: &[u8], off: usize) -> Option<i32> {
    u32_at(d, off).map(|v| v as i32)
}

#[inline]
pub fn u64_at(d: &[u8], off: usize) -> Option<u64> {
    let b = d.get(off..off.checked_add(8)?)?;
    Some(u64::from_le_bytes(b.try_into().ok()?))
}

#[inline]
pub fn i64_at(d: &[u8], off: usize) -> Option<i64> {
    u64_at(d, off).map(|v| v as i64)
}

/// Sequential reader used for structures that are parsed front to back
/// (the save directory, string tables).
pub struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn u32(&mut self) -> Option<u32> {
        let v = u32_at(self.data, self.pos)?;
        self.pos += 4;
        Some(v)
    }

    pub fn u64(&mut self) -> Option<u64> {
        let v = u64_at(self.data, self.pos)?;
        self.pos += 8;
        Some(v)
    }

    pub fn i64(&mut self) -> Option<i64> {
        let v = i64_at(self.data, self.pos)?;
        self.pos += 8;
        Some(v)
    }

    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let b = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(b)
    }

    /// `u32` byte length followed by UTF-8 text (FM's string encoding).
    pub fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.bytes(n)?;
        Some(String::from_utf8_lossy(b).into_owned())
    }
}

/// A date as FM serialises it: a `u16` whose low 9 bits are the 1-based day of the
/// year (the upper 7 bits carry flags on some dates, e.g. "date joined club"), then
/// a `u16` year. `01 00 6c 07` (day 1 of 1900) is FM's "no date" value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FmDate {
    pub day_of_year: u16,
    pub year: u16,
    /// Upper 7 bits of the day field (meaning unknown).
    pub flags: u8,
}

impl FmDate {
    pub const NULL: FmDate = FmDate { day_of_year: 1, year: 1900, flags: 0 };

    pub fn read(d: &[u8], off: usize) -> Option<FmDate> {
        let raw = u16_at(d, off)?;
        Some(FmDate { day_of_year: raw & 0x1FF, year: u16_at(d, off + 2)?, flags: (raw >> 9) as u8 })
    }

    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }

    pub fn is_plausible(&self) -> bool {
        (1..=366).contains(&self.day_of_year) && (1900..=2200).contains(&self.year)
    }

    fn is_leap(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// Calendar date `(year, month, day)`, or `None` for the null date or an
    /// out-of-range day number.
    pub fn ymd(&self) -> Option<(u16, u8, u8)> {
        if self.is_null() || !self.is_plausible() {
            return None;
        }
        let mut days = [31u16, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        if Self::is_leap(self.year) {
            days[1] = 29;
        }
        let mut left = self.day_of_year;
        for (m, &len) in days.iter().enumerate() {
            if left <= len {
                return Some((self.year, m as u8 + 1, left as u8));
            }
            left -= len;
        }
        None
    }

    /// ISO-8601 `YYYY-MM-DD`, or `None` when [`FmDate::ymd`] is `None`.
    pub fn iso(&self) -> Option<String> {
        self.ymd().map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_conversion() {
        // Erling Haaland's date of birth as stored in the FM24 save: day 203 of 2000.
        let d = FmDate { day_of_year: 203, year: 2000, flags: 0 };
        assert_eq!(d.iso().as_deref(), Some("2000-07-21"));
        // 1 July in a leap year is day 183.
        assert_eq!(FmDate { day_of_year: 183, year: 2024, flags: 0 }.iso().as_deref(), Some("2024-07-01"));
        assert_eq!(FmDate::NULL.iso(), None);
        assert_eq!(FmDate { day_of_year: 366, year: 2023, flags: 0 }.iso(), None);
        // Flag bits above the 9-bit day are ignored: 0x352d -> day 301.
        let d = FmDate::read(&[0x2d, 0x35, 0xe8, 0x07], 0).unwrap();
        assert_eq!((d.day_of_year, d.flags, d.iso().as_deref()), (301, 26, Some("2024-10-27")));
    }

    #[test]
    fn readers_are_bounds_checked() {
        let d = [1u8, 2, 3];
        assert_eq!(u16_at(&d, 1), Some(0x0302));
        assert_eq!(u32_at(&d, 0), None);
        assert_eq!(u16_at(&d, usize::MAX), None);
    }
}
