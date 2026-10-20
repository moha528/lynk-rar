use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

/// Date locale (ZIP, RAR : horodatage DOS sans fuseau) vers secondes Unix.
pub(crate) fn local_to_unix(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<i64> {
    Local.with_ymd_and_hms(y, mo, d, h, mi, s).earliest().map(|t| t.timestamp())
}

pub(crate) fn from_unix(secs: i64) -> SystemTime {
    if secs >= 0 {
        UNIX_EPOCH + Duration::from_secs(secs as u64)
    } else {
        UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs())
    }
}

pub(crate) struct LocalParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub(crate) fn local_parts(t: SystemTime) -> LocalParts {
    let dt: DateTime<Local> = t.into();
    LocalParts {
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
    }
}

/// Horodatage DOS (date en poids fort, heure en poids faible) vers secondes Unix.
pub(crate) fn dos_to_unix(dos: u32) -> Option<i64> {
    let date = dos >> 16;
    let time = dos & 0xFFFF;
    local_to_unix(
        1980 + (date >> 9) as i32,
        (date >> 5) & 0xF,
        date & 0x1F,
        time >> 11,
        (time >> 5) & 0x3F,
        (time & 0x1F) * 2,
    )
}
