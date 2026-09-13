use std::cmp::Ordering;
use std::time::Instant;

use redis_rs::aio::MultiplexedConnection;
use redis_rs::{Client, Cmd, ConnectionAddr, ConnectionInfo, RedisConnectionInfo, Value};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseRef, FilterCondition, FilterOperator, MutationBatch, MutationResult,
    OrderSpec, QueryColumn, QueryResponse, RowMutation, SchemaNode, TableColumn, TableMetadata,
    TablePage, TablePageRequest, TlsMode, parse_redis_database,
};

const MAX_PAGE_SIZE: u32 = 1_000;
const DEFAULT_QUERY_ROWS: u32 = 10_000;
const TREE_KEYS_PER_TYPE: usize = 250;
const KEY_LIST_SCAN_CAP: usize = 5_000;
const MAX_VALUE_BYTES: usize = 256 * 1024;
const KEY_TYPES: [&str; 6] = ["string", "hash", "list", "set", "zset", "stream"];

pub struct RedisSession {
    profile: ConnectionProfile,
    connection: Mutex<MultiplexedConnection>,
}

impl RedisSession {
    pub async fn connect(profile: ConnectionProfile, password: Option<String>) -> AppResult<Self> {
        if profile.ssh.is_some() {
            return Err(AppError::Unsupported(
                "SSH tunneling is not supported for this connection".into(),
            ));
        }
        let connection = connect_client(&profile, password.as_deref()).await?;
        Ok(Self {
            profile,
            connection: Mutex::new(connection),
        })
    }

    pub fn profile(&self) -> &ConnectionProfile {
        &self.profile
    }

    async fn conn(&self) -> tokio::sync::MutexGuard<'_, MultiplexedConnection> {
        self.connection.lock().await
    }

    pub async fn list_databases(&self) -> AppResult<Vec<DatabaseRef>> {
        let mut conn = self.conn().await;
        if cluster_enabled(&mut conn).await? {
            return Ok(vec![DatabaseRef {
                name: "0".into(),
                is_template: false,
                is_connectable: true,
            }]);
        }
        let count = database_count(&mut conn).await?;
        Ok((0..count)
            .map(|index| DatabaseRef {
                name: index.to_string(),
                is_template: false,
                is_connectable: true,
            })
            .collect())
    }

    pub async fn schema_tree(&self) -> AppResult<Vec<SchemaNode>> {
        let mut conn = self.conn().await;
        let mut nodes = vec![SchemaNode {
            name: "Keys".into(),
            kind: "schema".into(),
            schema: Some("keys".into()),
            table: None,
            children: vec![SchemaNode {
                name: "all".into(),
                kind: "table".into(),
                schema: Some("keys".into()),
                table: Some("all".into()),
                children: Vec::new(),
            }],
        }];
        for key_type in KEY_TYPES {
            let mut keys = scan_keys(&mut conn, "*", Some(key_type), TREE_KEYS_PER_TYPE).await?;
            keys.sort();
            if keys.is_empty() {
                continue;
            }
            nodes.push(SchemaNode {
                name: type_label(key_type).into(),
                kind: "schema".into(),
                schema: Some(key_type.into()),
                table: None,
                children: keys
                    .into_iter()
                    .map(|key| SchemaNode {
                        name: key.clone(),
                        kind: "key".into(),
                        schema: Some(key_type.into()),
                        table: Some(key),
                        children: Vec::new(),
                    })
                    .collect(),
            });
        }
        Ok(nodes)
    }

    pub async fn table_page(&self, request: &TablePageRequest) -> AppResult<TablePage> {
        let mut conn = self.conn().await;
        if request.schema == "keys" && request.table == "all" {
            return keys_table_page(&mut conn, request).await;
        }
        key_table_page(&mut conn, request).await
    }

    pub async fn run_query(&self, sql: &str, max_rows: Option<u32>) -> AppResult<QueryResponse> {
        let sql = sql.trim();
        if sql.is_empty() {
            return Err(AppError::InvalidInput("query is empty".into()));
        }
        let argv = tokenize_redis_cli(sql)?;
        let command = argv[0].to_ascii_lowercase();
        if is_unsupported_command(&command, &argv) {
            return Err(AppError::Unsupported(format!(
                "{command} is not supported in the Redis workbench"
            )));
        }
        if self.profile.read_only && !is_read_command(&argv) {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let max_rows = max_rows
            .unwrap_or(DEFAULT_QUERY_ROWS)
            .clamp(1, DEFAULT_QUERY_ROWS);
        let mut cmd = Cmd::new();
        for arg in &argv {
            cmd.arg(arg);
        }
        let started = Instant::now();
        let mut conn = self.conn().await;
        let value: Value = cmd.query_async(&mut *conn).await?;
        Ok(value_to_query_response(
            &argv,
            value,
            usize::try_from(max_rows).unwrap_or(usize::MAX),
            started.elapsed().as_millis(),
        ))
    }

    pub async fn apply_mutations(&self, batch: &MutationBatch) -> AppResult<MutationResult> {
        if self.profile.read_only {
            return Err(AppError::Unsupported("profile is read-only".into()));
        }
        let mut conn = self.conn().await;
        if batch.schema == "keys" && batch.table == "all" {
            return apply_key_index_mutations(&mut conn, batch).await;
        }
        apply_key_mutations(&mut conn, batch).await
    }
}

async fn connect_client(
    profile: &ConnectionProfile,
    password: Option<&str>,
) -> AppResult<MultiplexedConnection> {
    if matches!(profile.tls_mode, TlsMode::Disabled) {
        return open_connection(profile, password, false).await;
    }
    match open_connection(profile, password, true).await {
        Ok(connection) => Ok(connection),
        Err(_error) if matches!(profile.tls_mode, TlsMode::Preferred) => {
            open_connection(profile, password, false).await
        }
        Err(error) => Err(error),
    }
}

async fn open_connection(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tls: bool,
) -> AppResult<MultiplexedConnection> {
    let client = Client::open(connection_info(profile, password, tls)?)?;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: String = redis_rs::cmd("PING").query_async(&mut connection).await?;
    Ok(connection)
}

