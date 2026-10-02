use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: i64 = 86_400;

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or_default()
}

pub(crate) fn skew(local_now: u64, server_date: Option<&str>) -> i64 {
    let Some(server) = server_date
        .and_then(|date| httpdate::parse_http_date(date).ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    else {
        return 0;
    };
    local_now as i64 - server.as_secs() as i64
}

pub(crate) fn adjust(server_time: Option<u64>, skew: i64) -> Option<u64> {
    server_time.map(|time| u64::try_from(time as i64 + skew).unwrap_or(0))
}

pub(crate) fn amz_date(unix: u64) -> (String, String) {
    let seconds = unix as i64;
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    let time = seconds.rem_euclid(SECONDS_PER_DAY);
    let date = format!("{year:04}{month:02}{day:02}");
    let stamp = format!("{date}T{:02}{:02}{:02}Z", time / 3_600, time % 3_600 / 60, time % 60);
    (date, stamp)
}

fn number(text: &str, from: usize, to: usize) -> Option<u32> {
    text.get(from..to)?.parse().ok()
}

pub(crate) fn parse_iso8601(text: &str) -> Option<u64> {
    let text = text.trim();
    let (year, month, day) = (number(text, 0, 4)?, number(text, 5, 7)?, number(text, 8, 10)?);
    let (hour, minute, second) = (number(text, 11, 13)?, number(text, 14, 16)?, number(text, 17, 19)?);
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators.iter().any(|(index, byte)| text.as_bytes().get(*index) != Some(byte)) {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    let seconds = days * SECONDS_PER_DAY + i64::from(hour * 3_600 + minute * 60 + second);
    u64::try_from(seconds).ok()
}

#[cfg(test)]
mod tests;
