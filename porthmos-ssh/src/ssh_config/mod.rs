use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use porthmos_vfs::glob_match;

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

enum Applies {
    Everywhere,
    ToHosts(Vec<String>),
    Nowhere,
}

struct Block {
    applies: Applies,
    directives: Vec<(String, String)>,
}

impl Block {
    fn matches(&self, alias: &str) -> bool {
        match &self.applies {
            Applies::Everywhere => true,
            Applies::Nowhere => false,
            Applies::ToHosts(patterns) => {
                let mut matched = false;
                for pattern in patterns {
                    match pattern.strip_prefix('!') {
                        Some(negated) if glob_match(negated, alias) => return false,
                        Some(_) => {}
                        None => matched |= glob_match(pattern, alias),
                    }
                }
                matched
            }
        }
    }

    fn names(&self, alias: &str) -> bool {
        match &self.applies {
            Applies::ToHosts(patterns) => {
                patterns.iter().any(|pattern| !is_pattern(pattern) && pattern.eq_ignore_ascii_case(alias))
            }
            Applies::Everywhere | Applies::Nowhere => false,
        }
    }
}

pub fn parse(contents: &str, home: Option<&Path>) -> Vec<SshConfigHost> {
    let blocks = split_into_blocks(contents);
    concrete_aliases(&blocks).into_iter().map(|alias| resolve(&alias, &blocks, home)).collect()
}

fn split_into_blocks(contents: &str) -> Vec<Block> {
    let mut blocks = vec![Block { applies: Applies::Everywhere, directives: Vec::new() }];

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
                let patterns = value.split_whitespace().map(str::to_string).collect();
                blocks.push(Block { applies: Applies::ToHosts(patterns), directives: Vec::new() });
            }
            "match" => blocks.push(Block { applies: Applies::Nowhere, directives: Vec::new() }),
            keyword => {
                if let Some(block) = blocks.last_mut() {
                    block.directives.push((keyword.to_string(), value.to_string()));
                }
            }
        }
    }

    blocks
}

fn concrete_aliases(blocks: &[Block]) -> Vec<String> {
    let mut aliases: Vec<String> = Vec::new();
    for block in blocks {
        let Applies::ToHosts(patterns) = &block.applies else {
            continue;
        };
        for pattern in patterns {
            if is_pattern(pattern) || aliases.iter().any(|alias| alias.eq_ignore_ascii_case(pattern)) {
                continue;
            }
            aliases.push(pattern.clone());
        }
    }
    aliases
}

fn resolve(alias: &str, blocks: &[Block], home: Option<&Path>) -> SshConfigHost {
    let mut host =
        SshConfigHost { name: alias.to_string(), host_name: None, user: None, port: None, identity_file: None };

    let mut wildcard_identity_file = None;
    for block in blocks.iter().filter(|block| block.matches(alias)) {
        let names_alias = block.names(alias);
        for (keyword, value) in &block.directives {
            match keyword.as_str() {
                "hostname" if host.host_name.is_none() => host.host_name = Some(expand_tokens(value, alias)),
                "user" if host.user.is_none() => host.user = Some(value.clone()),
                "port" if host.port.is_none() => host.port = value.parse().ok(),
                "identityfile" if names_alias && host.identity_file.is_none() => {
                    host.identity_file = Some(expand_home(value, home));
                }
                "identityfile" if wildcard_identity_file.is_none() => {
                    wildcard_identity_file = Some(expand_home(value, home));
                }
                _ => {}
            }
        }
    }
    host.identity_file = host.identity_file.or(wildcard_identity_file);

    host
}

fn expand_tokens(value: &str, alias: &str) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            expanded.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => expanded.push_str(alias),
            Some('%') => expanded.push('%'),
            Some(other) => {
                expanded.push('%');
                expanded.push(other);
            }
            None => expanded.push('%'),
        }
    }
    expanded
}

fn is_pattern(alias: &str) -> bool {
    alias.contains(['*', '?', '!'])
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
