use std::collections::BTreeMap;

use porthmos_core::{
    ConnectionForm, OptionField, OptionKind, ProtocolInfo,
    profiles::{ConnectionEntry, ProfileDraft, SecretEdit},
};

use crate::widgets::dialog::{FieldKind, FormDialog, FormField, KEPT_SECRET};

const UNAVAILABLE_PORT: u16 = 22;
const COMMON_KEYS: [&str; 9] =
    ["protocol", "name", "group", "tags", "host", "port", "username", "password", "remote_path"];

struct Common {
    name: String,
    group: String,
    tags: String,
    host: String,
    port: String,
    username: String,
    password: String,
    saved_password: bool,
    remote_path: String,
}

fn protocol_choices(protocols: &[ProtocolInfo], unavailable: Option<&str>) -> Vec<(String, String)> {
    let mut choices: Vec<(String, String)> =
        protocols.iter().map(|info| (info.id.to_string(), info.display_name.to_string())).collect();
    if let Some(id) = unavailable
        && !protocols.iter().any(|info| info.id == id)
    {
        choices.push((id.to_string(), format!("{id} (not available)")));
    }
    choices
}

fn form_for(protocols: &[ProtocolInfo], id: &str) -> ConnectionForm {
    protocols
        .iter()
        .find(|info| info.id == id)
        .map(|info| info.form.clone())
        .unwrap_or_else(|| ConnectionForm::standard(UNAVAILABLE_PORT))
}

fn option_field(field: &OptionField, saved: Option<&str>, saved_secrets: &[String]) -> FormField {
    match &field.kind {
        OptionKind::Secret if saved_secrets.iter().any(|marker| marker == field.key) => {
            FormField::saved_secret(field.key, field.label)
        }
        OptionKind::Text { default } => FormField::text(field.key, field.label, saved.unwrap_or(default)),
        OptionKind::Secret => {
            FormField::masked(field.key, field.label, saved.filter(|value| *value != KEPT_SECRET).unwrap_or_default())
        }
        OptionKind::Choice { choices, default } => {
            let choices: Vec<(String, String)> =
                choices.iter().map(|choice| (choice.value.to_string(), choice.label.to_string())).collect();
            let wanted = saved.filter(|value| choices.iter().any(|(choice, _)| choice == value)).unwrap_or(default);
            FormField::choice(field.key, field.label, choices, wanted)
        }
        OptionKind::Toggle { default } => {
            let choices = vec![("true".to_string(), "Yes".to_string()), ("false".to_string(), "No".to_string())];
            let fallback = if *default { "true" } else { "false" };
            let wanted = saved.filter(|value| *value == "true" || *value == "false").unwrap_or(fallback);
            FormField::choice(field.key, field.label, choices, wanted)
        }
    }
}

fn fields(
    protocols: &[ProtocolInfo], protocol: &str, unavailable: Option<&str>, common: Common,
    saved_options: &BTreeMap<String, String>, saved_secrets: &[String],
) -> Vec<FormField> {
    let form = form_for(protocols, protocol);
    let mut fields = vec![
        FormField::choice("protocol", "Protocol", protocol_choices(protocols, unavailable), protocol),
        FormField::text("name", "Name", common.name),
        FormField::text("group", "Group", common.group),
        FormField::text("tags", "Tags", common.tags),
        FormField::text("host", form.host.label, common.host),
        FormField::text("port", form.port.label, common.port),
        FormField::text("username", form.username.label, common.username),
        if common.saved_password {
            FormField::saved_secret("password", form.password.label)
        } else {
            FormField::masked("password", form.password.label, common.password)
        },
        FormField::text("remote_path", "Remote folder", common.remote_path),
    ];
    fields.extend(
        form.options
            .iter()
            .map(|field| option_field(field, saved_options.get(field.key).map(String::as_str), saved_secrets)),
    );
    fields
}

