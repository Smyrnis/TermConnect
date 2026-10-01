use std::{
    collections::HashMap,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use chrono::{DateTime, Utc};
use porthmos_vfs::{ErrorKind, FileSystem, ProtocolError};
use tokio::io::AsyncWriteExt;

use super::super::{Engine, Event, Internal, Location};
use crate::{
    Severity,
    config::settings::EditSettings,
    edit::{
        EditChoice, EditQuestionKind, EditorCommand, EditorExit, MAX_EDIT_BYTES, conflict_copy_name, hash_file,
        printable, resolve_editor,
    },
    tasks::Scope,
    transfer::{self, TransferOutcome},
    user_message,
};

#[derive(Clone)]
pub(crate) struct TempCopy {
    dir: PathBuf,
    file: PathBuf,
}

#[derive(Clone, Copy)]
pub(crate) struct Baseline {
    size: u64,
    modified: Option<u64>,
    hash: u64,
}

pub(crate) struct EditSession {
    location: Location,
    remote_path: PathBuf,
    name: String,
    editor: EditorCommand,
    temp: Option<TempCopy>,
    baseline: Option<Baseline>,
    pending: Option<EditQuestionKind>,
    awaiting_editor: bool,
    saving: bool,
}

pub(crate) enum Prepared {
    Ready { temp: TempCopy, baseline: Baseline },
    Cancelled,
    Failed(String),
}

pub(crate) enum EditEvent {
    Prepared { edit_id: u64, result: Prepared },
    Inspected { edit_id: u64, result: Result<bool, String>, aborted: Option<i32> },
    Conflict { edit_id: u64 },
    Uploaded { edit_id: u64, result: Result<String, String> },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UploadMode {
    Checked,
    Forced,
    Copy,
}

pub(crate) struct EditState {
    sessions: HashMap<u64, EditSession>,
    next_id: u64,
    settings: EditSettings,
    clock: fn() -> DateTime<Utc>,
    busy: bool,
}

impl EditState {
    pub(crate) fn new(settings: EditSettings) -> Self {
        Self { sessions: HashMap::new(), next_id: 0, settings, clock: Utc::now, busy: false }
    }
}

fn editor_failure(message: &str) -> String {
    format!("Couldn't start the editor ({message}). Set $EDITOR or [edit] editor")
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    }
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

fn remove_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

async fn prepare(
    fs: Arc<dyn FileSystem>, local: Arc<dyn FileSystem>, remote: PathBuf, dir: PathBuf, name: String,
    cancel: Arc<AtomicBool>,
) -> Prepared {
    let shown = printable(&name);
    let failed = |err: &dyn std::fmt::Display| Prepared::Failed(user_message(format!("Edit failed: {shown}"), err));
    let before = match fs.stat(&remote).await {
        Ok(metadata) => metadata,
        Err(err) => return failed(&err),
    };
    if before.is_dir() {
        return Prepared::Failed("Can't edit a folder".to_string());
    }
    if before.size > MAX_EDIT_BYTES {
        return Prepared::Failed(format!("{shown} is too large to edit (limit 64 MiB)"));
    }
    if let Err(err) = create_private_dir(&dir) {
        return Prepared::Failed(format!("Couldn't create a temporary folder for editing: {err}"));
    }
    let file = dir.join(&name);
    match transfer::run(fs.as_ref(), &remote, local.as_ref(), &file, &cancel, false, |_| {}).await {
        Ok(TransferOutcome::Completed) => {}
        Ok(TransferOutcome::Cancelled) => {
            remove_dir(&dir);
            return Prepared::Cancelled;
        }
        Err(err) => {
            remove_dir(&dir);
            return failed(&err);
        }
    }
    if let Err(err) = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)) {
        remove_dir(&dir);
        return failed(&err);
    }
    let hash_path = file.clone();
    let hash = match tokio::task::spawn_blocking(move || hash_file(&hash_path)).await {
        Ok(Ok(hash)) => hash,
        Ok(Err(err)) => {
            remove_dir(&dir);
            return failed(&err);
        }
        Err(err) => {
            remove_dir(&dir);
            return failed(&err);
        }
    };
    Prepared::Ready {
        temp: TempCopy { dir, file },
        baseline: Baseline { size: before.size, modified: before.modified, hash },
    }
}

async fn write_in_place(
    remote: &dyn FileSystem, local: &dyn FileSystem, file: &Path, target: &Path,
) -> Result<(), ProtocolError> {
    let size = local.stat(file).await?.size;
    let mut reader = local.open_read(file, 0).await?;
    let mut writer = remote.open_write_sized(target, 0, size).await?;
    tokio::io::copy(&mut reader, &mut writer.stream).await?;
    writer.stream.shutdown().await?;
    Ok(())
}

