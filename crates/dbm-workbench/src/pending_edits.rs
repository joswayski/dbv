//! Local table drafts. Database writes only happen when a frontend submits
//! `mutations()` through the engine's existing apply_mutations operation.

use std::collections::BTreeMap;

use dbm_engine::models::{RowMutation, TableMetadata};
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct PendingEdits {
    rows: BTreeMap<String, RowMutation>,
}

impl PendingEdits {
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn clear(&mut self) {
        self.rows.clear();
    }
    pub fn delete_count(&self) -> usize {
        self.rows.values().filter(|row| row.deleted).count()
    }
    pub fn get(&self, metadata: &TableMetadata, row: &[Value]) -> Option<&RowMutation> {
        self.rows.get(&row_key(metadata, row)?)
    }
    pub fn mutations(&self) -> Vec<RowMutation> {
        self.rows.values().cloned().collect()
    }
    pub fn values(&self, metadata: &TableMetadata, row: &[Value]) -> Vec<Value> {
        self.get(metadata, row).map_or_else(
            || row.iter().take(metadata.columns.len()).cloned().collect(),
            |pending| pending.changes.clone(),
        )
    }

    pub fn set_cell(
        &mut self,
        metadata: &TableMetadata,
        row: &[Value],
        column: usize,
        text: &str,
    ) -> Result<(), String> {
        let field = metadata.columns.get(column).ok_or("Unknown column")?;
        if metadata.primary_key.contains(&field.name) {
            return Err("Primary-key columns cannot be edited".into());
        }
        let key =
            row_key(metadata, row).ok_or("A complete primary key is required to edit rows")?;
        let mut pending = self
            .rows
            .get(&key)
            .cloned()
            .map_or_else(|| snapshot(metadata, row), Ok)?;
        if pending.deleted {
            return Err("Undo the pending deletion before editing this row".into());
        }
        pending.changes[column] = parse_cell(text);
        if pending.changes == pending.original {
            self.rows.remove(&key);
        } else {
            self.rows.insert(key, pending);
        }
        Ok(())
    }

