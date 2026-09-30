use chrono::{TimeZone, Utc};

use super::*;

fn env_with(editor: Option<&str>) -> Environment {
    Environment { editor: editor.map(str::to_string), ..Environment::default() }
}

#[test]
fn a_command_is_split_on_whitespace() {
    assert_eq!(split_command("code --wait").unwrap(), ["code", "--wait"]);
    assert_eq!(split_command("  vim   -u  NONE ").unwrap(), ["vim", "-u", "NONE"]);
}

#[test]
fn quotes_group_words() {
    assert_eq!(split_command("\"/opt/my editor/bin/ed\" -w").unwrap(), ["/opt/my editor/bin/ed", "-w"]);
    assert_eq!(split_command("ed 'two words' \"and two\"").unwrap(), ["ed", "two words", "and two"]);
    assert_eq!(split_command("ed ''").unwrap(), ["ed", ""]);
}

#[test]
fn backslashes_escape_outside_quotes_and_inside_double_quotes_only() {
    assert_eq!(split_command("my\\ editor -w").unwrap(), ["my editor", "-w"]);
    assert_eq!(split_command("ed \"a\\\"b\" \"c\\\\d\" \"e\\nf\"").unwrap(), ["ed", "a\"b", "c\\d", "e\\nf"]);
    assert_eq!(split_command("ed 'a\\b'").unwrap(), ["ed", "a\\b"]);
}

#[test]
fn an_unterminated_quote_or_a_trailing_backslash_is_an_error() {
    assert!(split_command("ed \"oops").unwrap_err().contains("quote"));
    assert!(split_command("ed 'oops").unwrap_err().contains("quote"));
    assert!(split_command("ed oops\\").unwrap_err().contains("backslash"));
}

#[test]
fn shell_syntax_is_not_interpreted() {
    assert_eq!(split_command("ed $HOME `x` ; rm").unwrap(), ["ed", "$HOME", "`x`", ";", "rm"]);
}

#[test]
fn the_setting_beats_the_environment_and_the_environment_beats_vi() {
    let env = env_with(Some("nano -w"));

    assert_eq!(
        resolve_editor(Some("code --wait"), &env).unwrap(),
        EditorCommand { program: "code".to_string(), args: vec!["--wait".to_string()] }
    );
    assert_eq!(
        resolve_editor(None, &env).unwrap(),
        EditorCommand { program: "nano".to_string(), args: vec!["-w".to_string()] }
    );
    assert_eq!(
        resolve_editor(None, &env_with(None)).unwrap(),
        EditorCommand { program: "vi".to_string(), args: Vec::new() }
    );
}

#[test]
fn blank_values_fall_through_to_the_next_source() {
    assert_eq!(resolve_editor(Some("   "), &env_with(Some("nano"))).unwrap().program, "nano");
    assert_eq!(resolve_editor(Some(""), &env_with(Some("  "))).unwrap().program, "vi");
}

#[test]
fn a_bad_command_is_reported_not_guessed() {
    assert!(resolve_editor(Some("ed \"oops"), &env_with(None)).is_err());
    assert!(resolve_editor(Some("\"\""), &env_with(None)).unwrap_err().contains("empty"));
}

#[test]
fn editor_from_prefers_visual_and_ignores_blank_values() {
    assert_eq!(Environment::editor_from(Some("a".into()), Some("b".into())), Some("a".to_string()));
    assert_eq!(Environment::editor_from(None, Some("b".into())), Some("b".to_string()));
    assert_eq!(Environment::editor_from(Some("  ".into()), Some("b".into())), Some("b".to_string()));
    assert_eq!(Environment::editor_from(Some("".into()), Some(" ".into())), None);
    assert_eq!(Environment::editor_from(None, None), None);
}

#[test]
fn a_conflict_copy_keeps_the_extension_at_the_end() {
    let at = Utc.with_ymd_and_hms(2026, 9, 30, 10, 15, 0).unwrap();

    assert_eq!(conflict_copy_name("notes.txt", at), "notes.conflict-20260930-101500.txt");
    assert_eq!(conflict_copy_name("Makefile", at), "Makefile.conflict-20260930-101500");
    assert_eq!(conflict_copy_name(".bashrc", at), ".bashrc.conflict-20260930-101500");
    assert_eq!(conflict_copy_name("a.tar.gz", at), "a.tar.conflict-20260930-101500.gz");
}

#[test]
fn hashing_tells_identical_and_different_files_apart() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let same = dir.path().join("same");
    let other = dir.path().join("other");
    let empty = dir.path().join("empty");
    std::fs::write(&first, vec![7u8; 200_000]).unwrap();
    std::fs::write(&same, vec![7u8; 200_000]).unwrap();
    let mut changed = vec![7u8; 200_000];
    changed[199_999] = 8;
    std::fs::write(&other, changed).unwrap();
    std::fs::write(&empty, b"").unwrap();

    assert_eq!(hash_file(&first).unwrap(), hash_file(&same).unwrap());
    assert_ne!(hash_file(&first).unwrap(), hash_file(&other).unwrap());
    assert_ne!(hash_file(&first).unwrap(), hash_file(&empty).unwrap());
    assert!(hash_file(&dir.path().join("missing")).is_err());
}

#[test]
fn printable_replaces_control_characters_only() {
    assert_eq!(printable("a\nb\u{1b}[0m\tc \u{fc}"), "a?b?[0m?c \u{fc}");
}
