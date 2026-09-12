//! Connection URL import, ported from `apps/desktop/ui/src/connectionUrl.ts`.

use dbm_engine::models::{DatabaseEngine, TlsMode};
use percent_encoding::percent_decode_str;
use url::Url;

use crate::format::preset;

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedConnection {
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    pub tls_mode: TlsMode,
    pub password: Option<String>,
    pub suggested_name: String,
}

pub fn parse_connection_url(value: &str) -> Result<ImportedConnection, String> {
    let trimmed = value.trim();
    if !trimmed.contains("://") {
        return Err("Enter a valid connection URL.".to_owned());
    }
    let url = Url::parse(trimmed).map_err(|_| "Enter a valid connection URL.".to_owned())?;
    let engine = engine_from_scheme(url.scheme()).ok_or_else(|| {
        "The connection URL must begin with postgres://, postgresql://, mysql://, mariadb://, redis://, rediss://, valkey://, or valkeys://.".to_owned()
    })?;

    let defaults = preset(engine);
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let username = decode(url.username(), "username")?;
    if host.is_empty() {
        return Err("The connection URL must include a host.".to_owned());
    }
    if engine != DatabaseEngine::Redis && username.is_empty() {
        return Err("The connection URL must include a host and username.".to_owned());
    }

    let port = url.port().unwrap_or(defaults.port);
    if port == 0 {
        return Err("The connection URL contains an invalid port.".to_owned());
    }

    let database = decode(url.path().trim_start_matches('/'), "database")?;
    let default_database = if database.is_empty() {
        defaults.default_database.to_owned()
    } else {
        database
    };
    let password = match url.password() {
        Some(password) if !password.is_empty() => Some(decode(password, "password")?),
        _ => None,
    };

    Ok(ImportedConnection {
        engine,
        host: host.clone(),
        port,
        username,
        default_database: default_database.clone(),
        tls_mode: tls_mode_from_url(&url),
        password,
        suggested_name: format!("{default_database} @ {host}"),
    })
}

fn engine_from_scheme(scheme: &str) -> Option<DatabaseEngine> {
    match scheme {
        "postgres" | "postgresql" => Some(DatabaseEngine::Postgres),
        "mysql" | "mariadb" => Some(DatabaseEngine::Mysql),
        "redis" | "rediss" | "valkey" | "valkeys" => Some(DatabaseEngine::Redis),
        _ => None,
    }
}

fn tls_mode_from_url(url: &Url) -> TlsMode {
    if url.scheme() == "rediss" || url.scheme() == "valkeys" {
        return TlsMode::Required;
    }
    let ssl_mode = url
        .query_pairs()
        .find(|(key, _)| key == "sslmode" || key == "ssl-mode" || key == "sslMode")
        .map(|(_, value)| value.to_lowercase());
    if let Some(mode) = ssl_mode {
        if mode == "disable" || mode == "disabled" {
            return TlsMode::Disabled;
        }
        if matches!(
            mode.as_str(),
            "require"
                | "required"
                | "verify_ca"
                | "verify-ca"
                | "verify_identity"
                | "verify-identity"
                | "verify-full"
        ) {
            return TlsMode::Required;
        }
    }
    if url
        .query_pairs()
        .any(|(key, value)| key == "ssl" && value == "true")
    {
        return TlsMode::Required;
    }
    TlsMode::Preferred
}

fn decode(value: &str, field: &str) -> Result<String, String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|_| format!("The connection URL contains an invalid {field}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_railway_style_postgres_urls() {
        let imported = parse_connection_url(
            "postgresql://postgres:PASSWORD@caboose.proxy.rlwy.net:57394/railway",
        )
        .expect("import");
        assert_eq!(
            imported,
            ImportedConnection {
                engine: DatabaseEngine::Postgres,
                host: "caboose.proxy.rlwy.net".to_owned(),
                port: 57394,
                username: "postgres".to_owned(),
                default_database: "railway".to_owned(),
                tls_mode: TlsMode::Preferred,
                password: Some("PASSWORD".to_owned()),
                suggested_name: "railway @ caboose.proxy.rlwy.net".to_owned(),
            }
        );
    }

    #[test]
    fn decodes_credentials_and_maps_sslmode() {
        let imported = parse_connection_url(
            "postgres://user%40example.com:p%40ss@localhost/my%20db?sslmode=require",
        )
        .expect("import");
        assert_eq!(imported.username, "user@example.com");
        assert_eq!(imported.password.as_deref(), Some("p@ss"));
        assert_eq!(imported.default_database, "my db");
        assert_eq!(imported.tls_mode, TlsMode::Required);
    }

    #[test]
    fn imports_mysql_and_mariadb_urls() {
        let imported = parse_connection_url(
            "mysql://root:secret@caboose.proxy.rlwy.net:3306/railway?ssl-mode=REQUIRED",
        )
        .expect("import");
        assert_eq!(imported.engine, DatabaseEngine::Mysql);
        assert_eq!(imported.port, 3306);
        assert_eq!(imported.tls_mode, TlsMode::Required);

        let imported = parse_connection_url("mariadb://app@localhost/analytics").expect("import");
        assert_eq!(imported.engine, DatabaseEngine::Mysql);
        assert_eq!(imported.port, 3306);
        assert_eq!(imported.username, "app");
        assert_eq!(imported.default_database, "analytics");
        assert_eq!(imported.tls_mode, TlsMode::Preferred);
    }

    #[test]
    fn imports_redis_urls_including_tls_and_password_only_auth() {
        let imported = parse_connection_url("redis://:secret@localhost:6379/2").expect("import");
        assert_eq!(
            imported,
            ImportedConnection {
                engine: DatabaseEngine::Redis,
                host: "localhost".to_owned(),
                port: 6379,
                username: String::new(),
                default_database: "2".to_owned(),
                tls_mode: TlsMode::Preferred,
                password: Some("secret".to_owned()),
                suggested_name: "2 @ localhost".to_owned(),
            }
        );

        let imported =
            parse_connection_url("rediss://default:p@ss@cache.example:6380/0").expect("import");
        assert_eq!(imported.engine, DatabaseEngine::Redis);
        assert_eq!(imported.host, "cache.example");
        assert_eq!(imported.port, 6380);
        assert_eq!(imported.username, "default");
        assert_eq!(imported.password.as_deref(), Some("p@ss"));
        assert_eq!(imported.default_database, "0");
        assert_eq!(imported.tls_mode, TlsMode::Required);
    }

    #[test]
    fn rejects_unsupported_protocols_and_garbage() {
        assert_eq!(
            parse_connection_url("mongodb://localhost:27017").unwrap_err(),
            "The connection URL must begin with postgres://, postgresql://, mysql://, mariadb://, redis://, rediss://, valkey://, or valkeys://."
        );
        assert_eq!(
            parse_connection_url("not a url").unwrap_err(),
            "Enter a valid connection URL."
        );
    }
}
