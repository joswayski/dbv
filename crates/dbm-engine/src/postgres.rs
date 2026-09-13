use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::Value;
use std::error::Error;
use tokio_postgres::types::FromSql;
use tokio_postgres::{Client, Config, NoTls, Row, types::Type};
use uuid::Uuid;

use crate::util::hex_encode;

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;

#[derive(Debug)]
pub struct PgSession {
    profile: ConnectionProfile,
    client: Client,
}

impl PgSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        let client = connect_client(&profile, password.as_deref()).await?;
        Ok(Self { profile, client })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let rows = self
            .client
            .query(
                "SELECT datname, datistemplate, datallowconn
                 FROM pg_database ORDER BY datname",
                &[],
            )
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DatabaseRef {
                    name: row.try_get(0)?,
                    is_template: row.try_get(1)?,
                    is_connectable: row.try_get(2)?,
                })
            })
            .collect()
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let schema_rows = self
            .client
            .query(
                "SELECT schema_name
                 FROM information_schema.schemata
                 WHERE schema_name NOT LIKE 'pg_%'
                   AND schema_name <> 'information_schema'
                 ORDER BY schema_name",
                &[],
            )
            .await?;
        let table_rows = self
            .client
            .query(
                "SELECT table_schema, table_name, table_type
                 FROM information_schema.tables
                 WHERE table_schema NOT LIKE 'pg_%'
                   AND table_schema <> 'information_schema'
                 ORDER BY table_schema, table_name",
                &[],
            )
            .await?;

        let mut schemas = schema_rows
            .into_iter()
            .map(|row| {
                let name: String = row.try_get(0)?;
                Ok(SchemaNode {
                    name: name.clone(),
                    kind: "schema".into(),
                    schema: Some(name),
                    table: None,
                    children: Vec::new(),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        for row in table_rows {
            let schema: String = row.try_get(0)?;
            let table: String = row.try_get(1)?;
            let table_type: String = row.try_get(2)?;
            if let Some(schema_node) = schemas
                .iter_mut()
                .find(|node| node.schema.as_deref() == Some(schema.as_str()))
            {
                schema_node.children.push(SchemaNode {
                    name: table.clone(),
                    kind: if table_type == "VIEW" {
                        "view".into()
                    } else {
                        "table".into()
                    },
                    schema: Some(schema),
                    table: Some(table),
                    children: Vec::new(),
                });
            }
        }
        Ok(schemas)
    }

    pub async fn table_metadata(&self, schema: &str, table: &str) -> AppResult<TableMetadata> {
        let rows = self
            .client
            .query(
                "SELECT ordinal_position, column_name, data_type, is_nullable, column_default
                 FROM information_schema.columns
                 WHERE table_schema = $1 AND table_name = $2
                 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .await?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "table {schema}.{table} was not found"
            )));
        }
        let columns = rows
            .into_iter()
            .map(|row| {
                Ok(TableColumn {
                    ordinal: row.try_get::<_, i32>(0)?,
                    name: row.try_get(1)?,
                    data_type: row.try_get(2)?,
                    nullable: row.try_get::<_, String>(3)? == "YES",
                    default_value: row.try_get(4)?,
                })
            })
            .collect::<Result<Vec<_>, tokio_postgres::Error>>()?;
        let primary_key = self
            .client
            .query(
                "SELECT a.attname
                 FROM pg_index i
                 JOIN pg_class c ON c.oid = i.indrelid
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey)
                 WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
                 ORDER BY a.attnum",
                &[&schema, &table],
            )
            .await?
            .into_iter()
            .map(|row| row.try_get(0))
            .collect::<Result<Vec<String>, _>>()?;
        Ok(TableMetadata {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            primary_key,
            has_xmin: true,
        })
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let metadata = self.table_metadata(&request.schema, &request.table).await?;
        let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
        let offset = request.offset;
        let selected_columns = metadata
            .columns
            .iter()
            .map(|column| quote_identifier(&column.name))
            .chain(std::iter::once("xmin::text AS \"__dbm_xmin\"".to_owned()))
            .collect::<Vec<_>>();
        let table_name = qualified_name(&request.schema, &request.table)?;
        let predicate = build_predicate(&metadata, &request.filters)?;
        let order_by = build_order_by(&metadata, request.order_by.as_ref())?;
        let sql = format!(
            "SELECT {} FROM {table_name}{predicate}{order_by} LIMIT {} OFFSET {}",
            selected_columns.join(", "),
            limit + 1,
            offset
        );
        let rows = self.client.query(&sql, &[]).await?;
        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let rows = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or_default())
            .map(|row| {
                (0..row.len())
                    .map(|index| value_from_row(&row, index))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let total_rows = if request.include_total.unwrap_or(true) {
            let count_sql = format!("SELECT COUNT(*)::bigint FROM {table_name}{predicate}");
            self.client
                .query_one(&count_sql, &[])
                .await
                .ok()
                .and_then(|row| row.try_get::<_, i64>(0).ok())
                .and_then(|count| u64::try_from(count).ok())
        } else {
            None
        };
        let columns = metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
            .chain(std::iter::once("__dbm_xmin".into()))
            .collect();
        Ok(TablePage {
            metadata,
            columns,
            rows,
            total_rows,
            offset,
            limit,
            has_more,
        })
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        let sql = sql.trim();
        if sql.is_empty() {
            return Err(AppError::InvalidInput("query is empty".into()));
        }
        if self.profile.read_only && is_mutating_statement(sql) {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let started = Instant::now();
        let max_rows = max_rows
            .unwrap_or(DEFAULT_QUERY_ROWS)
            .clamp(1, DEFAULT_QUERY_ROWS);
        let keyword = sql
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let normalized_sql = sql.to_ascii_lowercase();
        let row_query = matches!(
            keyword.as_str(),
            "select" | "with" | "show" | "values" | "explain" | "describe" | "desc"
        ) || normalized_sql.contains(" returning ");
        if row_query {
            let rows = self.client.query(sql, &[]).await?;
            let truncated = rows.len() > usize::try_from(max_rows).unwrap_or(usize::MAX);
            let columns = rows
                .first()
                .map(|row| {
                    row.columns()
                        .iter()
                        .map(|column| QueryColumn {
                            name: column.name().to_owned(),
                            data_type: column.type_().name().to_owned(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let rows = rows
                .into_iter()
                .take(usize::try_from(max_rows).unwrap_or_default())
                .map(|row| {
                    (0..row.len())
                        .map(|index| value_from_row(&row, index))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            return Ok(QueryResponse {
                columns,
                row_count: rows.len(),
                rows,
                affected_rows: None,
                duration_ms: started.elapsed().as_millis(),
                truncated,
                notices: Vec::new(),
            });
        }
        let affected_rows = if sql.contains(';') {
            self.client.batch_execute(sql).await?;
            None
        } else {
            Some(self.client.execute(sql, &[]).await?)
        };
        Ok(QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows,
            duration_ms: started.elapsed().as_millis(),
            truncated: false,
            notices: Vec::new(),
        })
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        if self.profile.read_only {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let metadata = self.table_metadata(&batch.schema, &batch.table).await?;
        if metadata.primary_key.is_empty() {
            return Err(AppError::Unsupported(
                "only tables with a primary key can be edited".into(),
            ));
        }
        let table_name = qualified_name(&batch.schema, &batch.table)?;
        self.client.batch_execute("BEGIN").await?;
        let mut applied = 0;
        let mut conflicts = Vec::new();
        for mutation in &batch.mutations {
            let predicate = mutation_predicate(&metadata, mutation)?;
            let statement = if mutation.deleted {
                format!("DELETE FROM {table_name} WHERE {predicate}")
            } else {
                let assignments = metadata
                    .columns
                    .iter()
                    .enumerate()
                    .filter_map(|(index, column)| {
                        if metadata.primary_key.iter().any(|key| key == &column.name)
                            || mutation.original.get(index) == mutation.changes.get(index)
                        {
                            None
                        } else {
                            mutation.changes.get(index).map(|value| {
                                format!(
                                    "{} = {}",
                                    quote_identifier(&column.name),
                                    json_to_sql(value)
                                )
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                if assignments.is_empty() {
                    continue;
                }
                format!(
                    "UPDATE {table_name} SET {} WHERE {predicate}",
                    assignments.join(", ")
                )
            };
            let changed = match self.client.execute(&statement, &[]).await {
                Ok(changed) => changed,
                Err(error) => {
                    let _ = self.client.batch_execute("ROLLBACK").await;
                    return Err(error.into());
                }
            };
            if changed == 0 {
                conflicts.push(mutation.primary_key.clone());
            } else {
                applied += 1;
            }
        }
        if let Err(error) = self.client.batch_execute("COMMIT").await {
            let _ = self.client.batch_execute("ROLLBACK").await;
            return Err(error.into());
        }
        Ok(MutationResult { applied, conflicts })
    }
}

async fn connect_client(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Client> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        return connect_without_tls(profile, password).await;
    }
    let mut connector = native_tls::TlsConnector::builder();
    if let Some(path) = profile.ca_cert_path.as_deref() {
        let pem = std::fs::read(path).map_err(|error| AppError::Credential(error.to_string()))?;
        let certificate = native_tls::Certificate::from_pem(&pem)
            .map_err(|error| AppError::Credential(error.to_string()))?;
        connector.add_root_certificate(certificate);
    }
    let connector = connector
        .build()
        .map_err(|error| AppError::database(&error))?;
    let mut config = base_config(profile, password);
    let make_connector = postgres_native_tls::MakeTlsConnector::new(connector);
    match config.connect(make_connector).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(%error, "postgres connection closed");
                }
            });
            Ok(client)
        }
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            config = base_config(profile, password);
            let (client, connection) = config.connect(NoTls).await?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    tracing::error!(%error, "postgres connection closed");
                }
            });
            Ok(client)
        }
        Err(error) => Err(AppError::from(error)),
    }
}

async fn connect_without_tls(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<Client> {
    let (client, connection) = base_config(profile, password).connect(NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "postgres connection closed");
        }
    });
    Ok(client)
}

fn base_config(profile: &ConnectionProfile, password: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host(&profile.host)
        .port(profile.port)
        .user(&profile.username)
        .dbname(&profile.default_database);
    if let Some(password) = password {
        config.password(password);
    }
    config
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_mutating_statement(sql: &str) -> bool {
    matches!(
        sql.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "insert"
            | "update"
            | "delete"
            | "alter"
            | "drop"
            | "truncate"
            | "create"
            | "grant"
            | "revoke"
    )
}

fn qualified_name(schema: &str, table: &str) -> AppResult<String> {
    if schema.contains('\0') || table.contains('\0') {
        return Err(AppError::InvalidInput(
            "identifier contains a NUL byte".into(),
        ));
    }
    Ok(format!(
        "{}.{}",
        quote_identifier(schema),
        quote_identifier(table)
    ))
}

fn build_predicate(metadata: &TableMetadata, filters: &[FilterCondition]) -> AppResult<String> {
    if filters.is_empty() {
        return Ok(String::new());
    }
    let mut clauses = Vec::with_capacity(filters.len());
    for filter in filters {
        if !metadata
            .columns
            .iter()
            .any(|column| column.name == filter.column)
        {
            return Err(AppError::InvalidInput(format!(
                "unknown filter column {}",
                filter.column
            )));
        }
        let column = quote_identifier(&filter.column);
        let text_column = format!("{column}::text");
        let value = filter.value.as_deref().unwrap_or_default();
        let clause = match filter.operator {
            FilterOperator::Equals => {
                format!("{column} IS NOT DISTINCT FROM {}", quote_literal(value))
            }
            FilterOperator::NotEquals => {
                format!("{column} IS DISTINCT FROM {}", quote_literal(value))
            }
            FilterOperator::Contains => format!(
                "{text_column} ILIKE {}",
                quote_literal(&format!("%{value}%"))
            ),
            FilterOperator::StartsWith => format!(
                "{text_column} ILIKE {}",
                quote_literal(&format!("{value}%"))
            ),
            FilterOperator::EndsWith => format!(
                "{text_column} ILIKE {}",
                quote_literal(&format!("%{value}"))
            ),
            FilterOperator::GreaterThan => format!("{column} > {}", quote_literal(value)),
            FilterOperator::GreaterThanOrEqual => {
                format!("{column} >= {}", quote_literal(value))
            }
            FilterOperator::LessThan => format!("{column} < {}", quote_literal(value)),
            FilterOperator::LessThanOrEqual => {
                format!("{column} <= {}", quote_literal(value))
            }
            FilterOperator::In => format!("{column} IN ({})", filter_list(value)?),
            FilterOperator::NotIn => format!("{column} NOT IN ({})", filter_list(value)?),
            FilterOperator::IsNull => format!("{column} IS NULL"),
            FilterOperator::IsNotNull => format!("{column} IS NOT NULL"),
        };
        clauses.push(clause);
    }
    Ok(format!(" WHERE {}", clauses.join(" AND ")))
}

fn build_order_by(metadata: &TableMetadata, order_by: Option<&OrderSpec>) -> AppResult<String> {
    let mut clauses = Vec::new();
    if let Some(order_by) = order_by {
        if !metadata
            .columns
            .iter()
            .any(|column| column.name == order_by.column)
        {
            return Err(AppError::InvalidInput(format!(
                "unknown order column {}",
                order_by.column
            )));
        }
        clauses.push(format!(
            "{} {}",
            quote_identifier(&order_by.column),
            if order_by.descending { "DESC" } else { "ASC" }
        ));
    }
    clauses.extend(
        metadata
            .primary_key
            .iter()
            .filter(|column| match order_by {
                Some(order) => order.column.as_str() != column.as_str(),
                None => true,
            })
            .map(|column| format!("{} ASC", quote_identifier(column))),
    );
    if clauses.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(" ORDER BY {}", clauses.join(", ")))
    }
}