fn plain_options(protocols: &[ProtocolInfo], entry: &ConnectionEntry) -> BTreeMap<String, String> {
    let secrets = secret_keys(protocols, &entry.protocol);
    entry
        .options
        .iter()
        .filter(|(key, _)| !secrets.contains(&key.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub(super) fn build(title: &str, protocols: &[ProtocolInfo], existing: Option<&ConnectionEntry>) -> Option<FormDialog> {
    let fields = match existing {
        Some(entry) => fields(
            protocols,
            &entry.protocol,
            Some(&entry.protocol),
            Common {
                name: entry.name.clone(),
                group: entry.group.clone().unwrap_or_default(),
                tags: entry.tags.join(", "),
                host: entry.host.clone(),
                port: entry.port.to_string(),
                username: entry.username.clone(),
                password: String::new(),
                saved_password: entry.saved_password,
                remote_path: entry.option("remote_path").unwrap_or_default().to_string(),
            },
            &plain_options(protocols, entry),
            &entry.in_keyring,
        ),
        None => {
            let first = protocols.first()?;
            fields(
                protocols,
                first.id,
                None,
                Common {
                    name: String::new(),
                    group: String::new(),
                    tags: String::new(),
                    host: String::new(),
                    port: first.form.port.default.to_string(),
                    username: String::new(),
                    password: String::new(),
                    saved_password: false,
                    remote_path: String::new(),
                },
                &BTreeMap::new(),
                &[],
            )
        }
    };
    let mut dialog = FormDialog::new(title, fields);
    match existing {
        Some(entry) => {
            dialog.remembered = plain_options(protocols, entry);
            dialog.saved_secrets = Some((entry.protocol.clone(), entry.in_keyring.clone()));
            let registered_default =
                protocols.iter().find(|info| info.id == entry.protocol).map(|info| info.form.port.default);
            if registered_default == Some(entry.port) {
                dialog.prefilled.insert("port", entry.port.to_string());
            }
        }
        None => {
            if let Some(port) = dialog.value("port") {
                dialog.prefilled.insert("port", port);
            }
        }
    }
    Some(dialog)
}

fn unavailable_choice(form: &FormDialog, protocols: &[ProtocolInfo]) -> Option<String> {
    let FieldKind::Choice { choices, .. } = &form.fields.first()?.kind else {
        return None;
    };
    choices.iter().map(|(id, _)| id).find(|id| !protocols.iter().any(|info| info.id == id.as_str())).cloned()
}

pub(super) fn rebuild_for_protocol(form: &mut FormDialog, protocols: &[ProtocolInfo]) {
    let Some(protocol) = form.value("protocol") else {
        return;
    };
    let new_form = form_for(protocols, &protocol);
    let typed_port = form.value("port").unwrap_or_default();
    let port_untouched =
        typed_port.trim().is_empty() || form.prefilled.get("port").is_some_and(|port| port == typed_port.trim());
    let typed_password = form.value("password").filter(|value| value != KEPT_SECRET).unwrap_or_default();
    let restored: Vec<String> = form
        .saved_secrets
        .as_ref()
        .filter(|(saved_protocol, _)| *saved_protocol == protocol)
        .map(|(_, markers)| markers.clone())
        .unwrap_or_default();
    let common = Common {
        name: form.value("name").unwrap_or_default(),
        group: form.value("group").unwrap_or_default(),
        tags: form.value("tags").unwrap_or_default(),
        host: form.value("host").unwrap_or_default(),
        port: if port_untouched { new_form.port.default.to_string() } else { typed_port },
        username: form.value("username").unwrap_or_default(),
        saved_password: typed_password.is_empty() && restored.iter().any(|marker| marker == "password"),
        password: typed_password,
        remote_path: form.value("remote_path").unwrap_or_default(),
    };
    let unavailable = unavailable_choice(form, protocols);
    for field in form.fields.iter().filter(|field| !COMMON_KEYS.contains(&field.key)) {
        let value = field.submitted_value();
        if value == KEPT_SECRET {
            form.remembered.remove(field.key);
        } else {
            form.remembered.insert(field.key.to_string(), value);
        }
    }
    if port_untouched {
        form.prefilled.insert("port", common.port.clone());
    } else {
        form.prefilled.remove("port");
    }
    let untouched: Vec<String> =
        restored.into_iter().filter(|key| form.remembered.get(key).is_none_or(String::is_empty)).collect();
    form.fields = fields(protocols, &protocol, unavailable.as_deref(), common, &form.remembered, &untouched);
    form.focused = 0;
    form.error = None;
}

pub(super) fn build_labels(entry: &ConnectionEntry) -> FormDialog {
    let mut fields = vec![
        FormField::text("group", "Group", entry.group.clone().unwrap_or_default()),
        FormField::text("tags", "Tags", entry.tags.join(", ")),
    ];
    if entry.saved_password {
        let choices = vec![("keep".to_string(), "Keep".to_string()), ("forget".to_string(), "Forget".to_string())];
        fields.push(FormField::choice("saved_password", "Saved password", choices, "keep"));
    }
    FormDialog::new(format!("Labels for {}", entry.name), fields)
}

pub(super) fn secret_keys(protocols: &[ProtocolInfo], protocol: &str) -> Vec<&'static str> {
    form_for(protocols, protocol)
        .options
        .iter()
        .filter(|field| field.kind == OptionKind::Secret)
        .map(|field| field.key)
        .collect()
}

fn secret_edit(value: String) -> SecretEdit {
    if value == KEPT_SECRET {
        SecretEdit::Keep
    } else if value.is_empty() {
        SecretEdit::Clear
    } else {
        SecretEdit::Replace(value)
    }
}

pub(super) fn draft(values: Vec<(&'static str, String)>, secret_keys: &[&str]) -> ProfileDraft {
    let mut draft = ProfileDraft::default();
    for (key, value) in values {
        if secret_keys.contains(&key) {
            draft.secret_options.insert(key.to_string(), secret_edit(value));
            continue;
        }
        match key {
            "protocol" => draft.protocol = value,
            "name" => draft.name = value,
            "group" => draft.group = value,
            "tags" => draft.tags = value,
            "host" => draft.host = value,
            "port" => draft.port = value,
            "username" => draft.username = value,
            "password" => draft.password = secret_edit(value),
            "remote_path" => draft.remote_path = value,
            _ => {
                draft.options.insert(key.to_string(), value);
            }
        }
    }
    draft
}

#[cfg(test)]
mod tests;
