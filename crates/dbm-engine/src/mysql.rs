use std::time::Instant;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode,
};
use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{
    Conn, Opts, OptsBuilder, Pool, PoolConstraints, PoolOpts, Row, SslOpts, TxOpts, Value,
};
use serde_json::Value as JsonValue;

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;
const SYSTEM_SCHEMAS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

pub struct MysqlSession {
    profile: ConnectionProfile,
    pool: Pool,
}

impl MysqlSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        let pool = connect_pool(&profile, password.as_deref()).await?;
        Ok(Self { profile, pool })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    pub async fn close(&self) {
        let _ = self.pool.clone().disconnect().await;
    }

    async fn conn(&self) -> AppResult<Conn> {
        self.pool.get_conn().await.map_err(AppError::from)
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn
            .query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| string_cell(&row, 0))
            .map(|name| {
                let system = SYSTEM_SCHEMAS
                    .iter()
                    .any(|schema| schema.eq_ignore_ascii_case(&name));
                DatabaseRef {
                    name,
                    is_template: system,
                    is_connectable: true,
                }
            })
            .collect())
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let mut conn = self.conn().await?;
        let database = current_database(&mut conn, &self.profile.default_database).await?;
        let rows: Vec<Row> = conn
            .exec(
                "SELECT TABLE_NAME, TABLE_TYPE
                 FROM information_schema.TABLES
                 WHERE TABLE_SCHEMA = ?
                 ORDER BY TABLE_NAME",
                (&database,),
            )
            .await?;
        let children = rows
            .into_iter()
            .filter_map(|row| {
                let table = string_cell(&row, 0)?;
                let table_type = string_cell(&row, 1).unwrap_or_default();
                Some(SchemaNode {
                    name: table.clone(),
                    kind: if table_type.eq_ignore_ascii_case("VIEW")
                        || table_type.eq_ignore_ascii_case("SYSTEM VIEW")
                    {
                        "view".into()
                    } else {
                        "table".into()
                    },
                    schema: Some(database.clone()),
                    table: Some(table),
                    children: Vec::new(),
                })
            })
            .collect();
        Ok(vec![SchemaNode {
            name: database.clone(),
            kind: "schema".into(),
            schema: Some(database),
            table: None,
            children,
        }])
    }

    pub async fn table_metadata(&self, schema: &str, table: &str) -> AppResult<TableMetadata> {
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn
            .exec(
                "SELECT ORDINAL_POSITION, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT
                 FROM information_schema.COLUMNS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                 ORDER BY ORDINAL_POSITION",
                (schema, table),
            )
            .await?;
        if rows.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "table {schema}.{table} was not found"
            )));
        }
        let columns = rows
            .into_iter()
            .map(|row| TableColumn {
                ordinal: i32_cell(&row, 0).unwrap_or(0),
                name: string_cell(&row, 1).unwrap_or_default(),
                data_type: string_cell(&row, 2).unwrap_or_else(|| "text".into()),
                nullable: string_cell(&row, 3)
                    .is_some_and(|value| value.eq_ignore_ascii_case("YES")),
                default_value: string_cell(&row, 4),
            })
            .collect::<Vec<_>>();
        let primary_key = conn
            .exec::<Row, _, _>(
                "SELECT COLUMN_NAME
                 FROM information_schema.STATISTICS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
                 ORDER BY SEQ_IN_INDEX",
                (schema, table),
            )
            .await?
            .into_iter()
            .filter_map(|row| string_cell(&row, 0))
            .collect();
        Ok(TableMetadata {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns,
            primary_key,
            has_xmin: false,
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
        let mut conn = self.conn().await?;
        let rows: Vec<Row> = conn.query(sql).await?;
        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        let rows = rows
            .into_iter()
            .take(usize::try_from(limit).unwrap_or_default())
            .map(json_row)
            .collect::<Vec<_>>();
        let total_rows = if request.include_total.unwrap_or(true) {
            let count_sql = format!("SELECT COUNT(*) FROM {table_name}{predicate}");
            conn.query_first::<Value, _>(count_sql)
                .await
                .ok()
                .flatten()
                .and_then(value_as_u64)
        } else {
            None
        };
        let columns = metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
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
        let mut conn = self.conn().await?;
        let mut result = conn.query_iter(sql).await?;
        let column_meta = result.columns().map(|columns| columns.to_vec());
        if let Some(column_meta) = column_meta.filter(|columns| !columns.is_empty()) {
            let columns = column_meta
                .iter()
                .map(|column| QueryColumn {
                    name: column.name_str().into_owned(),
                    data_type: mysql_type_name(column.column_type()),
                })
                .collect::<Vec<_>>();
            let rows: Vec<Row> = result.collect().await?;
            let truncated = rows.len() > usize::try_from(max_rows).unwrap_or(usize::MAX);
            let rows = rows
                .into_iter()
                .take(usize::try_from(max_rows).unwrap_or_default())
                .map(json_row)
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
        let affected_rows = Some(result.affected_rows());
        result.drop_result().await?;
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
        let mut conn = self.conn().await?;
        let mut tx = conn.start_transaction(TxOpts::default()).await?;
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
            let changed = match tx.query_iter(&statement).await {
                Ok(result) => {
                    let changed = result.affected_rows();
                    if let Err(error) = result.drop_result().await {
                        let _ = tx.rollback().await;
                        return Err(error.into());
                    }
                    changed
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(error.into());
                }
            };
            if changed == 0 {
                conflicts.push(mutation.primary_key.clone());
            } else {
                applied += 1;
            }
        }
        tx.commit().await?;
        Ok(MutationResult { applied, conflicts })
    }
}

