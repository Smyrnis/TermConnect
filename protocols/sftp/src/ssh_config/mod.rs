use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigHost {
    pub name: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

pub fn load(home: Option<&Path>) -> Result<Vec<SshConfigHost>> {
    let Some(home) = home else {
        return Ok(Vec::new());
    };
    load_from(&home.join(".ssh").join("config"), Some(home))
}

fn load_from(path: &Path, home: Option<&Path>) -> Result<Vec<SshConfigHost>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    Ok(parse(&contents, home))
}

pub fn parse(contents: &str, home: Option<&Path>) -> Vec<SshConfigHost> {
    let mut hosts: Vec<SshConfigHost> = Vec::new();
    let mut group_start = 0;

    for line in contents.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }

        let Some((keyword, value)) = split_directive(line) else {
            continue;
        };

        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                group_start = hosts.len();
                for alias in value.split_whitespace() {
                    if is_pattern(alias) {
                        continue;
                    }
                    hosts.push(SshConfigHost {
                        name: alias.to_string(),
                        host_name: None,
                        user: None,
                        port: None,
                        identity_file: None,
                    });
                }
            }
            "hostname" => {
                set_group(&mut hosts, group_start, |host| host.host_name = Some(value.to_string()));
            }
            "user" => {
                set_group(&mut hosts, group_start, |host| host.user = Some(value.to_string()));
            }
            "port" => {
                if let Ok(port) = value.parse() {
                    set_group(&mut hosts, group_start, |host| host.port = Some(port));
                }
            }
            "identityfile" => {
                let path = expand_home(value, home);
                set_group(&mut hosts, group_start, |host| host.identity_file = Some(path.clone()));
            }
            _ => {}
        }
    }

    hosts
}

fn set_group(hosts: &mut [SshConfigHost], group_start: usize, f: impl Fn(&mut SshConfigHost)) {
    for host in hosts.iter_mut().skip(group_start) {
        f(host);
    }
}

fn is_pattern(alias: &str) -> bool {
    alias.contains('*') || alias.contains('?')
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn split_directive(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let split_at = line.find([' ', '\t', '='])?;
    let keyword = &line[..split_at];
    let value = line[split_at..].trim_start_matches([' ', '\t', '=']).trim();
    Some((keyword, value))
}

fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = home
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests;