struct UploadJob {
    edit_id: u64,
    remote: PathBuf,
    file: PathBuf,
    baseline: Baseline,
    name: String,
    mode: UploadMode,
    at: DateTime<Utc>,
}

async fn upload(job: UploadJob, fs: Arc<dyn FileSystem>, local: Arc<dyn FileSystem>) -> EditEvent {
    let UploadJob { edit_id, remote, file, baseline, name, mode, at } = job;
    let failed = |err: &dyn std::fmt::Display| EditEvent::Uploaded {
        edit_id,
        result: Err(user_message(format!("Upload failed: {}", printable(&name)), err)),
    };
    if mode == UploadMode::Checked {
        let changed = match fs.stat(&remote).await {
            Ok(metadata) => metadata.size != baseline.size || metadata.modified != baseline.modified,
            Err(err) if err.kind() == ErrorKind::NotFound => true,
            Err(err) => return failed(&err),
        };
        if changed {
            return EditEvent::Conflict { edit_id };
        }
    }
    let target = match mode {
        UploadMode::Copy => remote.with_file_name(conflict_copy_name(&name, at)),
        UploadMode::Checked | UploadMode::Forced => remote,
    };
    if mode == UploadMode::Copy && fs.stat(&target).await.is_ok() {
        return failed(&format!("{} already exists", target.display()));
    }
    match write_in_place(fs.as_ref(), local.as_ref(), &file, &target).await {
        Ok(()) => EditEvent::Uploaded {
            edit_id,
            result: Ok(target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()),
        },
        Err(err) => failed(&err),
    }
}

impl Engine {
    pub(crate) fn edit_file(&mut self, location: Location, path: PathBuf) {
        let editor = match resolve_editor(self.edit.settings.editor.as_deref(), &self.env) {
            Ok(editor) => editor,
            Err(message) => {
                self.notice(Severity::Error, editor_failure(&message));
                return;
            }
        };
        match location {
            Location::Local => self.edit_local_file(path, editor),
            Location::Session(session_id) => self.edit_remote_file(session_id, path, editor),
        }
    }

    fn new_edit_session(&mut self, location: Location, path: PathBuf, editor: EditorCommand) -> u64 {
        let edit_id = self.edit.next_id;
        self.edit.next_id += 1;
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.edit.sessions.insert(
            edit_id,
            EditSession {
                location,
                remote_path: path,
                name,
                editor,
                temp: None,
                baseline: None,
                pending: None,
                awaiting_editor: false,
                saving: false,
            },
        );
        edit_id
    }

    fn edit_local_file(&mut self, path: PathBuf, editor: EditorCommand) {
        if path.is_dir() {
            self.notice(Severity::Warning, "Can't edit a folder");
            return;
        }
        let edit_id = self.new_edit_session(Location::Local, path.clone(), editor.clone());
        if let Some(session) = self.edit.sessions.get_mut(&edit_id) {
            session.awaiting_editor = true;
        }
        self.emit(Event::EditReady { edit_id, file: path, editor });
    }

    fn edit_remote_file(&mut self, session_id: u64, path: PathBuf, editor: EditorCommand) {
        let Some(fs) = self.fs_for(Location::Session(session_id)) else {
            self.notice(Severity::Error, "Edit failed: session disconnected");
            return;
        };
        let edit_id = self.new_edit_session(Location::Session(session_id), path.clone(), editor);
        let name = self.edit.sessions[&edit_id].name.clone();
        let dir = self.paths.edit_dir().join(format!("{}-{edit_id}", (self.edit.clock)().timestamp_millis()));
        let local = self.local_fs.clone();
        let internal = self.internal.clone();
        self.notice(Severity::Info, format!("Downloading {} to edit\u{2026}", printable(&name)));
        self.tasks.spawn("edit-download", Scope::Edit(edit_id), move |cancel| async move {
            let result = prepare(fs, local, path, dir, name, cancel).await;
            let _ = internal.send(Internal::Edit(EditEvent::Prepared { edit_id, result }));
        });
    }

    pub(crate) fn cancel_edit_downloads(&mut self) {
        let ids: Vec<u64> = self.edit.sessions.keys().copied().collect();
        for id in ids {
            self.tasks.cancel(Scope::Edit(id));
        }
    }