fn filter_list(value: &str) -> AppResult<String> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(quote_literal)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(AppError::InvalidInput(
            "IN filters require one or more comma-separated values".into(),
        ));
    }
    Ok(values.join(", "))
}

fn mutation_predicate(metadata: &TableMetadata, mutation: &RowMutation) -> AppResult<String> {
    if mutation.primary_key.len() != metadata.primary_key.len() {
        return Err(AppError::InvalidInput(
            "primary key values do not match table".into(),
        ));
    }
    let mut clauses = metadata
        .primary_key
        .iter()
        .zip(&mutation.primary_key)
        .map(|(column, value)| {
            format!(
                "{} IS NOT DISTINCT FROM {}",
                quote_identifier(column),
                json_to_sql(value)
            )
        })
        .collect::<Vec<_>>();
    if let Some(xmin) = mutation.xmin.as_deref() {
        clauses.push(format!("xmin::text = {}", quote_literal(xmin)));
    }
    Ok(clauses.join(" AND "))
}

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn json_to_sql(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Bool(value) => value.to_string().to_ascii_uppercase(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => quote_literal(value),
        Value::Array(_) | Value::Object(_) => quote_literal(&value.to_string()),
    }
}

fn value_from_row(row: &Row, index: usize) -> Value {
    let ty = row.columns()[index].type_();
    if *ty == Type::BOOL {
        return row
            .try_get::<_, bool>(index)
            .map(Value::Bool)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT2 {
        return row
            .try_get::<_, i16>(index)
            .map(|value| Value::Number(serde_json::Number::from(i64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT4 {
        return row
            .try_get::<_, i32>(index)
            .map(|value| Value::Number(serde_json::Number::from(i64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::INT8 {
        return row
            .try_get::<_, i64>(index)
            .map(|value| Value::Number(serde_json::Number::from(value)))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::OID {
        return row
            .try_get::<_, u32>(index)
            .map(|value| Value::Number(serde_json::Number::from(u64::from(value))))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT4 {
        return row
            .try_get::<_, f32>(index)
            .ok()
            .and_then(|value| serde_json::Number::from_f64(f64::from(value)))
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::FLOAT8 {
        return row
            .try_get::<_, f64>(index)
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if *ty == Type::NUMERIC {
        // `numeric` has no decoder in the tokio-postgres feature set this
        // workspace resolves, so DBM reads the binary format itself. Keeping
        // the declared scale matters: 7.00 must not render as 7.
        return row
            .try_get::<_, PgNumeric>(index)
            .map(|value| Value::String(value.as_str().to_owned()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::UUID {
        return row
            .try_get::<_, Uuid>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::BYTEA {
        return row
            .try_get::<_, Vec<u8>>(index)
            .map(|bytes| Value::String(format!("\\x{}", hex_encode(&bytes))))
            .unwrap_or(Value::Null);
    }
    if [Type::JSON, Type::JSONB].contains(ty) {
        return row.try_get::<_, Value>(index).unwrap_or(Value::Null);
    }
    if *ty == Type::DATE {
        return row
            .try_get::<_, NaiveDate>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIME {
        return row
            .try_get::<_, NaiveTime>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMP {
        return row
            .try_get::<_, NaiveDateTime>(index)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null);
    }
    if *ty == Type::TIMESTAMPTZ {
        return row
            .try_get::<_, DateTime<Utc>>(index)
            .map(|value| Value::String(value.to_rfc3339()))
            .unwrap_or(Value::Null);
    }
    row.try_get::<_, Option<String>>(index)
        .ok()
        .flatten()
        .map(Value::String)
        .unwrap_or(Value::Null)
}

/// A PostgreSQL `numeric`/`decimal` value.
///
/// tokio-postgres has no numeric decoder in the feature set this workspace
/// resolves, so DBM reads the documented binary format and renders the value
/// as text with the column's declared scale (`7.00`, not `7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PgNumeric(String);

impl PgNumeric {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromSql<'_> for PgNumeric {
    fn from_sql(_ty: &Type, raw: &[u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        decode_numeric(raw)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::NUMERIC)
    }
}

fn decode_numeric(raw: &[u8]) -> Result<PgNumeric, Box<dyn Error + Sync + Send>> {
    if raw.len() < 8 {
        return Err("numeric value is shorter than its header".into());
    }
    let ndigits = i16::from_be_bytes([raw[0], raw[1]]);
    let weight = i16::from_be_bytes([raw[2], raw[3]]);
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let scale = usize::from(u16::from_be_bytes([raw[6], raw[7]]));
    match sign {
        0xC000 => return Ok(PgNumeric("NaN".into())),
        0xD000 => return Ok(PgNumeric("Infinity".into())),
        0xF000 => return Ok(PgNumeric("-Infinity".into())),
        _ => {}
    }
    if ndigits < 0 || raw.len() < 8 + usize::from(ndigits as u16) * 2 {
        return Err("numeric value is shorter than its digits".into());
    }
    let digits: Vec<u16> = (0..ndigits as usize)
        .map(|index| u16::from_be_bytes([raw[8 + index * 2], raw[9 + index * 2]]))
        .collect();

    let mut rendered = String::new();
    if sign == 0x4000 {
        rendered.push('-');
    }
    // Digits are base 10000, and `weight` is the index of the group just left
    // of the decimal point, so the integer part is `digits[0..=weight]`.
    if weight < 0 {
        rendered.push('0');
    } else {
        for index in 0..=weight as usize {
            let digit = digits.get(index).copied().unwrap_or(0);
            if index == 0 {
                rendered.push_str(&digit.to_string());
            } else {
                rendered.push_str(&format!("{digit:04}"));
            }
        }
    }
    if scale > 0 {
        let mut fraction = String::new();
        for digit in digits.iter().skip((weight + 1).max(0) as usize) {
            fraction.push_str(&format!("{digit:04}"));
        }
        while fraction.len() < scale {
            fraction.push('0');
        }
        fraction.truncate(scale);
        rendered.push('.');
        rendered.push_str(&fraction);
    }
    Ok(PgNumeric(rendered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_literals_are_quoted() {
        assert_eq!(quote_identifier("user\"name"), "\"user\"\"name\"");
        assert_eq!(quote_literal("O'Reilly"), "'O''Reilly'");
    }

    #[test]
    fn predicates_only_accept_known_columns() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![TableColumn {
                name: "name".into(),
                data_type: "text".into(),
                nullable: true,
                default_value: None,
                ordinal: 1,
            }],
            primary_key: Vec::new(),
            has_xmin: true,
        };
        let result = build_predicate(
            &metadata,
            &[FilterCondition {
                column: "missing".into(),
                operator: FilterOperator::Equals,
                value: Some("x".into()),
            }],
        );
        assert!(result.is_err());
    }

    #[test]
    fn predicates_support_comparisons_and_lists() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "status".into(),
                    data_type: "text".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: true,
        };
        let predicate = build_predicate(
            &metadata,
            &[
                FilterCondition {
                    column: "id".into(),
                    operator: FilterOperator::GreaterThanOrEqual,
                    value: Some("10".into()),
                },
                FilterCondition {
                    column: "status".into(),
                    operator: FilterOperator::In,
                    value: Some("active, pending".into()),
                },
            ],
        )
        .expect("predicate");

        assert_eq!(
            predicate,
            " WHERE \"id\" >= '10' AND \"status\" IN ('active', 'pending')"
        );
    }

    #[test]
    fn ordering_uses_primary_keys_as_stable_tiebreakers() {
        let metadata = TableMetadata {
            schema: "public".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "status".into(),
                    data_type: "text".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: true,
        };
        let order = build_order_by(
            &metadata,
            Some(&OrderSpec {
                column: "status".into(),
                descending: true,
            }),
        )
        .expect("order");

        assert_eq!(order, " ORDER BY \"status\" DESC, \"id\" ASC");
    }

    #[test]
    fn read_only_detection_covers_common_writes() {
        assert!(is_mutating_statement("DELETE FROM users"));
        assert!(is_mutating_statement(
            "ALTER TABLE users ADD COLUMN note text"
        ));
        assert!(!is_mutating_statement("SELECT * FROM users"));
    }

    fn numeric_bytes(ndigits: i16, weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&ndigits.to_be_bytes());
        raw.extend_from_slice(&weight.to_be_bytes());
        raw.extend_from_slice(&sign.to_be_bytes());
        raw.extend_from_slice(&scale.to_be_bytes());
        for digit in digits {
            raw.extend_from_slice(&digit.to_be_bytes());
        }
        raw
    }

    #[test]
    fn numeric_values_keep_their_declared_scale() {
        let cases = [
            (numeric_bytes(1, 0, 0, 2, &[7]), "7.00"),
            (numeric_bytes(1, -1, 0x4000, 2, &[500]), "-0.05"),
            (numeric_bytes(3, 1, 0, 3, &[1, 2345, 6780]), "12345.678"),
            (numeric_bytes(0, 0, 0, 0, &[]), "0"),
            (numeric_bytes(0, 0, 0, 2, &[]), "0.00"),
            (numeric_bytes(1, 4, 0, 0, &[10]), "100000000000000000"),
            (numeric_bytes(3, 1, 0, 1, &[10, 0, 5000]), "100000.5"),
            (numeric_bytes(0, 0, 0xC000, 0, &[]), "NaN"),
        ];
        for (raw, expected) in cases {
            assert_eq!(decode_numeric(&raw).expect("numeric").as_str(), expected);
        }
    }

    #[test]
    fn truncated_numeric_values_are_rejected() {
        assert!(decode_numeric(&[0, 1]).is_err());
        assert!(decode_numeric(&numeric_bytes(2, 0, 0, 0, &[1])).is_err());
    }
}