    /// Mixed selections all become deletions. An entirely deleted selection
    /// is restored, retaining edits that were staged before deletion.
    pub fn toggle_delete(
        &mut self,
        metadata: &TableMetadata,
        rows: &[Vec<Value>],
    ) -> Result<(), String> {
        let drafts = rows
            .iter()
            .map(|row| {
                let key = row_key(metadata, row)
                    .ok_or("A complete primary key is required to delete rows")?;
                let pending = self
                    .rows
                    .get(&key)
                    .cloned()
                    .map_or_else(|| snapshot(metadata, row), Ok)?;
                Ok((key, pending))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let deleted = !drafts.iter().all(|(_, pending)| pending.deleted);
        for (key, mut pending) in drafts {
            pending.deleted = deleted;
            if !deleted && pending.changes == pending.original {
                self.rows.remove(&key);
            } else {
                self.rows.insert(key, pending);
            }
        }
        Ok(())
    }

    /// Undo delete first; a second discard removes any underlying edits.
    pub fn discard_row(&mut self, metadata: &TableMetadata, row: &[Value]) {
        let Some(key) = row_key(metadata, row) else {
            return;
        };
        if let Some(pending) = self.rows.get_mut(&key)
            && pending.deleted
            && pending.changes != pending.original
        {
            pending.deleted = false;
            return;
        }
        self.rows.remove(&key);
    }
}

pub fn row_key(metadata: &TableMetadata, row: &[Value]) -> Option<String> {
    if metadata.primary_key.is_empty() {
        return None;
    }
    let keys = metadata
        .primary_key
        .iter()
        .map(|key| {
            let index = metadata
                .columns
                .iter()
                .position(|column| &column.name == key)?;
            row.get(index).filter(|value| !value.is_null())
        })
        .collect::<Option<Vec<_>>>()?;
    Some(serde_json::to_string(&keys).expect("JSON values serialize"))
}

fn snapshot(metadata: &TableMetadata, row: &[Value]) -> Result<RowMutation, String> {
    if row.len() < metadata.columns.len() {
        return Err("Incomplete table row".into());
    }
    let primary_key = metadata
        .primary_key
        .iter()
        .map(|key| {
            let index = metadata
                .columns
                .iter()
                .position(|column| &column.name == key)
                .ok_or("Unknown primary-key column")?;
            Ok(row[index].clone())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let original = row[..metadata.columns.len()].to_vec();
    let xmin = if metadata.has_xmin {
        match row.get(metadata.columns.len()) {
            Some(Value::String(value)) => Some(value.clone()),
            Some(Value::Number(value)) => Some(value.to_string()),
            _ => return Err("The row version is missing; refresh before editing".into()),
        }
    } else {
        None
    };
    Ok(RowMutation {
        changes: original.clone(),
        original,
        primary_key,
        xmin,
        deleted: false,
    })
}

/// Mirrors the React cell editor: blank → NULL, exact boolean literals and
/// plain decimal numbers → typed values; everything else stays a string.
pub fn parse_cell(text: &str) -> Value {
    if text.trim().is_empty() {
        return Value::Null;
    }
    if text == "true" {
        return Value::Bool(true);
    }
    if text == "false" {
        return Value::Bool(false);
    }
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let parts: Vec<_> = unsigned.split('.').collect();
    if parts.len() <= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        if let Ok(value) = text.parse::<i64>() {
            return value.into();
        }
        if let Ok(value) = text.parse::<u64>() {
            return value.into();
        }
        if let Ok(value) = text.parse::<f64>()
            && let Some(number) = serde_json::Number::from_f64(value)
        {
            return Value::Number(number);
        }
    }
    Value::String(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_engine::models::TableColumn;
    use serde_json::json;

    fn metadata() -> TableMetadata {
        TableMetadata {
            schema: "public".into(),
            table: "products".into(),
            columns: ["tenant", "id", "name", "price"]
                .into_iter()
                .enumerate()
                .map(|(ordinal, name)| TableColumn {
                    name: name.into(),
                    data_type: "text".into(),
                    nullable: true,
                    default_value: None,
                    ordinal: ordinal as i32,
                })
                .collect(),
            primary_key: vec!["id".into(), "tenant".into()],
            has_xmin: true,
        }
    }

    #[test]
    fn drafts_preserve_original_key_order_and_version_across_reloads() {
        let metadata = metadata();
        let row = vec![json!(9), json!(4), json!("Canvas"), json!(85), json!("72")];
        let mut drafts = PendingEdits::default();
        drafts
            .set_cell(&metadata, &row, 2, "Canvas low top")
            .unwrap();
        let reloaded = vec![
            json!(9),
            json!(4),
            json!("Someone else"),
            json!(90),
            json!("73"),
        ];
        drafts.set_cell(&metadata, &reloaded, 3, "95").unwrap();
        let mutation = &drafts.mutations()[0];
        assert_eq!(mutation.original, row[..4]);
        assert_eq!(
            mutation.changes,
            vec![json!(9), json!(4), json!("Canvas low top"), json!(95)]
        );
        assert_eq!(mutation.primary_key, vec![json!(4), json!(9)]);
        assert_eq!(mutation.xmin.as_deref(), Some("72"));
        assert!(drafts.set_cell(&metadata, &row, 1, "5").is_err());
        assert_eq!(drafts.len(), 1);
    }

    #[test]
    fn mixed_delete_and_undo_preserve_edits_and_remove_unchanged_drafts() {
        let metadata = metadata();
        let a = vec![json!(9), json!(4), json!("A"), json!(85), json!("72")];
        let b = vec![json!(9), json!(5), json!("B"), json!(90), json!("73")];
        let mut drafts = PendingEdits::default();
        drafts.set_cell(&metadata, &a, 2, "Edited A").unwrap();
        drafts
            .toggle_delete(&metadata, std::slice::from_ref(&b))
            .unwrap();
        drafts
            .toggle_delete(&metadata, &[a.clone(), b.clone()])
            .unwrap();
        assert_eq!(drafts.delete_count(), 2);
        assert!(
            drafts
                .set_cell(&metadata, &a, 2, "Cannot edit deleted")
                .is_err()
        );
        drafts.discard_row(&metadata, &a);
        assert_eq!(drafts.values(&metadata, &a)[2], json!("Edited A"));
        drafts.toggle_delete(&metadata, &[b]).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts.delete_count(), 0);
        drafts.set_cell(&metadata, &a, 2, "A").unwrap();
        assert!(drafts.is_empty());
    }

    #[test]
    fn unsafe_rows_cannot_be_staged() {
        let mut metadata = metadata();
        let mut drafts = PendingEdits::default();
        let row = vec![json!(9), json!(4), json!("A"), json!(85)];
        assert!(drafts.set_cell(&metadata, &row, 2, "B").is_err());
        metadata.has_xmin = false;
        drafts.set_cell(&metadata, &row, 2, "B").unwrap();
        assert_eq!(drafts.mutations()[0].xmin, None);
        metadata.primary_key.clear();
        assert!(drafts.toggle_delete(&metadata, &[row]).is_err());
    }

    #[test]
    fn parser_distinguishes_literals_from_text() {
        for (input, expected) in [
            (" ", Value::Null),
            ("false", json!(false)),
            (" false ", json!(" false ")),
            ("-12.50", json!(-12.5)),
            ("0012", json!(12)),
            ("1e3", json!("1e3")),
            (".5", json!(".5")),
            ("NULL", json!("NULL")),
            ("{\"a\":1}", json!("{\"a\":1}")),
        ] {
            assert_eq!(parse_cell(input), expected, "{input}");
        }
    }
}
