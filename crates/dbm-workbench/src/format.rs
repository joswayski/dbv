//! Presentation-agnostic helpers shared by the native workbench views.
//!
//! These mirror the behavior of the React UI (`apps/desktop/ui/src`) so the
//! native frontends show the same values, CSV output, presets, and
//! confirmations as the shipping Tauri app.

use dbm_engine::error::AppError;
use dbm_engine::models::{
    ConnectionProfile, DatabaseEngine, SaveProfileInput, SchemaNode, TlsMode,
};
use serde_json::Value;

pub const DEFAULT_CONNECTION_COLOR: &str = "#38bdf8";
pub const CONNECTION_COLORS: [&str; 6] = [
    "#38bdf8", "#22c55e", "#a78bfa", "#f59e0b", "#ef4444", "#64748b",
];
pub const MAX_PREVIEW_ROWS: u32 = 200;
pub const EXPORT_PAGE_SIZE: u32 = 1_000;
pub const LARGE_EXPORT_WARNING_ROWS: u64 = 100_000;
pub const QUERY_ROW_LIMIT: u32 = 10_000;

pub struct EnginePreset {
    pub label: &'static str,
    pub name: &'static str,
    pub port: u16,
    pub username: &'static str,
    pub default_database: &'static str,
    pub url_placeholder: &'static str,
}

pub fn preset(engine: DatabaseEngine) -> EnginePreset {
    match engine {
        DatabaseEngine::Postgres => EnginePreset {
            label: "PostgreSQL",
            name: "Local PostgreSQL",
            port: 5432,
            username: "postgres",
            default_database: "postgres",
            url_placeholder: "postgresql://user:password@host:5432/database",
        },
        DatabaseEngine::Mysql => EnginePreset {
            label: "MySQL",
            name: "Local MySQL",
            port: 3306,
            username: "root",
            default_database: "mysql",
            url_placeholder: "mysql://user:password@host:3306/database",
        },
        DatabaseEngine::Redis => EnginePreset {
            label: "Redis",
            name: "Local Redis",
            port: 6379,
            username: "default",
            default_database: "0",
            url_placeholder: "redis://default:password@host:6379/0",
        },
    }
}

pub fn engine_label(engine: DatabaseEngine) -> &'static str {
    preset(engine).label
}

pub fn default_query_text(engine: DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Redis => "PING",
        _ => "SELECT now();",
    }
}

pub fn fallback_database(engine: DatabaseEngine) -> &'static str {
    preset(engine).default_database
}

pub fn profile_color(profile: &ConnectionProfile) -> &str {
    profile
        .color
        .as_deref()
        .filter(|color| !color.is_empty())
        .unwrap_or(DEFAULT_CONNECTION_COLOR)
}

pub fn default_profile_input(profile: Option<&ConnectionProfile>) -> SaveProfileInput {
    let engine = profile.map_or(DatabaseEngine::Postgres, |profile| profile.engine);
    let preset = preset(engine);
    SaveProfileInput {
        id: profile.map(|profile| profile.id),
        name: profile.map_or_else(|| preset.name.to_owned(), |profile| profile.name.clone()),
        color: Some(
            profile
                .and_then(|profile| profile.color.clone())
                .unwrap_or_else(|| DEFAULT_CONNECTION_COLOR.to_owned()),
        ),
        engine,
        host: profile.map_or_else(|| "localhost".to_owned(), |profile| profile.host.clone()),
        port: profile.map_or(preset.port, |profile| profile.port),
        username: profile.map_or_else(
            || preset.username.to_owned(),
            |profile| profile.username.clone(),
        ),
        default_database: profile.map_or_else(
            || preset.default_database.to_owned(),
            |profile| profile.default_database.clone(),
        ),
        tls_mode: profile.map_or(TlsMode::Preferred, |profile| profile.tls_mode.clone()),
        ca_cert_path: profile.and_then(|profile| profile.ca_cert_path.clone()),
        ssh: profile.and_then(|profile| profile.ssh.clone()),
        read_only: profile.is_some_and(|profile| profile.read_only),
        password: None,
    }
}

/// Mirrors `applyEngineDefaults` in the React UI: switching engines only
/// replaces fields that still hold the previous engine's preset.
pub fn apply_engine_defaults(form: &mut SaveProfileInput, engine: DatabaseEngine) {
    let previous = preset(form.engine);
    let next = preset(engine);
    if form.name == previous.name {
        form.name = next.name.to_owned();
    }
    if form.port == previous.port {
        form.port = next.port;
    }
    if form.username == previous.username {
        form.username = next.username.to_owned();
    }
    if form.default_database == previous.default_database {
        form.default_database = next.default_database.to_owned();
    }
    form.engine = engine;
}

pub fn display_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

