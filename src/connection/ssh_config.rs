use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// One `Host` block parsed from `~/.ssh/config`. Only the directives
/// TermConnect needs to dial a connection are extracted; everything else
/// (`ProxyJump`, `Match`, `Include`, ...) is intentionally ignored for now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigHost {
    pub name: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".ssh").join("config"))
}

/// Reads and parses `~/.ssh/config`. A missing file is not an error — it
/// simply means there is nothing to merge in.
pub fn load() -> Result<Vec<SshConfigHost>> {
    let Some(path) = default_path() else {
        return Ok(Vec::new());
    };
    load_from(&path)
}

fn load_from(path: &Path) -> Result<Vec<SshConfigHost>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    Ok(parse(&contents))
}

/// Parses the contents of an SSH client config file into concrete
/// (non-wildcard) `Host` entries.
pub fn parse(contents: &str) -> Vec<SshConfigHost> {
    let mut hosts: Vec<SshConfigHost> = Vec::new();
    // Index into `hosts` where the current `Host` line's aliases begin — a
    // single `Host a b` line creates one entry per alias, and every
    // directive until the next `Host` line applies to all of them.
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
                set_group(&mut hosts, group_start, |host| {
                    host.host_name = Some(value.to_string())
                });
            }
            "user" => {
                set_group(&mut hosts, group_start, |host| {
                    host.user = Some(value.to_string())
                });
            }
            "port" => {
                if let Ok(port) = value.parse() {
                    set_group(&mut hosts, group_start, |host| host.port = Some(port));
                }
            }
            "identityfile" => {
                let path = expand_home(value);
                set_group(&mut hosts, group_start, |host| {
                    host.identity_file = Some(path.clone())
                });
            }
            _ => {}
        }
    }

    hosts
}

/// Applies `f` to every host entry in the current `Host` group, i.e. those
/// at index `group_start` and after.
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

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_host_block() {
        let config = "\
Host production
    HostName server.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
";
        let hosts = parse(config);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "production");
        assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
        assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[0].port, Some(2222));
    }

    #[test]
    fn skips_wildcard_host_patterns() {
        let config = "\
Host *
    User default_user

Host staging
    HostName staging.example.com
";
        let hosts = parse(config);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "staging");
    }

    #[test]
    fn one_host_line_with_multiple_aliases_shares_settings() {
        let config = "\
Host a b
    HostName shared.example.com
";
        let hosts = parse(config);

        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "a");
        assert_eq!(hosts[1].name, "b");
        assert_eq!(hosts[0].host_name.as_deref(), Some("shared.example.com"));
        assert_eq!(hosts[1].host_name.as_deref(), Some("shared.example.com"));
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let config = "\
# a comment
Host production # trailing comment
    HostName server.example.com

";
        let hosts = parse(config);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "production");
        assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
    }

    #[test]
    fn unknown_directives_are_ignored() {
        let config = "\
Host production
    ProxyJump bastion
    HostName server.example.com
";
        let hosts = parse(config);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
    }
}
