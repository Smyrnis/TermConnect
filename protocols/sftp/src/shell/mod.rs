use std::{
    ffi::{OsStr, OsString},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
};

use termconnect_vfs::{ShellInvocation, Target};

pub fn invocation_for(target: &Target, sshpass: Option<PathBuf>) -> ShellInvocation {
    let (program, mut args, env) = match (&target.password, sshpass) {
        (Some(password), Some(sshpass_path)) => (
            sshpass_path.into_os_string(),
            vec![OsString::from("-e"), OsString::from("ssh")],
            vec![(OsString::from("SSHPASS"), OsString::from(password))],
        ),
        _ => (OsString::from("ssh"), Vec::new(), Vec::new()),
    };

    args.push(OsString::from("-p"));
    args.push(OsString::from(target.port.to_string()));
    if let Some(identity_file) = target.option("identity_file") {
        args.push(OsString::from("-i"));
        args.push(OsString::from(identity_file));
    }
    args.push(OsString::from(format!("{}@{}", target.username, target.host)));
    ShellInvocation { program, args, env }
}

pub fn find_sshpass_in(path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var).map(|dir| dir.join("sshpass")).find(|candidate| {
        candidate.is_file()
            && candidate.metadata().map(|metadata| metadata.permissions().mode() & 0o111 != 0).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests;