fn connection_info(
    profile: &ConnectionProfile,
    password: Option<&str>,
    tls: bool,
) -> AppResult<ConnectionInfo> {
    let db = parse_redis_database(&profile.default_database)?;
    let addr = if tls {
        ConnectionAddr::TcpTls {
            host: profile.host.clone(),
            port: profile.port,
            insecure: matches!(profile.tls_mode, TlsMode::Preferred)
                && profile.ca_cert_path.is_none(),
            tls_params: None,
        }
    } else {
        ConnectionAddr::Tcp(profile.host.clone(), profile.port)
    };
    let mut redis = RedisConnectionInfo {
        db,
        username: None,
        password: password
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        protocol: redis_rs::ProtocolVersion::RESP2,
    };
    let username = profile.username.trim();
    if !username.is_empty() && !username.eq_ignore_ascii_case("default") {
        redis.username = Some(username.to_owned());
    }
    Ok(ConnectionInfo { addr, redis })
}

async fn cluster_enabled(conn: &mut MultiplexedConnection) -> AppResult<bool> {
    let info: Result<String, _> = redis_rs::cmd("INFO").arg("cluster").query_async(conn).await;
    Ok(info.is_ok_and(|value| {
        value
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case("cluster_enabled:1"))
    }))
}

async fn database_count(conn: &mut MultiplexedConnection) -> AppResult<i64> {
    let values: Result<Vec<String>, _> = redis_rs::cmd("CONFIG")
        .arg("GET")
        .arg("databases")
        .query_async(conn)
        .await;
    Ok(values
        .ok()
        .and_then(|values| values.get(1)?.parse::<i64>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(16))
}

async fn scan_keys(
    conn: &mut MultiplexedConnection,
    pattern: &str,
    type_filter: Option<&str>,
    limit: usize,
) -> AppResult<Vec<String>> {
    let mut cursor: u64 = 0;
    let mut keys = Vec::new();
    let mut use_type = type_filter.is_some();
    loop {
        let mut cmd = redis_rs::cmd("SCAN");
        cmd.arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(200);
        if use_type && let Some(key_type) = type_filter {
            cmd.arg("TYPE").arg(key_type);
        }
        match cmd.query_async::<(u64, Vec<String>)>(conn).await {
            Ok((next, batch)) => {
                cursor = next;
                keys.extend(batch);
                if keys.len() >= limit || cursor == 0 {
                    keys.truncate(limit);
                    break;
                }
            }
            Err(_) if use_type => {
                use_type = false;
                cursor = 0;
                keys.clear();
            }
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(key_type) = type_filter.filter(|_| !use_type) {
        keys = filter_keys_by_type(conn, keys, key_type).await?;
        keys.truncate(limit);
    }
    Ok(keys)
}

async fn filter_keys_by_type(
    conn: &mut MultiplexedConnection,
    keys: Vec<String>,
    key_type: &str,
) -> AppResult<Vec<String>> {
    if keys.is_empty() {
        return Ok(keys);
    }
    let mut pipe = redis_rs::pipe();
    for key in &keys {
        pipe.cmd("TYPE").arg(key);
    }
    let types: Vec<String> = pipe.query_async(conn).await?;
    Ok(keys
        .into_iter()
        .zip(types)
        .filter(|(_, found)| found == key_type)
        .map(|(key, _)| key)
        .collect())
}

async fn keys_table_page(
    conn: &mut MultiplexedConnection,
    request: &TablePageRequest,
) -> AppResult<TablePage> {
    let metadata = key_index_metadata();
    let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
    let pattern = scan_pattern(&request.filters)?;
    let type_filter = type_filter(&request.filters);
    let scanned = scan_keys(conn, &pattern, type_filter.as_deref(), KEY_LIST_SCAN_CAP).await?;
    let scan_capped = scanned.len() >= KEY_LIST_SCAN_CAP;
    let mut rows = key_index_rows(conn, scanned).await?;
    rows.retain(|row| row_matches(&metadata, row, &request.filters));
    sort_rows(&metadata, &mut rows, request.order_by.as_ref())?;
    let total_rows = if request.include_total.unwrap_or(true) && !scan_capped {
        Some(rows.len() as u64)
    } else if request.include_total.unwrap_or(true) && request.filters.is_empty() {
        dbsize(conn).await.ok()
    } else {
        None
    };
    let offset = usize::try_from(request.offset).unwrap_or(0);
    let take = usize::try_from(limit).unwrap_or(0);
    let page_rows: Vec<Vec<JsonValue>> = rows.iter().skip(offset).take(take).cloned().collect();
    let has_more = offset + page_rows.len() < rows.len()
        || (scan_capped && offset + page_rows.len() >= rows.len());
    Ok(TablePage {
        metadata,
        columns: vec!["key".into(), "type".into(), "ttl".into()],
        rows: page_rows,
        total_rows,
        offset: request.offset,
        limit,
        has_more,
    })
}

async fn key_index_rows(
    conn: &mut MultiplexedConnection,
    keys: Vec<String>,
) -> AppResult<Vec<Vec<JsonValue>>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut pipe = redis_rs::pipe();
    for key in &keys {
        pipe.cmd("TYPE").arg(key);
        pipe.cmd("TTL").arg(key);
    }
    let values: Vec<Value> = pipe.query_async(conn).await?;
    let mut rows = Vec::with_capacity(keys.len());
    for (index, key) in keys.into_iter().enumerate() {
        let key_type = values
            .get(index * 2)
            .cloned()
            .map(value_as_string)
            .unwrap_or_else(|| "none".into());
        let ttl = values
            .get(index * 2 + 1)
            .cloned()
            .and_then(value_as_i64)
            .unwrap_or(-2);
        if key_type == "none" {
            continue;
        }
        rows.push(vec![
            JsonValue::String(key),
            JsonValue::String(key_type),
            JsonValue::Number(ttl.into()),
        ]);
    }
    Ok(rows)
}

async fn dbsize(conn: &mut MultiplexedConnection) -> AppResult<u64> {
    let size: i64 = redis_rs::cmd("DBSIZE").query_async(conn).await?;
    u64::try_from(size).map_err(|_| AppError::Database("invalid DBSIZE".into()))
}

async fn key_table_page(
    conn: &mut MultiplexedConnection,
    request: &TablePageRequest,
) -> AppResult<TablePage> {
    let key_type = inspect_type(conn, &request.table).await?;
    if key_type == "none" {
        return Err(AppError::InvalidInput(format!(
            "key {} was not found",
            request.table
        )));
    }
    if request.schema != key_type && request.schema != "keys" {
        return Err(AppError::InvalidInput(format!(
            "key {} is a {key_type}, not a {}",
            request.table, request.schema
        )));
    }
    let metadata = key_metadata(&key_type, &request.table);
    let mut rows = load_key_rows(conn, &key_type, &request.table).await?;
    rows.retain(|row| row_matches(&metadata, row, &request.filters));
    sort_rows(&metadata, &mut rows, request.order_by.as_ref())?;
    let total_rows = if request.include_total.unwrap_or(true) {
        Some(rows.len() as u64)
    } else {
        None
    };
    let limit = request.limit.clamp(1, MAX_PAGE_SIZE);
    let offset = usize::try_from(request.offset).unwrap_or(0);
    let take = usize::try_from(limit).unwrap_or(0);
    let page_rows = rows
        .iter()
        .skip(offset)
        .take(take)
        .cloned()
        .collect::<Vec<_>>();
    let has_more = offset + page_rows.len() < rows.len();
    let columns = metadata
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    Ok(TablePage {
        metadata,
        columns,
        rows: page_rows,
        total_rows,
        offset: request.offset,
        limit,
        has_more,
    })
}

async fn inspect_type(conn: &mut MultiplexedConnection, key: &str) -> AppResult<String> {
    let key_type: String = redis_rs::cmd("TYPE").arg(key).query_async(conn).await?;
    Ok(key_type)
}

async fn load_key_rows(
    conn: &mut MultiplexedConnection,
    key_type: &str,
    key: &str,
) -> AppResult<Vec<Vec<JsonValue>>> {
    match key_type {
        "string" => {
            let value: Value = redis_rs::cmd("GET").arg(key).query_async(conn).await?;
            Ok(vec![vec![
                JsonValue::String(key.to_owned()),
                json_from_redis(value),
            ]])
        }
        "hash" => {
            let values: Vec<String> = redis_rs::cmd("HGETALL").arg(key).query_async(conn).await?;
            Ok(values
                .chunks(2)
                .filter_map(|chunk| {
                    Some(vec![
                        JsonValue::String(chunk.first()?.clone()),
                        JsonValue::String(chunk.get(1)?.clone()),
                    ])
                })
                .collect())
        }
        "list" => {
            let values: Vec<String> = redis_rs::cmd("LRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .query_async(conn)
                .await?;
            Ok(values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    vec![
                        JsonValue::Number(i64::try_from(index).unwrap_or(i64::MAX).into()),
                        JsonValue::String(value),
                    ]
                })
                .collect())
        }
        "set" => {
            let values: Vec<String> = redis_rs::cmd("SMEMBERS").arg(key).query_async(conn).await?;
            Ok(values
                .into_iter()
                .map(|member| vec![JsonValue::String(member)])
                .collect())
        }
        "zset" => {
            let values: Vec<String> = redis_rs::cmd("ZRANGE")
                .arg(key)
                .arg(0)
                .arg(-1)
                .arg("WITHSCORES")
                .query_async(conn)
                .await?;
            Ok(values
                .chunks(2)
                .filter_map(|chunk| {
                    Some(vec![
                        JsonValue::String(chunk.first()?.clone()),
                        score_json(chunk.get(1)?),
                    ])
                })
                .collect())
        }
        "stream" => {
            let entries: Vec<Value> = redis_rs::cmd("XRANGE")
                .arg(key)
                .arg("-")
                .arg("+")
                .query_async(conn)
                .await?;
            Ok(entries
                .into_iter()
                .filter_map(|entry| {
                    let Value::Array(parts) = entry else {
                        return None;
                    };
                    let id = json_from_redis(parts.first()?.clone());
                    let fields = parts.get(1).cloned().unwrap_or(Value::Nil);
                    Some(vec![id, json_from_redis(fields)])
                })
                .collect())
        }
        other => Err(AppError::Unsupported(format!(
            "Redis type {other} is not supported in the table viewer"
        ))),
    }
}

async fn apply_key_index_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    let mut applied = 0;
    let mut conflicts = Vec::new();
    for mutation in &batch.mutations {
        let key = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
        if key.is_empty() {
            return Err(AppError::InvalidInput("Redis key is required".into()));
        }
        let exists: bool = redis_rs::cmd("EXISTS").arg(&key).query_async(conn).await?;
        if !exists {
            conflicts.push(mutation.primary_key.clone());
            continue;
        }
        if mutation.deleted {
            let _: i64 = redis_rs::cmd("DEL").arg(&key).query_async(conn).await?;
            applied += 1;
            continue;
        }
        if mutation.original.get(1) != mutation.changes.get(1) {
            return Err(AppError::Unsupported(
                "Redis key types cannot be changed from the table editor".into(),
            ));
        }
        if mutation.original.get(2) != mutation.changes.get(2) {
            apply_ttl(conn, &key, mutation.changes.get(2)).await?;
        }
        applied += 1;
    }
    Ok(MutationResult { applied, conflicts })
}

