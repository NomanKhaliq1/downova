use serde::{Deserialize, Serialize};

/// When a queue may run. `days` bit 0 = Monday ... bit 6 = Sunday. Minutes are local time since midnight.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Schedule {
    pub days: u8,
    pub start: Option<u16>,
    pub stop: Option<u16>,
}

impl Schedule {
    pub const EVERY_DAY: u8 = 0b111_1111;

    /// `day_bit` is `1 << days_from_monday` of today.
    pub fn is_open(&self, day_bit: u8, minute: u16) -> bool {
        if self.days & day_bit == 0 {
            return false;
        }
        match (self.start, self.stop) {
            (None, None) => true,
            (Some(a), None) => minute >= a,
            (None, Some(b)) => minute < b,
            (Some(a), Some(b)) if a <= b => (a..b).contains(&minute),
            (Some(a), Some(b)) => minute >= a || minute < b, // overnight window, e.g. 23:00 - 06:00
        }
    }
}

/// What to do when a queue has finished everything. The app decides how to carry it out.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum FinishAction {
    #[default]
    None,
    Exit,
    Sleep,
    Hibernate,
    Shutdown,
    Command(String),
}

/// Local weekday bit and minute of day; falls back to UTC when the offset is unknown.
pub fn local_now() -> (u8, u16) {
    let n = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    (1 << n.weekday().number_days_from_monday(), n.hour() as u16 * 60 + n.minute() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: u8 = 1;
    const TUE: u8 = 2;

    fn s(days: u8, a: Option<u16>, b: Option<u16>) -> Schedule {
        Schedule { days, start: a, stop: b }
    }

    #[test]
    fn day_filter() {
        assert!(s(MON, None, None).is_open(MON, 0));
        assert!(!s(MON, None, None).is_open(TUE, 0));
    }

    #[test]
    fn daytime_window() {
        let w = s(Schedule::EVERY_DAY, Some(60), Some(120));
        assert!(!w.is_open(MON, 59));
        assert!(w.is_open(MON, 60));
        assert!(w.is_open(MON, 119));
        assert!(!w.is_open(MON, 120));
    }

    #[test]
    fn overnight_window() {
        let w = s(Schedule::EVERY_DAY, Some(23 * 60), Some(6 * 60));
        assert!(w.is_open(MON, 23 * 60 + 30));
        assert!(w.is_open(MON, 3 * 60));
        assert!(!w.is_open(MON, 12 * 60));
    }

    #[test]
    fn open_ended() {
        assert!(s(MON, Some(600), None).is_open(MON, 700));
        assert!(!s(MON, Some(600), None).is_open(MON, 500));
        assert!(s(MON, None, Some(600)).is_open(MON, 500));
    }
}
