use porthmos_vfs::{FileKind, Metadata};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) name: String,
    pub(crate) metadata: Metadata,
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year - 719_468
}

fn year_of(unix: u64) -> i64 {
    let days = (unix as i64).div_euclid(SECONDS_PER_DAY);
    let mut year = 1970 + days / 365;
    while days_from_civil(year, 1, 1) > days {
        year -= 1;
    }
    while days_from_civil(year + 1, 1, 1) <= days {
        year += 1;
    }
    year
}

fn seconds(year: i64, month: u32, day: u32, minutes: i64) -> Option<u64> {
    u64::try_from(days_from_civil(year, month, day) * SECONDS_PER_DAY + minutes * 60).ok()
}

fn modified(month: &str, day: &str, time_or_year: &str, now: u64) -> Option<u64> {
    let month = MONTHS.iter().position(|name| *name == month)? as u32 + 1;
    let day: u32 = day.parse().ok().filter(|day| (1..=31).contains(day))?;
    if let Some((hours, minutes)) = time_or_year.split_once(':') {
        let minutes = hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?;
        let this_year = seconds(year_of(now), month, day, minutes)?;
        return if this_year > now + SECONDS_PER_DAY as u64 {
            seconds(year_of(now) - 1, month, day, minutes)
        } else {
            Some(this_year)
        };
    }
    seconds(time_or_year.parse().ok()?, month, day, 0)
}

fn permissions(mode: &str) -> Option<u32> {
    let bits: Vec<char> = mode.chars().skip(1).collect();
    if bits.len() < 9 {
        return None;
    }
    let mut value = 0u32;
    for (index, character) in bits.iter().take(9).enumerate() {
        let bit = 1 << (8 - index);
        let executable_position = index % 3 == 2;
        match character {
            'r' | 'w' | 'x' => value |= bit,
            's' | 't' if executable_position => value |= bit,
            _ => {}
        }
    }
    let special = |index: usize, flag: u32| if matches!(bits[index], 's' | 'S' | 't' | 'T') { flag } else { 0 };
    Some(value | special(2, 0o4000) | special(5, 0o2000) | special(8, 0o1000))
}

fn fields(line: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        match (character == ' ', start) {
            (true, Some(begin)) => {
                found.push((begin, &line[begin..index]));
                start = None;
            }
            (false, None) => start = Some(index),
            _ => {}
        }
    }
    if let Some(begin) = start {
        found.push((begin, &line[begin..]));
    }
    found
}

pub(crate) fn parse_line(line: &str, now: u64) -> Option<Line> {
    let words = fields(line);
    let mode = words.first()?.1;
    let kind_letter = mode.chars().next()?;
    if !"-dlcbps".contains(kind_letter) {
        return None;
    }
    let device = words.get(4)?.1.ends_with(',');
    let date_start = if device { 6 } else { 5 };
    let size = if device { 0 } else { words.get(4)?.1.parse().ok()? };
    let (month, day, time_or_year) =
        (words.get(date_start)?.1, words.get(date_start + 1)?.1, words.get(date_start + 2)?);
    let name_start = time_or_year.0 + time_or_year.1.len() + 1;
    let name = line.get(name_start..).filter(|name| !name.is_empty())?.to_string();
    let kind = if kind_letter == 'd' { FileKind::Dir } else { FileKind::File };
    let modified = modified(month, day, time_or_year.1, now)?;
    let metadata = Metadata { size, modified: Some(modified), kind, permissions: permissions(mode) };
    Some(Line { name, metadata })
}

pub(crate) fn parse(output: &str, now: u64) -> Vec<Line> {
    output
        .lines()
        .filter(|line| !line.starts_with("total "))
        .filter_map(|line| parse_line(line, now))
        .filter(|line| line.name != "." && line.name != "..")
        .collect()
}

#[cfg(test)]
mod tests;