async fn apply_key_mutations(
    conn: &mut MultiplexedConnection,
    batch: &MutationBatch,
) -> AppResult<MutationResult> {
    let key_type = inspect_type(conn, &batch.table).await?;
    if key_type == "none" {
        return Ok(MutationResult {
            applied: 0,
            conflicts: batch
                .mutations
                .iter()
                .map(|mutation| mutation.primary_key.clone())
                .collect(),
        });
    }
    if batch.schema != key_type {
        return Err(AppError::InvalidInput(format!(
            "key {} is a {key_type}, not a {}",
            batch.table, batch.schema
        )));
    }
    let mut applied = 0;
    let mut conflicts = Vec::new();
    for mutation in &batch.mutations {
        let changed = match key_type.as_str() {
            "string" => apply_string_mutation(conn, &batch.table, mutation).await?,
            "hash" => apply_hash_mutation(conn, &batch.table, mutation).await?,
            "list" => apply_list_mutation(conn, &batch.table, mutation).await?,
            "set" => apply_set_mutation(conn, &batch.table, mutation).await?,
            "zset" => apply_zset_mutation(conn, &batch.table, mutation).await?,
            "stream" => apply_stream_mutation(conn, &batch.table, mutation).await?,
            other => {
                return Err(AppError::Unsupported(format!(
                    "Redis type {other} cannot be edited"
                )));
            }
        };
        if changed {
            applied += 1;
        } else {
            conflicts.push(mutation.primary_key.clone());
        }
    }
    Ok(MutationResult { applied, conflicts })
}

