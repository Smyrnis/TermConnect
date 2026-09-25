use porthmos_vfs::{FileKind, Metadata};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListedItem {
    pub(crate) name: String,
    pub(crate) metadata: Metadata,
    pub(crate) link_target: Option<String>,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const UNIX_LEADING_FIELDS: usize = 8;

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn unix_seconds(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Option<u64> {
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second);
    if !valid {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds).ok()
}

fn civil_year_from_days(days: i64) -> i64 {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    year_of_era + era * 400 + i64::from(month_index >= 10)
}

pub(crate) fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0)
}

fn year_of(seconds: u64) -> i64 {
    civil_year_from_days((seconds / 86_400) as i64)
}

const DAY: u64 = 86_400;

fn digits(text: &str, range: std::ops::Range<usize>) -> Option<i64> {
    text.get(range)?.parse().ok()
}

fn parse_modify(value: &str) -> Option<u64> {
    let stamp = value.split('.').next()?;
    if stamp.len() != 14 {
        return None;
    }
    unix_seconds(
        digits(stamp, 0..4)?,
        digits(stamp, 4..6)?,
        digits(stamp, 6..8)?,
        digits(stamp, 8..10)?,
        digits(stamp, 10..12)?,
        digits(stamp, 12..14)?,
    )
}

fn metadata_from_facts(facts: &str, skip_directory_markers: bool) -> Option<Metadata> {
    let mut metadata = Metadata { size: 0, modified: None, kind: FileKind::File, permissions: None };
    for fact in facts.split(';').filter(|fact| !fact.is_empty()) {
        let (key, value) = fact.split_once('=')?;
        match key.trim().to_ascii_lowercase().as_str() {
            "type" => {
                let value = value.to_ascii_lowercase();
                metadata.kind = match value.as_str() {
                    "cdir" | "pdir" if skip_directory_markers => return None,
                    "cdir" | "pdir" => FileKind::Dir,
                    "dir" => FileKind::Dir,
                    "file" => FileKind::File,
                    other if other.contains("symlink") || other.contains("slink") => FileKind::Symlink,
                    _ => FileKind::File,
                };
            }
            "size" | "sizd" => metadata.size = value.parse().unwrap_or(0),
            "modify" => metadata.modified = parse_modify(value),
            "unix.mode" => metadata.permissions = u32::from_str_radix(value.trim_start_matches('0'), 8).ok(),
            _ => {}
        }
    }
    Some(metadata)
}

pub(crate) fn parse_mlsd_line(line: &str) -> Option<ListedItem> {
    let (facts, name) = line.split_once(' ')?;
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let metadata = metadata_from_facts(facts, true)?;
    Some(ListedItem { name: name.to_string(), metadata, link_target: None })
}

pub(crate) fn parse_mlst_facts(line: &str) -> Option<Metadata> {
    let line = line.trim_start();
    let facts = line.split_once(' ').map_or(line, |(facts, _)| facts);
    metadata_from_facts(facts, false)
}

fn permissions_from_mode(mode: &str) -> Option<u32> {
    let bits = mode.get(1..10)?;
    Some(bits.chars().fold(0u32, |acc, bit| (acc << 1) | u32::from(bit != '-')))
}

fn month_number(name: &str) -> Option<i64> {
    let lower = name.to_ascii_lowercase();
    MONTHS.iter().position(|month| *month == lower).map(|index| index as i64 + 1)
}

fn token_end(line: &str, count: usize) -> Option<usize> {
    let mut seen = 0;
    let mut in_token = false;
    for (index, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if in_token {
                seen += 1;
                if seen == count {
                    return Some(index);
                }
            }
            in_token = false;
        } else {
            in_token = true;
        }
    }
    None
}

fn parse_unix(line: &str, now: u64) -> Option<ListedItem> {
    let fields: Vec<&str> = line.split_whitespace().take(UNIX_LEADING_FIELDS).collect();
    let mode = *fields.first()?;
    let kind = match mode.chars().next()? {
        'd' => FileKind::Dir,
        'l' => FileKind::Symlink,
        '-' => FileKind::File,
        _ => return None,
    };
    let month_at = (3..fields.len().saturating_sub(2))
        .find(|&index| month_number(fields[index]).is_some() && fields[index - 1].parse::<u64>().is_ok())?;
    let (size, month, day, time_or_year) =
        (fields[month_at - 1], fields[month_at], fields[month_at + 1], fields[month_at + 2]);
    let name_start = token_end(line, month_at + 3)? + 1;
    let raw_name = line.get(name_start..)?;
    let (name, link_target) = match (kind, raw_name.split_once(" -> ")) {
        (FileKind::Symlink, Some((name, target))) => (name, Some(target.to_string())),
        _ => (raw_name, None),
    };
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let month = month_number(month)?;
    let day: i64 = day.parse().ok()?;
    let modified = match time_or_year.split_once(':') {
        Some((hour, minute)) => {
            let (hour, minute) = (hour.parse().ok()?, minute.parse().ok()?);
            let this_year = unix_seconds(year_of(now), month, day, hour, minute, 0);
            match this_year {
                Some(stamp) if stamp > now + DAY => unix_seconds(year_of(now) - 1, month, day, hour, minute, 0),
                other => other,
            }
        }
        None => unix_seconds(time_or_year.parse().ok()?, month, day, 0, 0, 0),
    };
    Some(ListedItem {
        name: name.to_string(),
        metadata: Metadata {
            size: size.parse().unwrap_or(0),
            modified,
            kind,
            permissions: permissions_from_mode(mode),
        },
        link_target,
    })
}

fn parse_windows(line: &str) -> Option<ListedItem> {
    let fields: Vec<&str> = line.split_whitespace().take(3).collect();
    let [date, time, size_or_dir] = fields.as_slice() else {
        return None;
    };
    let mut date_parts = date.split('-');
    let (month, day, year) = (date_parts.next()?, date_parts.next()?, date_parts.next()?);
    let year: i64 = year.parse().ok()?;
    let year = if year < 100 { if year < 70 { 2000 + year } else { 1900 + year } } else { year };
    let upper = time.to_ascii_uppercase();
    let (clock, meridiem) = match upper.strip_suffix("AM").or_else(|| upper.strip_suffix("PM")) {
        Some(clock) => (clock.to_string(), &upper[clock.len()..]),
        None => (upper.clone(), ""),
    };
    let (hour, minute) = clock.split_once(':')?;
    let hour: i64 = hour.parse().ok()?;
    let hour = match (meridiem, hour) {
        ("AM", 12) => 0,
        ("PM", 12) => 12,
        ("PM", hour) => hour + 12,
        (_, hour) => hour,
    };
    let modified = unix_seconds(year, month.parse().ok()?, day.parse().ok()?, hour, minute.parse().ok()?, 0);
    let name = line.get(token_end(line, 3)?..)?.trim_start();
    let (kind, size) =
        if *size_or_dir == "<DIR>" { (FileKind::Dir, 0) } else { (FileKind::File, size_or_dir.parse().ok()?) };
    if name.is_empty() {
        return None;
    }
    Some(ListedItem {
        name: name.to_string(),
        metadata: Metadata { size, modified, kind, permissions: None },
        link_target: None,
    })
}

fn is_windows_date(token: &str) -> bool {
    let parts: Vec<&str> = token.split('-').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn parse_list_line(line: &str, now: u64) -> Option<ListedItem> {
    let first = line.split_whitespace().next()?;
    if is_windows_date(first) { parse_windows(line) } else { parse_unix(line, now) }
}

#[cfg(test)]
mod tests;
