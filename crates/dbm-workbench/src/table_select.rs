//! Resolving `SELECT * FROM table` to the single table it names.
//!
//! The React workbench shows a full, editable table viewer when a query is an
//! exact full-table select that resolves to one table in the sidebar. The
//! native frontends share this resolver so they agree on when that applies.

use dbm_engine::models::SchemaNode;

/// Returns the `(schema, table)` a full-table select names, when the query is
/// exactly `SELECT * FROM [schema.]table` and the schema tree has one match.
pub fn resolve_full_table_select(sql: &str, tree: &[SchemaNode]) -> Option<(String, String)> {
    let (schema, table) = parse_full_table_select(sql)?;
    let mut matches = Vec::new();
    for node in tree {
        collect_matches(node, schema.as_deref(), &table, &mut matches);
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

fn collect_matches(
    node: &SchemaNode,
    schema: Option<&str>,
    table: &str,
    matches: &mut Vec<(String, String)>,
) {
    let is_match = node.kind == "table"
        && node.table.as_deref() == Some(table)
        && schema.is_none_or(|wanted| node.schema.as_deref() == Some(wanted));
    if is_match && let Some(node_schema) = node.schema.clone() {
        matches.push((node_schema, table.to_owned()));
    }
    for child in &node.children {
        collect_matches(child, schema, table, matches);
    }
}

/// Parses the shape only; the caller resolves the identifiers against a tree.
fn parse_full_table_select(sql: &str) -> Option<(Option<String>, String)> {
    let mut rest = sql.trim();
    rest = rest.strip_suffix(';').map_or(rest, str::trim_end);
    rest = expect_keyword(rest, "select")?;
    rest = expect_symbol(rest, '*')?;
    rest = expect_keyword(rest, "from")?;
    let (first, tail) = scan_identifier(rest)?;
    let (second, tail) = match tail.trim_start().strip_prefix('.') {
        Some(after_dot) => {
            let (identifier, tail) = scan_identifier(after_dot)?;
            (Some(identifier), tail)
        }
        None => (None, tail),
    };
    if !tail.trim().is_empty() {
        return None;
    }
    match second {
        Some(table) => Some((Some(first), table)),
        None => Some((None, first)),
    }
}

fn expect_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = input.trim_start();
    let (word, rest) = split_word(trimmed)?;
    word.eq_ignore_ascii_case(keyword).then_some(rest)
}

fn expect_symbol(input: &str, symbol: char) -> Option<&str> {
    input.trim_start().strip_prefix(symbol)
}

fn split_word(input: &str) -> Option<(&str, &str)> {
    let end = input
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(input.len());
    (end > 0).then(|| (&input[..end], &input[end..]))
}

/// Reads a bare, quoted, or backticked identifier and decodes it the way the
/// database would: unquoted identifiers fold to lower case.
fn scan_identifier(input: &str) -> Option<(String, &str)> {
    let trimmed = input.trim_start();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let mut value = String::new();
        let mut offset = 0;
        while offset < rest.len() {
            let character = rest[offset..].chars().next()?;
            if character == '"' {
                let after = &rest[offset + 1..];
                if let Some(tail) = after.strip_prefix('"') {
                    value.push('"');
                    offset = rest.len() - tail.len();
                    continue;
                }
                return Some((value, after));
            }
            value.push(character);
            offset += character.len_utf8();
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('`') {
        let end = rest.find('`')?;
        return Some((rest[..end].to_owned(), &rest[end + 1..]));
    }
    let (word, rest) = split_word(trimmed)?;
    if word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((word.to_ascii_lowercase(), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: &str, schema: &str, table: &str) -> SchemaNode {
        SchemaNode {
            name: table.to_owned(),
            kind: kind.to_owned(),
            schema: Some(schema.to_owned()),
            table: Some(table.to_owned()),
            children: Vec::new(),
        }
    }

    fn tree() -> Vec<SchemaNode> {
        vec![
            SchemaNode {
                name: "public".to_owned(),
                kind: "schema".to_owned(),
                schema: None,
                table: None,
                children: vec![
                    node("table", "public", "users"),
                    node("table", "public", "orders"),
                ],
            },
            SchemaNode {
                name: "audit".to_owned(),
                kind: "schema".to_owned(),
                schema: None,
                table: None,
                children: vec![node("table", "audit", "users")],
            },
        ]
    }

    #[test]
    fn resolves_an_exact_full_table_select() {
        assert_eq!(
            resolve_full_table_select("SELECT * FROM orders;", &tree()),
            Some(("public".to_owned(), "orders".to_owned()))
        );
        assert_eq!(
            resolve_full_table_select("select * from public.orders", &tree()),
            Some(("public".to_owned(), "orders".to_owned()))
        );
    }

    #[test]
    fn qualified_names_disambiguate() {
        assert_eq!(
            resolve_full_table_select("SELECT * FROM audit.users", &tree()),
            Some(("audit".to_owned(), "users".to_owned()))
        );
        assert_eq!(
            resolve_full_table_select("SELECT * FROM public.users", &tree()),
            Some(("public".to_owned(), "users".to_owned()))
        );
    }

    #[test]
    fn ambiguous_or_complex_queries_stay_read_only() {
        // `users` exists in two schemas, so an unqualified select is ambiguous.
        assert_eq!(
            resolve_full_table_select("SELECT * FROM users", &tree()),
            None
        );
        assert_eq!(
            resolve_full_table_select("SELECT * FROM missing", &tree()),
            None
        );
        assert_eq!(
            resolve_full_table_select("SELECT id FROM users", &tree()),
            None
        );
        assert_eq!(
            resolve_full_table_select("SELECT * FROM users WHERE id = 1", &tree()),
            None
        );
        assert_eq!(
            resolve_full_table_select("SELECT * FROM users; SELECT 1", &tree()),
            None
        );
    }

    #[test]
    fn quoted_identifiers_keep_their_case() {
        let tree = vec![SchemaNode {
            name: "Mixed".to_owned(),
            kind: "schema".to_owned(),
            schema: None,
            table: None,
            children: vec![node("table", "Mixed", "Users")],
        }];
        assert_eq!(
            resolve_full_table_select("SELECT * FROM \"Mixed\".\"Users\"", &tree),
            Some(("Mixed".to_owned(), "Users".to_owned()))
        );
        assert_eq!(
            resolve_full_table_select("SELECT * FROM \"Users\"", &tree),
            Some(("Mixed".to_owned(), "Users".to_owned()))
        );
        // Unquoted identifiers fold to lower case, so `Users` no longer matches.
        assert_eq!(
            resolve_full_table_select("SELECT * FROM Users", &tree),
            None
        );
    }
}