async fn apply_string_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let exists: bool = redis_rs::cmd("EXISTS").arg(key).query_async(conn).await?;
    if !exists {
        return Ok(false);
    }
    if mutation.deleted {
        let _: i64 = redis_rs::cmd("DEL").arg(key).query_async(conn).await?;
        return Ok(true);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis_rs::cmd("SET")
        .arg(key)
        .arg(value)
        .query_async::<Value>(conn)
        .await?;
    Ok(true)
}

async fn apply_hash_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let field = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
    let exists: bool = redis_rs::cmd("HEXISTS")
        .arg(key)
        .arg(&field)
        .query_async(conn)
        .await?;
    if !exists {
        return Ok(false);
    }
    if mutation.deleted {
        let _: i64 = redis_rs::cmd("HDEL")
            .arg(key)
            .arg(&field)
            .query_async(conn)
            .await?;
        return Ok(true);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis_rs::cmd("HSET")
        .arg(key)
        .arg(&field)
        .arg(value)
        .query_async::<Value>(conn)
        .await?;
    Ok(true)
}

async fn apply_list_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let index = json_to_i64(mutation.primary_key.first().unwrap_or(&JsonValue::Null))?;
    let current: Value = redis_rs::cmd("LINDEX")
        .arg(key)
        .arg(index)
        .query_async(conn)
        .await?;
    if matches!(current, Value::Nil) {
        return Ok(false);
    }
    if mutation.deleted {
        let sentinel = format!("dbm-deleted-{}", Uuid::new_v4());
        if redis_rs::cmd("LSET")
            .arg(key)
            .arg(index)
            .arg(&sentinel)
            .query_async::<Value>(conn)
            .await
            .is_err()
        {
            return Ok(false);
        }
        let _: i64 = redis_rs::cmd("LREM")
            .arg(key)
            .arg(1)
            .arg(&sentinel)
            .query_async(conn)
            .await?;
        return Ok(true);
    }
    let value = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis_rs::cmd("LSET")
        .arg(key)
        .arg(index)
        .arg(value)
        .query_async::<Value>(conn)
        .await
        .map_err(|_| AppError::Database("list index is out of range".into()))?;
    Ok(true)
}

async fn apply_set_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let member = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
    let exists: bool = redis_rs::cmd("SISMEMBER")
        .arg(key)
        .arg(&member)
        .query_async(conn)
        .await?;
    if !exists {
        return Ok(false);
    }
    if mutation.deleted {
        let _: i64 = redis_rs::cmd("SREM")
            .arg(key)
            .arg(&member)
            .query_async(conn)
            .await?;
        return Ok(true);
    }
    Ok(true)
}

async fn apply_zset_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    let member = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
    let score: Value = redis_rs::cmd("ZSCORE")
        .arg(key)
        .arg(&member)
        .query_async(conn)
        .await?;
    if matches!(score, Value::Nil) {
        return Ok(false);
    }
    if mutation.deleted {
        let _: i64 = redis_rs::cmd("ZREM")
            .arg(key)
            .arg(&member)
            .query_async(conn)
            .await?;
        return Ok(true);
    }
    let next_score = json_to_string(mutation.changes.get(1).unwrap_or(&JsonValue::Null));
    redis_rs::cmd("ZADD")
        .arg(key)
        .arg(next_score)
        .arg(&member)
        .query_async::<Value>(conn)
        .await?;
    Ok(true)
}

async fn apply_stream_mutation(
    conn: &mut MultiplexedConnection,
    key: &str,
    mutation: &RowMutation,
) -> AppResult<bool> {
    if !mutation.deleted {
        return Err(AppError::Unsupported(
            "stream entries can only be deleted from the table editor".into(),
        ));
    }
    let id = json_to_string(mutation.primary_key.first().unwrap_or(&JsonValue::Null));
    let removed: i64 = redis_rs::cmd("XDEL")
        .arg(key)
        .arg(&id)
        .query_async(conn)
        .await?;
    Ok(removed > 0)
}