    fn drop_edit(&mut self, edit_id: u64) {
        self.tasks.forget(Scope::Edit(edit_id));
        if let Some(session) = self.edit.sessions.remove(&edit_id)
            && let Some(temp) = session.temp
        {
            remove_dir(&temp.dir);
        }
    }

    pub(crate) fn handle_edit_event(&mut self, event: EditEvent) {
        match event {
            EditEvent::Prepared { edit_id, result } => self.finish_prepare(edit_id, result),
            EditEvent::Inspected { edit_id, result, aborted } => self.finish_inspect(edit_id, result, aborted),
            EditEvent::Conflict { edit_id } => self.ask_edit(edit_id, EditQuestionKind::Conflict),
            EditEvent::Uploaded { edit_id, result } => self.finish_upload(edit_id, result),
        }
    }

    fn finish_prepare(&mut self, edit_id: u64, result: Prepared) {
        match result {
            Prepared::Ready { temp, baseline } => {
                if !self.edit.sessions.contains_key(&edit_id) {
                    remove_dir(&temp.dir);
                    return;
                }
                if self.tasks.take_cancelled(Scope::Edit(edit_id)) {
                    self.edit.sessions.remove(&edit_id);
                    remove_dir(&temp.dir);
                    self.notice(Severity::Info, "Edit cancelled");
                    return;
                }
                let Some(session) = self.edit.sessions.get_mut(&edit_id) else {
                    return;
                };
                session.baseline = Some(baseline);
                session.awaiting_editor = true;
                let editor = session.editor.clone();
                let file = temp.file.clone();
                session.temp = Some(temp);
                self.emit(Event::EditReady { edit_id, file, editor });
            }
            Prepared::Cancelled => {
                self.tasks.forget(Scope::Edit(edit_id));
                self.edit.sessions.remove(&edit_id);
                self.notice(Severity::Info, "Edit cancelled");
            }
            Prepared::Failed(message) => {
                self.tasks.forget(Scope::Edit(edit_id));
                self.edit.sessions.remove(&edit_id);
                self.notice(Severity::Error, message);
            }
        }
    }

    pub(crate) fn finish_edit(&mut self, edit_id: u64, exit: EditorExit) {
        let Some(session) = self.edit.sessions.get_mut(&edit_id) else {
            return;
        };
        if !session.awaiting_editor {
            return;
        }
        session.awaiting_editor = false;
        let location = session.location;
        match (location, exit) {
            (_, EditorExit::LaunchFailed(message)) => {
                self.drop_edit(edit_id);
                self.notice(Severity::Error, editor_failure(&message));
            }
            (Location::Local, EditorExit::Status(code)) => {
                self.drop_edit(edit_id);
                self.notice(Severity::Warning, format!("Editor exited with status {code}"));
                self.emit(Event::LocationChanged { location: Location::Local });
            }
            (Location::Local, EditorExit::Success) => {
                self.drop_edit(edit_id);
                self.emit(Event::LocationChanged { location: Location::Local });
            }
            (Location::Session(_), EditorExit::Status(code)) => self.inspect_edit(edit_id, Some(code)),
            (Location::Session(_), EditorExit::Success) => self.inspect_edit(edit_id, None),
        }
    }

    fn inspect_edit(&mut self, edit_id: u64, aborted: Option<i32>) {
        let Some(session) = self.edit.sessions.get_mut(&edit_id) else {
            return;
        };
        session.saving = true;
        let (Some(temp), Some(baseline)) = (session.temp.clone(), session.baseline) else {
            self.drop_edit(edit_id);
            self.notice(Severity::Error, "Edit failed: the temporary copy is missing");
            return;
        };
        let internal = self.internal.clone();
        self.tasks.spawn("edit-inspect", Scope::Edit(edit_id), move |_| async move {
            let file = temp.file.clone();
            let result = match tokio::task::spawn_blocking(move || hash_file(&file)).await {
                Ok(Ok(hash)) => Ok(hash != baseline.hash),
                Ok(Err(err)) => Err(err.to_string()),
                Err(err) => Err(err.to_string()),
            };
            let _ = internal.send(Internal::Edit(EditEvent::Inspected { edit_id, result, aborted }));
        });
    }

