use std::collections::BTreeMap;

use porthmos_core::{
    ConnectionForm, OptionField, OptionKind, ProtocolInfo,
    profiles::{ConnectionEntry, ProfileDraft},
};

use crate::widgets::dialog::{FieldKind, FormDialog, FormField};

const UNAVAILABLE_PORT: u16 = 22;
const COMMON_KEYS: [&str; 7] = ["protocol", "name", "host", "port", "username", "password", "remote_path"];

struct Common {
    name: String,
    host: String,
    port: String,
    username: String,
    password: String,
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

fn option_field(field: &OptionField, saved: Option<&str>) -> FormField {
    match &field.kind {
        OptionKind::Text { default } => FormField::text(field.key, field.label, saved.unwrap_or(default)),
        OptionKind::Secret => FormField::masked(field.key, field.label, saved.unwrap_or_default()),
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
    saved_options: &BTreeMap<String, String>,
) -> Vec<FormField> {
    let form = form_for(protocols, protocol);
    let mut fields = vec![
        FormField::choice("protocol", "Protocol", protocol_choices(protocols, unavailable), protocol),
        FormField::text("name", "Name", common.name),
        FormField::text("host", form.host.label, common.host),
        FormField::text("port", form.port.label, common.port),
        FormField::text("username", form.username.label, common.username),
        FormField::masked("password", form.password.label, common.password),
        FormField::text("remote_path", "Remote folder", common.remote_path),
    ];
    fields
        .extend(form.options.iter().map(|field| option_field(field, saved_options.get(field.key).map(String::as_str))));
    fields
}

pub(super) fn build(title: &str, protocols: &[ProtocolInfo], existing: Option<&ConnectionEntry>) -> Option<FormDialog> {
    let fields = match existing {
        Some(entry) => fields(
            protocols,
            &entry.protocol,
            Some(&entry.protocol),
            Common {
                name: entry.name.clone(),
                host: entry.host.clone(),
                port: entry.port.to_string(),
                username: entry.username.clone(),
                password: entry.password.clone().unwrap_or_default(),
                remote_path: entry.option("remote_path").unwrap_or_default().to_string(),
            },
            &entry.options,
        ),
        None => {
            let first = protocols.first()?;
            fields(
                protocols,
                first.id,
                None,
                Common {
                    name: String::new(),
                    host: String::new(),
                    port: first.form.port.default.to_string(),
                    username: String::new(),
                    password: String::new(),
                    remote_path: String::new(),
                },
                &BTreeMap::new(),
            )
        }
    };
    let mut dialog = FormDialog::new(title, fields);
    match existing {
        Some(entry) => {
            dialog.remembered = entry.options.clone();
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
    let common = Common {
        name: form.value("name").unwrap_or_default(),
        host: form.value("host").unwrap_or_default(),
        port: if port_untouched { new_form.port.default.to_string() } else { typed_port },
        username: form.value("username").unwrap_or_default(),
        password: form.value("password").unwrap_or_default(),
        remote_path: form.value("remote_path").unwrap_or_default(),
    };
    let unavailable = unavailable_choice(form, protocols);
    for field in form.fields.iter().filter(|field| !COMMON_KEYS.contains(&field.key)) {
        form.remembered.insert(field.key.to_string(), field.submitted_value());
    }
    if port_untouched {
        form.prefilled.insert("port", common.port.clone());
    } else {
        form.prefilled.remove("port");
    }
    form.fields = fields(protocols, &protocol, unavailable.as_deref(), common, &form.remembered);
    form.focused = 0;
    form.error = None;
}

pub(super) fn draft(values: Vec<(&'static str, String)>) -> ProfileDraft {
    let mut draft = ProfileDraft::default();
    for (key, value) in values {
        match key {
            "protocol" => draft.protocol = value,
            "name" => draft.name = value,
            "host" => draft.host = value,
            "port" => draft.port = value,
            "username" => draft.username = value,
            "password" => draft.password = value,
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