async fn connect_pool(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Pool> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        let pool = Pool::new(base_opts(profile, password, None));
        probe_pool(&pool).await?;
        return Ok(pool);
    }
    let ssl_opts = Some(ssl_opts(profile)?);
    let pool = Pool::new(base_opts(profile, password, ssl_opts.clone()));
    match probe_pool(&pool).await {
        Ok(()) => Ok(pool),
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            let _ = pool.disconnect().await;
            let fallback = Pool::new(base_opts(profile, password, None));
            probe_pool(&fallback).await?;
            Ok(fallback)
        }
        Err(error) => {
            let _ = pool.disconnect().await;
            Err(error)
        }
    }
}

async fn probe_pool(pool: &Pool) -> AppResult<()> {
    let _conn = pool.get_conn().await?;
    Ok(())
}

fn base_opts(
    profile: &ConnectionProfile,
    password: Option<&str>,
    ssl_opts: Option<SslOpts>,
) -> Opts {
    let constraints = PoolConstraints::new(1, 4).unwrap_or_default();
    OptsBuilder::default()
        .ip_or_hostname(profile.host.clone())
        .tcp_port(profile.port)
        .user(Some(profile.username.clone()))
        .pass(password.map(ToOwned::to_owned))
        .db_name(Some(profile.default_database.clone()))
        .prefer_socket(false)
        .pool_opts(PoolOpts::default().with_constraints(constraints))
        .ssl_opts(ssl_opts)
        .into()
}

fn ssl_opts(profile: &ConnectionProfile) -> AppResult<SslOpts> {
    let mut opts = SslOpts::default();
    if let Some(path) = profile.ca_cert_path.as_deref() {
        opts = opts.with_root_certs(vec![std::path::PathBuf::from(path).into()]);
    } else if matches!(profile.tls_mode, TlsMode::Preferred) {
        opts = opts.with_danger_accept_invalid_certs(true);
    }
    Ok(opts)
}

async fn current_database(conn: &mut Conn, fallback: &str) -> AppResult<String> {
    let value: Option<Value> = conn.query_first("SELECT DATABASE()").await?;
    Ok(value
        .and_then(value_as_string)
        .unwrap_or_else(|| fallback.to_owned()))
}

fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
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
            | "replace"
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
        let text_column = format!("CAST({column} AS CHAR)");
        let value = filter.value.as_deref().unwrap_or_default();
        let clause = match filter.operator {
            FilterOperator::Equals => format!("{column} <=> {}", quote_literal(value)),
            FilterOperator::NotEquals => format!("NOT ({column} <=> {})", quote_literal(value)),
            FilterOperator::Contains => format!(
                "{text_column} LIKE {}",
                quote_literal(&format!("%{value}%"))
            ),
            FilterOperator::StartsWith => {
                format!("{text_column} LIKE {}", quote_literal(&format!("{value}%")))
            }
            FilterOperator::EndsWith => {
                format!("{text_column} LIKE {}", quote_literal(&format!("%{value}")))
            }
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
    Ok(metadata
        .primary_key
        .iter()
        .zip(&mutation.primary_key)
        .map(|(column, value)| format!("{} <=> {}", quote_identifier(column), json_to_sql(value)))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn json_to_sql(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "NULL".into(),
        JsonValue::Bool(value) => {
            if *value {
                "1".into()
            } else {
                "0".into()
            }
        }
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => quote_literal(value),
        JsonValue::Array(_) | JsonValue::Object(_) => quote_literal(&value.to_string()),
    }
}

fn json_row(row: Row) -> Vec<JsonValue> {
    (0..row.len())
        .map(|index| {
            row.as_ref(index)
                .map(value_from_mysql)
                .unwrap_or(JsonValue::Null)
        })
        .collect()
}

fn value_from_mysql(value: &Value) -> JsonValue {
    match value {
        Value::NULL => JsonValue::Null,
        Value::Bytes(bytes) => String::from_utf8(bytes.clone())
            .map(JsonValue::String)
            .unwrap_or_else(|_| JsonValue::String(format!("\\x{}", hex_encode(bytes)))),
        Value::Int(value) => JsonValue::Number(serde_json::Number::from(*value)),
        Value::UInt(value) => JsonValue::Number(serde_json::Number::from(*value)),
        Value::Float(value) => serde_json::Number::from_f64(f64::from(*value))
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Double(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if *hour == 0 && *minute == 0 && *second == 0 && *micros == 0 {
                JsonValue::String(format!("{year:04}-{month:02}-{day:02}"))
            } else if *micros == 0 {
                JsonValue::String(format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
                ))
            } else {
                JsonValue::String(format!(
                    "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"
                ))
            }
        }
        Value::Time(negative, days, hour, minute, second, micros) => {
            let sign = if *negative { "-" } else { "" };
            if *micros == 0 {
                JsonValue::String(format!("{sign}{days} {hour:02}:{minute:02}:{second:02}"))
            } else {
                JsonValue::String(format!(
                    "{sign}{days} {hour:02}:{minute:02}:{second:02}.{micros:06}"
                ))
            }
        }
    }
}

