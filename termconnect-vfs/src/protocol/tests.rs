use super::*;

#[test]
fn target_debug_redacts_the_password() {
    let target = Target {
        name: "web".into(),
        host: "example.com".into(),
        port: 22,
        username: "deploy".into(),
        password: Some("hunter2".into()),
        options: BTreeMap::new(),
    };
    let printed = format!("{target:?}");
    assert!(!printed.contains("hunter2"));
    assert!(printed.contains("<redacted>"));
}

#[test]
fn shell_invocation_debug_redacts_environment_values() {
    let invocation = ShellInvocation {
        program: "sshpass".into(),
        args: vec!["-e".into()],
        env: vec![("SSHPASS".into(), "hunter2".into())],
    };
    let printed = format!("{invocation:?}");
    assert!(!printed.contains("hunter2"));
    assert!(printed.contains("SSHPASS"));
}

#[test]
fn target_option_reads_protocol_specific_keys() {
    let mut options = BTreeMap::new();
    options.insert("identity_file".to_string(), "/k".to_string());
    let target = Target { name: "n".into(), host: "h".into(), port: 22, username: "u".into(), password: None, options };
    assert_eq!(target.option("identity_file"), Some("/k"));
    assert_eq!(target.option("remote_path"), None);
}

#[test]
fn shell_invocation_builds_a_command_with_its_program_args_and_env() {
    let invocation = ShellInvocation {
        program: "ssh".into(),
        args: vec!["-p".into(), "22".into()],
        env: vec![("SSHPASS".into(), "x".into())],
    };
    let command = invocation.to_command();
    assert_eq!(command.get_program(), "ssh");
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["-p", "22"]);
    assert_eq!(
        command.get_envs().collect::<Vec<_>>(),
        [(std::ffi::OsStr::new("SSHPASS"), Some(std::ffi::OsStr::new("x")))]
    );
}