async fn apply_ttl(
    conn: &mut MultiplexedConnection,
    key: &str,
    ttl: Option<&JsonValue>,
) -> AppResult<()> {
    let ttl = ttl
        .map(json_to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-1".into())
        .parse::<i64>()
        .map_err(|_| AppError::InvalidInput("TTL must be an integer number of seconds".into()))?;
    if ttl < 0 {
        redis_rs::cmd("PERSIST")
            .arg(key)
            .query_async::<Value>(conn)
            .await?;
    } else {
        redis_rs::cmd("EXPIRE")
            .arg(key)
            .arg(ttl)
            .query_async::<Value>(conn)
            .await?;
    }
    Ok(())
}

fn key_index_metadata() -> TableMetadata {
    TableMetadata {
        schema: "keys".into(),
        table: "all".into(),
        columns: vec![
            column("key", "string", 1, false),
            column("type", "string", 2, false),
            column("ttl", "integer", 3, false),
        ],
        primary_key: vec!["key".into()],
        has_xmin: false,
    }
}

fn key_metadata(key_type: &str, key: &str) -> TableMetadata {
    let (columns, primary_key) = match key_type {
        "string" => (
            vec![
                column("key", "string", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["key".into()],
        ),
        "hash" => (
            vec![
                column("field", "string", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["field".into()],
        ),
        "list" => (
            vec![
                column("index", "integer", 1, false),
                column("value", "string", 2, true),
            ],
            vec!["index".into()],
        ),
        "set" => (
            vec![column("member", "string", 1, false)],
            vec!["member".into()],
        ),
        "zset" => (
            vec![
                column("member", "string", 1, false),
                column("score", "number", 2, false),
            ],
            vec!["member".into()],
        ),
        _ => (
            vec![
                column("id", "string", 1, false),
                column("fields", "string", 2, true),
            ],
            vec!["id".into()],
        ),
    };
    TableMetadata {
        schema: key_type.to_owned(),
        table: key.to_owned(),
        columns,
        primary_key,
        has_xmin: false,
    }
}

fn column(name: &str, data_type: &str, ordinal: i32, nullable: bool) -> TableColumn {
    TableColumn {
        name: name.into(),
        data_type: data_type.into(),
        nullable,
        default_value: None,
        ordinal,
    }
}

fn type_label(key_type: &str) -> &'static str {
    match key_type {
        "string" => "Strings",
        "hash" => "Hashes",
        "list" => "Lists",
        "set" => "Sets",
        "zset" => "Sorted sets",
        "stream" => "Streams",
        _ => "Keys",
    }
}

fn scan_pattern(filters: &[FilterCondition]) -> AppResult<String> {
    let Some(filter) = filters.iter().find(|filter| filter.column == "key") else {
        return Ok("*".into());
    };
    let value = filter.value.as_deref().unwrap_or_default();
    Ok(match filter.operator {
        FilterOperator::Equals => value.to_owned(),
        FilterOperator::Contains => format!("*{value}*"),
        FilterOperator::StartsWith => format!("{value}*"),
        FilterOperator::EndsWith => format!("*{value}"),
        FilterOperator::In => {
            return Err(AppError::InvalidInput(
                "Redis key IN filters are not supported; use equals, contains, starts with, or ends with".into(),
            ));
        }
        _ => format!("*{value}*"),
    })
}

fn type_filter(filters: &[FilterCondition]) -> Option<String> {
    filters.iter().find_map(|filter| {
        if filter.column == "type" && matches!(filter.operator, FilterOperator::Equals) {
            filter.value.clone()
        } else {
            None
        }
    })
}

fn row_matches(metadata: &TableMetadata, row: &[JsonValue], filters: &[FilterCondition]) -> bool {
    filters.iter().all(|filter| {
        let Some(index) = metadata
            .columns
            .iter()
            .position(|column| column.name == filter.column)
        else {
            return false;
        };
        let cell = row.get(index).unwrap_or(&JsonValue::Null);
        let cell_text = json_to_string(cell);
        let value = filter.value.as_deref().unwrap_or_default();
        match filter.operator {
            FilterOperator::Equals => cell_text == value,
            FilterOperator::NotEquals => cell_text != value,
            FilterOperator::Contains => cell_text
                .to_ascii_lowercase()
                .contains(&value.to_ascii_lowercase()),
            FilterOperator::StartsWith => cell_text
                .to_ascii_lowercase()
                .starts_with(&value.to_ascii_lowercase()),
            FilterOperator::EndsWith => cell_text
                .to_ascii_lowercase()
                .ends_with(&value.to_ascii_lowercase()),
            FilterOperator::GreaterThan => compare_json(cell, value) == Ordering::Greater,
            FilterOperator::GreaterThanOrEqual => compare_json(cell, value) != Ordering::Less,
            FilterOperator::LessThan => compare_json(cell, value) == Ordering::Less,
            FilterOperator::LessThanOrEqual => compare_json(cell, value) != Ordering::Greater,
            FilterOperator::In => value
                .split(',')
                .map(str::trim)
                .any(|item| item == cell_text),
            FilterOperator::NotIn => value
                .split(',')
                .map(str::trim)
                .all(|item| item != cell_text),
            FilterOperator::IsNull => matches!(cell, JsonValue::Null),
            FilterOperator::IsNotNull => !matches!(cell, JsonValue::Null),
        }
    })
}

fn sort_rows(
    metadata: &TableMetadata,
    rows: &mut [Vec<JsonValue>],
    order_by: Option<&OrderSpec>,
) -> AppResult<()> {
    let Some(order_by) = order_by else {
        return Ok(());
    };
    let Some(index) = metadata
        .columns
        .iter()
        .position(|column| column.name == order_by.column)
    else {
        return Err(AppError::InvalidInput(format!(
            "unknown order column {}",
            order_by.column
        )));
    };
    rows.sort_by(|left, right| {
        let ordering = compare_cells(
            left.get(index).unwrap_or(&JsonValue::Null),
            right.get(index).unwrap_or(&JsonValue::Null),
        );
        if order_by.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(())
}

fn compare_json(cell: &JsonValue, value: &str) -> Ordering {
    if let (Some(left), Ok(right)) = (cell.as_i64(), value.parse::<i64>()) {
        return left.cmp(&right);
    }
    if let (Some(left), Ok(right)) = (cell.as_f64(), value.parse::<f64>()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    json_to_string(cell).cmp(&value.to_owned())
}

fn compare_cells(left: &JsonValue, right: &JsonValue) -> Ordering {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left.cmp(&right);
    }
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    json_to_string(left).cmp(&json_to_string(right))
}

fn tokenize_redis_cli(input: &str) -> AppResult<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(character) = chars.next() {
        match character {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if in_double => match chars.next() {
                Some('n') => current.push('\n'),
                Some('r') => current.push('\r'),
                Some('t') => current.push('\t'),
                Some(escaped) => current.push(escaped),
                None => {
                    return Err(AppError::InvalidInput(
                        "Redis command has a trailing escape".into(),
                    ));
                }
            },
            character if character.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if in_single || in_double {
        return Err(AppError::InvalidInput(
            "Redis command has an unterminated quote".into(),
        ));
    }
    if !current.is_empty() {
        args.push(current);
    }
    if args.is_empty() {
        return Err(AppError::InvalidInput("query is empty".into()));
    }
    Ok(args)
}

fn is_unsupported_command(command: &str, argv: &[String]) -> bool {
    matches!(
        command,
        "subscribe"
            | "psubscribe"
            | "unsubscribe"
            | "punsubscribe"
            | "ssubscribe"
            | "monitor"
            | "blpop"
            | "brpop"
            | "brpoplpush"
            | "blmove"
            | "blmpop"
            | "bzpopmin"
            | "bzpopmax"
            | "bzmpop"
    ) || (matches!(command, "xread" | "xreadgroup")
        && argv.iter().any(|arg| arg.eq_ignore_ascii_case("block")))
}

fn is_read_command(argv: &[String]) -> bool {
    let command = argv[0].to_ascii_lowercase();
    let sub = argv.get(1).map(|value| value.to_ascii_lowercase());
    match command.as_str() {
        "config" => sub.as_deref() == Some("get"),
        "acl" => matches!(
            sub.as_deref(),
            Some("cat" | "genpass" | "getuser" | "help" | "list" | "log" | "whoami")
        ),
        "client" => matches!(sub.as_deref(), Some("getname" | "id" | "info" | "list")),
        "memory" => matches!(
            sub.as_deref(),
            Some("doctor" | "help" | "malloc-stats" | "stats" | "usage")
        ),
        "slowlog" => sub.as_deref() != Some("reset"),
        "script" => matches!(sub.as_deref(), Some("exists" | "help")),
        "module" => matches!(sub.as_deref(), Some("help" | "list")),
        "command" => true,
        "xinfo" => true,
        "object" => true,
        "latency" => sub.as_deref() != Some("reset"),
        _ => matches!(
            command.as_str(),
            "ping"
                | "echo"
                | "get"
                | "mget"
                | "getrange"
                | "substr"
                | "strlen"
                | "exists"
                | "type"
                | "ttl"
                | "pttl"
                | "scan"
                | "keys"
                | "dbsize"
                | "info"
                | "hget"
                | "hmget"
                | "hgetall"
                | "hkeys"
                | "hvals"
                | "hexists"
                | "hlen"
                | "hscan"
                | "lrange"
                | "lindex"
                | "llen"
                | "smembers"
                | "scard"
                | "sismember"
                | "srandmember"
                | "sinter"
                | "sunion"
                | "sdiff"
                | "sscan"
                | "zrange"
                | "zrevrange"
                | "zrangebyscore"
                | "zrevrangebyscore"
                | "zscore"
                | "zcard"
                | "zcount"
                | "zrank"
                | "zrevrank"
                | "zscan"
                | "xrange"
                | "xrevrange"
                | "xlen"
                | "dump"
                | "randomkey"
                | "time"
                | "lastsave"
                | "role"
                | "hello"
                | "lolwut"
                | "auth"
                | "select"
                | "readonly"
        ),
    }
}

fn value_to_query_response(
    argv: &[String],
    value: Value,
    max_rows: usize,
    duration_ms: u128,
) -> QueryResponse {
    let command = argv
        .first()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(value, Value::Okay) {
        return QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows: Some(1),
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        };
    }
    if value.looks_like_cursor() {
        return cursor_query_response(value, max_rows, duration_ms);
    }
    if is_pair_command(&command, argv)
        && let Some(rows) = pair_rows(&value)
    {
        return capped_query_response(
            vec![
                QueryColumn {
                    name: "field".into(),
                    data_type: "string".into(),
                },
                QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                },
            ],
            rows,
            max_rows,
            duration_ms,
        );
    }
    match value {
        Value::Nil => QueryResponse {
            columns: vec![QueryColumn {
                name: "value".into(),
                data_type: "nil".into(),
            }],
            rows: vec![vec![JsonValue::Null]],
            row_count: 1,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
        Value::Array(items) if items.iter().all(|item| matches!(item, Value::Array(_))) => {
            let rows = items
                .into_iter()
                .filter_map(|item| {
                    if let Value::Array(cells) = item {
                        Some(cells.into_iter().map(json_from_redis).collect::<Vec<_>>())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            let columns = (0..width)
                .map(|index| QueryColumn {
                    name: format!("column{}", index + 1),
                    data_type: "string".into(),
                })
                .collect();
            capped_query_response(columns, rows, max_rows, duration_ms)
        }
        Value::Array(items) => {
            let rows = items
                .into_iter()
                .map(|item| vec![json_from_redis(item)])
                .collect();
            capped_query_response(
                vec![QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                }],
                rows,
                max_rows,
                duration_ms,
            )
        }
        Value::Map(pairs) => {
            let rows = pairs
                .into_iter()
                .map(|(field, value)| vec![json_from_redis(field), json_from_redis(value)])
                .collect();
            capped_query_response(
                vec![
                    QueryColumn {
                        name: "field".into(),
                        data_type: "string".into(),
                    },
                    QueryColumn {
                        name: "value".into(),
                        data_type: "string".into(),
                    },
                ],
                rows,
                max_rows,
                duration_ms,
            )
        }
        Value::Int(number) => QueryResponse {
            columns: vec![QueryColumn {
                name: "result".into(),
                data_type: "integer".into(),
            }],
            rows: vec![vec![JsonValue::Number(number.into())]],
            row_count: 1,
            affected_rows: Some(u64::try_from(number).unwrap_or(0)),
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
        other => QueryResponse {
            columns: vec![QueryColumn {
                name: "value".into(),
                data_type: "string".into(),
            }],
            rows: vec![vec![json_from_redis(other)]],
            row_count: 1,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        },
    }
}

fn cursor_query_response(value: Value, max_rows: usize, duration_ms: u128) -> QueryResponse {
    let Value::Array(mut parts) = value else {
        return QueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows: None,
            duration_ms,
            truncated: false,
            notices: Vec::new(),
        };
    };
    let cursor = parts
        .first()
        .cloned()
        .map(json_from_redis)
        .unwrap_or(JsonValue::String("0".into()));
    let items = if parts.len() > 1 {
        parts.swap_remove(1)
    } else {
        Value::Nil
    };
    let rows = if let Some(pairs) = pair_rows(&items) {
        let mut response = capped_query_response(
            vec![
                QueryColumn {
                    name: "field".into(),
                    data_type: "string".into(),
                },
                QueryColumn {
                    name: "value".into(),
                    data_type: "string".into(),
                },
            ],
            pairs,
            max_rows,
            duration_ms,
        );
        response.notices = vec![format!("cursor {cursor}")];
        return response;
    } else if let Value::Array(values) = items {
        values
            .into_iter()
            .map(|item| vec![json_from_redis(item)])
            .collect()
    } else {
        vec![vec![json_from_redis(items)]]
    };
    let mut response = capped_query_response(
        vec![QueryColumn {
            name: "value".into(),
            data_type: "string".into(),
        }],
        rows,
        max_rows,
        duration_ms,
    );
    response.notices = vec![format!("cursor {cursor}")];
    response
}

fn capped_query_response(
    columns: Vec<QueryColumn>,
    mut rows: Vec<Vec<JsonValue>>,
    max_rows: usize,
    duration_ms: u128,
) -> QueryResponse {
    let truncated = rows.len() > max_rows;
    rows.truncate(max_rows);
    QueryResponse {
        row_count: rows.len(),
        rows,
        columns,
        affected_rows: None,
        duration_ms,
        truncated,
        notices: Vec::new(),
    }
}

fn is_pair_command(command: &str, argv: &[String]) -> bool {
    matches!(command, "hgetall" | "config")
        || (command == "zrange"
            || command == "zrevrange"
            || command == "zrangebyscore"
            || command == "zrevrangebyscore")
            && argv
                .iter()
                .any(|arg| arg.eq_ignore_ascii_case("withscores"))
}

fn pair_rows(value: &Value) -> Option<Vec<Vec<JsonValue>>> {
    let Value::Array(items) = value else {
        return None;
    };
    if items.len() % 2 != 0 {
        return None;
    }
    Some(
        items
            .chunks(2)
            .map(|chunk| {
                vec![
                    json_from_redis(chunk[0].clone()),
                    json_from_redis(chunk[1].clone()),
                ]
            })
            .collect(),
    )
}

fn json_from_redis(value: Value) -> JsonValue {
    match value {
        Value::Nil => JsonValue::Null,
        Value::Int(value) => JsonValue::Number(value.into()),
        Value::BulkString(bytes) => bulk_to_json(&bytes),
        Value::Array(items) => JsonValue::Array(items.into_iter().map(json_from_redis).collect()),
        Value::SimpleString(value) => JsonValue::String(value),
        Value::Okay => JsonValue::String("OK".into()),
        Value::Map(pairs) => {
            let mut object = serde_json::Map::new();
            for (key, value) in pairs {
                object.insert(
                    json_to_string(&json_from_redis(key)),
                    json_from_redis(value),
                );
            }
            JsonValue::Object(object)
        }
        Value::Boolean(value) => JsonValue::Bool(value),
        Value::Double(value) => serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Set(items) => JsonValue::Array(items.into_iter().map(json_from_redis).collect()),
        Value::VerbatimString { text, .. } => JsonValue::String(text),
        Value::Attribute { data, .. } => json_from_redis(*data),
        other => JsonValue::String(format!("{other:?}")),
    }
}

fn bulk_to_json(bytes: &[u8]) -> JsonValue {
    match String::from_utf8(bytes.to_vec()) {
        Ok(mut text) => {
            if text.len() > MAX_VALUE_BYTES {
                text.truncate(MAX_VALUE_BYTES);
                text.push_str("… (truncated)");
            }
            JsonValue::String(text)
        }
        Err(_) => JsonValue::String(format!("\\x{}", hex_encode(bytes))),
    }
}

fn value_as_string(value: Value) -> String {
    match json_from_redis(value) {
        JsonValue::String(value) => value,
        other => json_to_string(&other),
    }
}

fn value_as_i64(value: Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(value),
        Value::BulkString(bytes) => String::from_utf8(bytes).ok()?.parse().ok(),
        Value::SimpleString(value) => value.parse().ok(),
        _ => None,
    }
}

fn score_json(value: &str) -> JsonValue {
    value
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(JsonValue::Number)
        .unwrap_or_else(|| JsonValue::String(value.to_owned()))
}

fn json_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn json_to_i64(value: &JsonValue) -> AppResult<i64> {
    match value {
        JsonValue::Number(value) => value
            .as_i64()
            .ok_or_else(|| AppError::InvalidInput("list index must be an integer".into())),
        JsonValue::String(value) => value
            .parse()
            .map_err(|_| AppError::InvalidInput("list index must be an integer".into())),
        _ => Err(AppError::InvalidInput(
            "list index must be an integer".into(),
        )),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DatabaseEngine;
    use chrono::Utc;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn tokenizes_quoted_redis_cli_arguments() {
        assert_eq!(
            tokenize_redis_cli(r#"SET greeting "hello world""#).expect("tokens"),
            ["SET", "greeting", "hello world"]
        );
        assert_eq!(
            tokenize_redis_cli("HSET user:1 name 'Ada Lovelace'").expect("tokens"),
            ["HSET", "user:1", "name", "Ada Lovelace"]
        );
        assert!(tokenize_redis_cli(r#"SET broken "unterminated"#).is_err());
    }

    #[test]
    fn read_only_command_detection_covers_common_writes() {
        assert!(is_read_command(&["GET".into(), "greeting".into()]));
        assert!(is_read_command(&["HGETALL".into(), "user:1".into()]));
        assert!(is_read_command(&[
            "CONFIG".into(),
            "GET".into(),
            "databases".into()
        ]));
        assert!(!is_read_command(&[
            "SET".into(),
            "greeting".into(),
            "hi".into()
        ]));
        assert!(!is_read_command(&["DEL".into(), "greeting".into()]));
        assert!(!is_read_command(&["FLUSHALL".into()]));
        assert!(!is_read_command(&[
            "CONFIG".into(),
            "SET".into(),
            "slowlog-log-slower-than".into()
        ]));
        assert!(is_unsupported_command(
            "subscribe",
            &["SUBSCRIBE".into(), "news".into()]
        ));
        assert!(is_unsupported_command(
            "xread",
            &[
                "XREAD".into(),
                "BLOCK".into(),
                "0".into(),
                "STREAMS".into(),
                "s".into(),
                "0".into()
            ]
        ));
    }

    #[test]
    fn hgetall_results_become_field_value_rows() {
        let response = value_to_query_response(
            &["HGETALL".into(), "user:1".into()],
            Value::Array(vec![
                Value::SimpleString("name".into()),
                Value::SimpleString("Ada".into()),
                Value::SimpleString("role".into()),
                Value::SimpleString("engineer".into()),
            ]),
            10,
            3,
        );
        assert_eq!(
            response
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["field", "value"]
        );
        assert_eq!(response.rows.len(), 2);
        assert_eq!(response.rows[0][0], JsonValue::String("name".into()));
    }

    struct TestRedis {
        child: std::process::Child,
    }

    impl Drop for TestRedis {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn unused_port() -> Option<u16> {
        std::net::TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()
            .map(|addr| addr.port())
    }

    async fn start_test_redis() -> Option<(TestRedis, RedisSession)> {
        let port = unused_port()?;
        let child = Command::new("redis-server")
            .args([
                "--port",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
                "--save",
                "",
                "--appendonly",
                "no",
                "--protected-mode",
                "no",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let server = TestRedis { child };
        let profile = test_profile(port, false);
        for _ in 0..80 {
            if let Ok(session) = RedisSession::connect(profile.clone(), None).await {
                return Some((server, session));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    fn test_profile(port: u16, read_only: bool) -> ConnectionProfile {
        let now = Utc::now();
        ConnectionProfile {
            id: Uuid::new_v4(),
            name: "test-redis".into(),
            color: None,
            engine: DatabaseEngine::Redis,
            host: "127.0.0.1".into(),
            port,
            username: String::new(),
            default_database: "0".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn redis_session_browses_keys_runs_commands_and_applies_edits() {
        let Some((_server, session)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        session
            .run_query(r#"SET greeting "hello dbm""#, None)
            .await
            .expect("set string");
        session
            .run_query("HSET user:1 name Ada role engineer", None)
            .await
            .expect("set hash");
        session
            .run_query("LPUSH tasks ship", None)
            .await
            .expect("set list");

        let databases = session.list_databases().await.expect("databases");
        assert!(databases.iter().any(|database| database.name == "0"));

        let tree = session.schema_tree().await.expect("tree");
        assert!(
            tree.iter()
                .any(|node| node.schema.as_deref() == Some("keys"))
        );
        assert!(tree.iter().any(|node| {
            node.schema.as_deref() == Some("string")
                && node
                    .children
                    .iter()
                    .any(|child| child.table.as_deref() == Some("greeting"))
        }));
        assert!(tree.iter().any(|node| {
            node.schema.as_deref() == Some("hash")
                && node
                    .children
                    .iter()
                    .any(|child| child.table.as_deref() == Some("user:1"))
        }));

        let keys = session
            .table_page(&TablePageRequest {
                profile_id: session.profile().id,
                schema: "keys".into(),
                table: "all".into(),
                offset: 0,
                limit: 50,
                filters: vec![FilterCondition {
                    column: "key".into(),
                    operator: FilterOperator::Contains,
                    value: Some("user".into()),
                }],
                order_by: Some(OrderSpec {
                    column: "key".into(),
                    descending: false,
                }),
                include_total: Some(true),
            })
            .await
            .expect("keys page");
        assert_eq!(keys.rows.len(), 1);
        assert_eq!(keys.rows[0][0], JsonValue::String("user:1".into()));
        assert_eq!(keys.rows[0][1], JsonValue::String("hash".into()));

        let hash = session
            .table_page(&TablePageRequest {
                profile_id: session.profile().id,
                schema: "hash".into(),
                table: "user:1".into(),
                offset: 0,
                limit: 50,
                filters: Vec::new(),
                order_by: None,
                include_total: Some(true),
            })
            .await
            .expect("hash page");
        assert!(
            hash.rows
                .iter()
                .any(|row| row[0] == JsonValue::String("name".into())
                    && row[1] == JsonValue::String("Ada".into()))
        );

        let ping = session.run_query("PING", None).await.expect("ping");
        assert_eq!(ping.rows[0][0], JsonValue::String("PONG".into()));

        let mutated = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                schema: "hash".into(),
                table: "user:1".into(),
                mutations: vec![RowMutation {
                    original: vec![
                        JsonValue::String("name".into()),
                        JsonValue::String("Ada".into()),
                    ],
                    changes: vec![
                        JsonValue::String("name".into()),
                        JsonValue::String("Grace".into()),
                    ],
                    primary_key: vec![JsonValue::String("name".into())],
                    xmin: None,
                    deleted: false,
                }],
            })
            .await
            .expect("mutate hash");
        assert_eq!(mutated.applied, 1);

        let after = session
            .run_query("HGET user:1 name", None)
            .await
            .expect("hget");
        assert_eq!(after.rows[0][0], JsonValue::String("Grace".into()));

        let deleted = session
            .apply_mutations(&MutationBatch {
                profile_id: session.profile().id,
                schema: "keys".into(),
                table: "all".into(),
                mutations: vec![RowMutation {
                    original: vec![
                        JsonValue::String("greeting".into()),
                        JsonValue::String("string".into()),
                        JsonValue::Number((-1).into()),
                    ],
                    changes: vec![
                        JsonValue::String("greeting".into()),
                        JsonValue::String("string".into()),
                        JsonValue::Number((-1).into()),
                    ],
                    primary_key: vec![JsonValue::String("greeting".into())],
                    xmin: None,
                    deleted: true,
                }],
            })
            .await
            .expect("delete key");
        assert_eq!(deleted.applied, 1);
        let missing = session
            .run_query("EXISTS greeting", None)
            .await
            .expect("exists");
        assert_eq!(missing.rows[0][0], JsonValue::Number(0.into()));
    }

    #[tokio::test]
    async fn redis_read_only_profiles_block_writes() {
        let Some((_server, writable)) = start_test_redis().await else {
            eprintln!("skipping live Redis test because redis-server is unavailable");
            return;
        };
        let port = writable.profile().port;
        drop(writable);
        let session = RedisSession::connect(test_profile(port, true), None)
            .await
            .expect("connect read-only");
        let error = session
            .run_query("SET blocked 1", None)
            .await
            .expect_err("write blocked");
        assert!(error.to_string().contains("read-only"));
    }
}