fn value_as_string(value: Value) -> Option<String> {
    match value {
        Value::NULL => None,
        Value::Bytes(bytes) => String::from_utf8(bytes).ok(),
        other => Some(other.as_sql(false).trim_matches('\'').to_owned()),
    }
}

fn value_as_u64(value: Value) -> Option<u64> {
    match value {
        Value::Int(value) => u64::try_from(value).ok(),
        Value::UInt(value) => Some(value),
        Value::Bytes(bytes) => String::from_utf8(bytes).ok()?.parse().ok(),
        _ => None,
    }
}

fn string_cell(row: &Row, index: usize) -> Option<String> {
    row.as_ref(index).cloned().and_then(value_as_string)
}

fn i32_cell(row: &Row, index: usize) -> Option<i32> {
    match row.as_ref(index)? {
        Value::Int(value) => i32::try_from(*value).ok(),
        Value::UInt(value) => i32::try_from(*value).ok(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).parse().ok(),
        _ => None,
    }
}

fn mysql_type_name(column_type: ColumnType) -> String {
    match column_type {
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "decimal".into(),
        ColumnType::MYSQL_TYPE_TINY => "tinyint".into(),
        ColumnType::MYSQL_TYPE_SHORT => "smallint".into(),
        ColumnType::MYSQL_TYPE_LONG => "int".into(),
        ColumnType::MYSQL_TYPE_FLOAT => "float".into(),
        ColumnType::MYSQL_TYPE_DOUBLE => "double".into(),
        ColumnType::MYSQL_TYPE_NULL => "null".into(),
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "timestamp".into(),
        ColumnType::MYSQL_TYPE_LONGLONG => "bigint".into(),
        ColumnType::MYSQL_TYPE_INT24 => "mediumint".into(),
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date".into(),
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "time".into(),
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "datetime".into(),
        ColumnType::MYSQL_TYPE_YEAR => "year".into(),
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => "varchar".into(),
        ColumnType::MYSQL_TYPE_BIT => "bit".into(),
        ColumnType::MYSQL_TYPE_JSON => "json".into(),
        ColumnType::MYSQL_TYPE_TINY_BLOB => "tinyblob".into(),
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => "mediumblob".into(),
        ColumnType::MYSQL_TYPE_LONG_BLOB => "longblob".into(),
        ColumnType::MYSQL_TYPE_BLOB => "blob".into(),
        ColumnType::MYSQL_TYPE_STRING => "char".into(),
        ColumnType::MYSQL_TYPE_GEOMETRY => "geometry".into(),
        other => format!("{other:?}"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_literals_use_mysql_quoting() {
        assert_eq!(quote_identifier("user`name"), "`user``name`");
        assert_eq!(quote_literal("O'Reilly"), "'O''Reilly'");
    }

    #[test]
    fn predicates_use_null_safe_equals_and_like() {
        let metadata = TableMetadata {
            schema: "app".into(),
            table: "users".into(),
            columns: vec![
                TableColumn {
                    name: "id".into(),
                    data_type: "int".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 1,
                },
                TableColumn {
                    name: "email".into(),
                    data_type: "varchar".into(),
                    nullable: false,
                    default_value: None,
                    ordinal: 2,
                },
            ],
            primary_key: vec!["id".into()],
            has_xmin: false,
        };
        let predicate = build_predicate(
            &metadata,
            &[
                FilterCondition {
                    column: "id".into(),
                    operator: FilterOperator::Equals,
                    value: Some("10".into()),
                },
                FilterCondition {
                    column: "email".into(),
                    operator: FilterOperator::Contains,
                    value: Some("ex.com".into()),
                },
            ],
        )
        .expect("predicate");
        assert_eq!(
            predicate,
            " WHERE `id` <=> '10' AND CAST(`email` AS CHAR) LIKE '%ex.com%'"
        );
    }

    #[test]
    fn mutations_match_primary_keys_without_xmin() {
        let metadata = TableMetadata {
            schema: "app".into(),
            table: "users".into(),
            columns: vec![TableColumn {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
                default_value: None,
                ordinal: 1,
            }],
            primary_key: vec!["id".into()],
            has_xmin: false,
        };
        let predicate = mutation_predicate(
            &metadata,
            &RowMutation {
                original: vec![JsonValue::from(1)],
                changes: vec![JsonValue::from(1)],
                primary_key: vec![JsonValue::from(1)],
                xmin: None,
                deleted: false,
            },
        )
        .expect("predicate");
        assert_eq!(predicate, "`id` <=> 1");
    }
}