    fn finish_inspect(&mut self, edit_id: u64, result: Result<bool, String>, aborted: Option<i32>) {
        let Some(session) = self.edit.sessions.get(&edit_id) else {
            return;
        };
        let name = printable(&session.name);
        match (result, aborted) {
            (Err(message), _) => {
                self.keep_edit(edit_id, Severity::Error, &format!("Couldn't read the edited copy of {name}: {message}"))
            }
            (Ok(false), None) => {
                self.drop_edit(edit_id);
                self.notice(Severity::Info, format!("No changes to {name}"));
            }
            (Ok(false), Some(code)) => {
                self.drop_edit(edit_id);
                self.notice(Severity::Warning, format!("Editor exited with status {code}; nothing was uploaded"));
            }
            (Ok(true), Some(code)) => self.keep_edit(
                edit_id,
                Severity::Warning,
                &format!("Editor exited with status {code}; nothing was uploaded"),
            ),
            (Ok(true), None) if self.edit.settings.auto_upload => self.start_upload(edit_id, UploadMode::Checked),
            (Ok(true), None) => self.ask_edit(edit_id, EditQuestionKind::Upload),
        }
    }

    pub(crate) fn edit_in_progress(&self, edit_id: u64) -> bool {
        self.edit.sessions.contains_key(&edit_id)
    }

    pub(crate) fn fail_edit(&mut self, edit_id: u64, reason: &str) {
        self.keep_edit(edit_id, Severity::Error, reason);
    }

    fn keep_edit(&mut self, edit_id: u64, severity: Severity, reason: &str) {
        self.tasks.forget(Scope::Edit(edit_id));
        let Some(session) = self.edit.sessions.remove(&edit_id) else {
            return;
        };
        let reason = printable(&reason.replace('\n', " "));
        match session.temp {
            Some(temp) => self.notice(
                severity,
                format!("{reason}; your changes are kept in {}", printable(&temp.file.display().to_string())),
            ),
            None => self.notice(severity, reason),
        }
    }

    pub(crate) fn publish_edit_busy(&mut self) {
        let busy = self.edit.sessions.values().any(|session| session.saving);
        if busy != self.edit.busy {
            self.edit.busy = busy;
            self.emit(Event::EditsBusy(busy));
        }
    }

    fn ask_edit(&mut self, edit_id: u64, kind: EditQuestionKind) {
        let Some(session) = self.edit.sessions.get_mut(&edit_id) else {
            return;
        };
        session.pending = Some(kind);
        session.saving = false;
        let name = session.name.clone();
        self.emit(Event::EditQuestion { edit_id, name, kind });
    }

    pub(crate) fn resolve_edit(&mut self, edit_id: u64, choice: EditChoice) {
        let Some(kind) = self.edit.sessions.get_mut(&edit_id).and_then(|session| session.pending.take()) else {
            return;
        };
        match (kind, choice) {
            (EditQuestionKind::Upload, EditChoice::Upload) => self.start_upload(edit_id, UploadMode::Checked),
            (EditQuestionKind::Conflict, EditChoice::Upload) => self.start_upload(edit_id, UploadMode::Forced),
            (EditQuestionKind::Conflict, EditChoice::KeepCopy) => self.start_upload(edit_id, UploadMode::Copy),
            (_, EditChoice::Cancel) | (EditQuestionKind::Upload, EditChoice::KeepCopy) => {
                self.keep_edit(edit_id, Severity::Warning, "Edit cancelled")
            }
        }
    }

    fn start_upload(&mut self, edit_id: u64, mode: UploadMode) {
        let Some(session) = self.edit.sessions.get_mut(&edit_id) else {
            return;
        };
        let (Some(temp), Some(baseline)) = (session.temp.clone(), session.baseline) else {
            return;
        };
        session.saving = true;
        let (location, remote, name) = (session.location, session.remote_path.clone(), session.name.clone());
        let Some(fs) = self.fs_for(location) else {
            self.keep_edit(
                edit_id,
                Severity::Error,
                &format!("Upload failed: session disconnected ({})", printable(&name)),
            );
            return;
        };
        let local = self.local_fs.clone();
        let internal = self.internal.clone();
        let job = UploadJob { edit_id, remote, file: temp.file, baseline, name, mode, at: (self.edit.clock)() };
        self.tasks.spawn("edit-upload", Scope::Edit(edit_id), move |_| async move {
            let event = upload(job, fs, local).await;
            let _ = internal.send(Internal::Edit(event));
        });
    }

    fn finish_upload(&mut self, edit_id: u64, result: Result<String, String>) {
        let Some(location) = self.edit.sessions.get(&edit_id).map(|session| session.location) else {
            return;
        };
        match result {
            Ok(name) => {
                self.drop_edit(edit_id);
                self.emit(Event::LocationChanged { location });
                self.notice(Severity::Info, format!("Uploaded {}", printable(&name)));
            }
            Err(message) => self.keep_edit(edit_id, Severity::Error, &message),
        }
    }
}

#[cfg(test)]
mod tests;