pub fn csv_document(columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(csv_line(
        &columns
            .iter()
            .map(|name| Value::String(name.clone()))
            .collect::<Vec<_>>(),
    ));
    // Table pages can carry a trailing `__dbm_xmin` value for mutations; the
    // document only has columns for the visible ones.
    lines.extend(
        rows.iter()
            .map(|row| csv_line(&row[..row.len().min(columns.len())])),
    );
    lines.join("\n")
}

pub fn csv_line(values: &[Value]) -> String {
    values
        .iter()
        .map(|value| {
            let text = display_value(value);
            if text.contains(['"', ',', '\r', '\n']) {
                format!("\"{}\"", text.replace('"', "\"\""))
            } else {
                text
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if "\\/:*?\"<>|".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect()
}

/// Mirrors `requiresConfirmation` in the React UI.
pub fn requires_confirmation(sql: &str, engine: DatabaseEngine) -> bool {
    if engine == DatabaseEngine::Redis {
        return first_word(sql).is_some_and(|word| {
            word.eq_ignore_ascii_case("flushall") || word.eq_ignore_ascii_case("flushdb")
        });
    }
    let stripped = strip_sql_comments(sql);
    let tokens = tokens(&stripped);
    let has = |word: &str| tokens.iter().any(|token| token.eq_ignore_ascii_case(word));
    has("drop") || has("truncate") || ((has("delete") || has("update")) && !has("where"))
}

/// Mirrors `describeSchemaRefresh` in the React UI: reports which objects were
/// added or removed by a schema refresh.
pub fn describe_schema_refresh(
    previous: &[SchemaNode],
    next: &[SchemaNode],
    kind: &str,
) -> (bool, String) {
    let previous_objects = schema_objects(previous);
    let next_objects = schema_objects(next);
    let previous_keys: std::collections::HashSet<&str> = previous_objects
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    let next_keys: std::collections::HashSet<&str> =
        next_objects.iter().map(|(key, _)| key.as_str()).collect();
    let added: Vec<&str> = next_objects
        .iter()
        .filter(|(key, _)| !previous_keys.contains(key.as_str()))
        .map(|(_, label)| label.as_str())
        .collect();
    let removed: Vec<&str> = previous_objects
        .iter()
        .filter(|(key, _)| !next_keys.contains(key.as_str()))
        .map(|(_, label)| label.as_str())
        .collect();
    if added.is_empty() && removed.is_empty() {
        return (false, format!("{kind} is already up to date."));
    }
    let mut changes = Vec::new();
    if !added.is_empty() {
        changes.push(format!("Added {}", summarize_schema_objects(&added)));
    }
    if !removed.is_empty() {
        changes.push(format!("Removed {}", summarize_schema_objects(&removed)));
    }
    (true, format!("{kind} refreshed · {}.", changes.join(" · ")))
}

fn schema_objects(nodes: &[SchemaNode]) -> Vec<(String, String)> {
    let mut objects = Vec::new();
    fn visit(node: &SchemaNode, objects: &mut Vec<(String, String)>) {
        let qualified_name = match (&node.schema, &node.table) {
            (Some(schema), Some(table)) => format!("{schema}.{table}"),
            _ => node.name.clone(),
        };
        objects.push((
            format!("{}:{qualified_name}", node.kind),
            format!("{} {qualified_name}", node.kind),
        ));
        for child in &node.children {
            visit(child, objects);
        }
    }
    for node in nodes {
        visit(node, &mut objects);
    }
    objects.sort();
    objects
}

fn summarize_schema_objects(labels: &[&str]) -> String {
    if labels.len() == 1 {
        return labels[0].to_owned();
    }
    let visible = labels
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    let remainder = labels.len().saturating_sub(3);
    if remainder > 0 {
        format!("{} objects: {visible}, and {remainder} more", labels.len())
    } else {
        format!("{} objects: {visible}", labels.len())
    }
}

pub fn error_message(error: &AppError) -> String {
    let message = error.to_string();
    if let AppError::Credential(detail) = error {
        let lowered = detail.to_ascii_lowercase();
        if lowered.contains("cancel")
            || lowered.contains("denied")
            || lowered.contains("interaction")
        {
            return "DBM could not read this connection's saved password because access to the operating system credential manager was not approved. Approve the system prompt, then select the connection again.".to_owned();
        }
    }
    message
}

fn first_word(sql: &str) -> Option<&str> {
    sql.split_whitespace().next()
}

fn strip_sql_comments(sql: &str) -> String {
    let characters: Vec<char> = sql.chars().collect();
    let mut result = String::with_capacity(sql.len());
    let mut index = 0;
    let mut single_quoted = false;
    let mut double_quoted = false;
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        if single_quoted {
            result.push(character);
            if character == '\\' {
                if let Some(escaped) = next {
                    result.push(escaped);
                    index += 2;
                    continue;
                }
            } else if character == '\'' && next == Some('\'') {
                result.push('\'');
                index += 2;
                continue;
            } else if character == '\'' {
                single_quoted = false;
            }
            index += 1;
            continue;
        }
        if double_quoted {
            result.push(character);
            if character == '"' && next == Some('"') {
                result.push('"');
                index += 2;
                continue;
            }
            if character == '"' {
                double_quoted = false;
            }
            index += 1;
            continue;
        }
        if character == '-' && next == Some('-') {
            while index < characters.len() && characters[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if character == '/' && next == Some('*') {
            index += 2;
            while index < characters.len() {
                if characters[index] == '*' && characters.get(index + 1) == Some(&'/') {
                    index += 2;
                    break;
                }
                index += 1;
            }
            result.push(' ');
            continue;
        }
        if character == '\'' {
            single_quoted = true;
        } else if character == '"' {
            double_quoted = true;
        }
        result.push(character);
        index += 1;
    }
    result
}

fn tokens(sql: &str) -> Vec<String> {
    sql.split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_value_matches_the_react_ui() {
        assert_eq!(display_value(&Value::Null), "NULL");
        assert_eq!(display_value(&json!(true)), "true");
        assert_eq!(display_value(&json!(42)), "42");
        assert_eq!(display_value(&json!("hi")), "hi");
        assert_eq!(display_value(&json!({ "a": 1 })), "{\"a\":1}");
    }

    #[test]
    fn csv_lines_escape_quotes_commas_and_newlines() {
        assert_eq!(
            csv_line(&[json!(1), json!("plain"), Value::Null]),
            "1,plain,NULL"
        );
        assert_eq!(
            csv_line(&[json!("a,b"), json!("say \"hi\"")]),
            "\"a,b\",\"say \"\"hi\"\"\""
        );
        assert_eq!(csv_line(&[json!("line\nbreak")]), "\"line\nbreak\"");
    }

    #[test]
    fn csv_document_includes_the_header_row() {
        let document = csv_document(
            &["id".to_owned(), "name".to_owned()],
            &[vec![json!(1), json!("Ada")]],
        );
        assert_eq!(document, "id,name\n1,Ada");
    }

    #[test]
    fn csv_document_drops_extra_table_page_values() {
        // PostgreSQL table pages append `__dbm_xmin` after the real columns.
        let document = csv_document(
            &["id".to_owned(), "name".to_owned()],
            &[vec![json!(1), json!("Ada"), json!("728")]],
        );
        assert_eq!(document, "id,name\n1,Ada");
    }

    #[test]
    fn destructive_statements_require_confirmation() {
        assert!(requires_confirmation(
            "DROP TABLE users;",
            DatabaseEngine::Postgres
        ));
        assert!(requires_confirmation(
            "truncate orders",
            DatabaseEngine::Postgres
        ));
        assert!(requires_confirmation(
            "DELETE FROM users",
            DatabaseEngine::Postgres
        ));
        assert!(requires_confirmation(
            "UPDATE users SET active = false",
            DatabaseEngine::Postgres
        ));
        assert!(!requires_confirmation(
            "DELETE FROM users WHERE id = 1",
            DatabaseEngine::Postgres
        ));
        assert!(requires_confirmation(
            // The React UI matches keywords anywhere outside comments,
            // including inside string literals, so a SELECT that mentions one
            // still asks for confirmation.
            "SELECT 'drop table'",
            DatabaseEngine::Postgres
        ));
        assert!(!requires_confirmation(
            "-- drop table users",
            DatabaseEngine::Postgres
        ));
        assert!(!requires_confirmation(
            "/* drop */ SELECT 1",
            DatabaseEngine::Postgres
        ));
        assert!(requires_confirmation("FLUSHALL", DatabaseEngine::Redis));
        assert!(requires_confirmation("  flushdb", DatabaseEngine::Redis));
        assert!(!requires_confirmation(
            "DEL greeting",
            DatabaseEngine::Redis
        ));
    }

    #[test]
    fn switching_engines_only_replaces_preset_values() {
        let mut form = default_profile_input(None);
        form.host = "db.internal".to_owned();
        apply_engine_defaults(&mut form, DatabaseEngine::Mysql);
        assert_eq!(form.name, "Local MySQL");
        assert_eq!(form.port, 3306);
        assert_eq!(form.username, "root");
        assert_eq!(form.default_database, "mysql");
        assert_eq!(form.host, "db.internal");

        form.name = "Production".to_owned();
        apply_engine_defaults(&mut form, DatabaseEngine::Redis);
        assert_eq!(form.name, "Production");
        assert_eq!(form.username, "default");
    }

    #[test]
    fn safe_file_names_drop_path_separators() {
        assert_eq!(safe_file_name("public.users"), "public.users");
        assert_eq!(safe_file_name("a/b:c"), "a_b_c");
    }
}
