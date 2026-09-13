//! Statement targeting for the query editor.
//!
//! Ports `apps/desktop/ui/src/sqlSelection.ts` so the native workbench runs the
//! same statement under the cursor, or the exact selection, as the Tauri app.
//! Ranges are character offsets (not bytes) to match GTK text buffer positions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Selection,
    Statement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlTarget {
    pub from: usize,
    pub to: usize,
    pub sql: String,
    pub kind: TargetKind,
}

pub fn sql_execution_target(text: &str, from: usize, to: usize) -> Option<SqlTarget> {
    let characters: Vec<char> = text.chars().collect();
    let from = clamp(from.min(to), 0, characters.len());
    let to = clamp(from.max(to), 0, characters.len());
    let range = if from != to {
        trim_range(&characters, from, to)
    } else {
        statement_range_at_cursor(&characters, from)
    };
    let (range_from, range_to) = range?;
    Some(SqlTarget {
        from: range_from,
        to: range_to,
        sql: characters[range_from..range_to].iter().collect(),
        kind: if from != to {
            TargetKind::Selection
        } else {
            TargetKind::Statement
        },
    })
}

pub fn line_execution_target(text: &str, from: usize, to: usize) -> Option<SqlTarget> {
    let characters: Vec<char> = text.chars().collect();
    let from = clamp(from.min(to), 0, characters.len());
    let to = clamp(from.max(to), 0, characters.len());
    let range = if from != to {
        trim_range(&characters, from, to)
    } else {
        line_range_at_cursor(&characters, from)
    };
    let (range_from, range_to) = range?;
    Some(SqlTarget {
        from: range_from,
        to: range_to,
        sql: characters[range_from..range_to].iter().collect(),
        kind: if from != to {
            TargetKind::Selection
        } else {
            TargetKind::Statement
        },
    })
}

fn statement_range_at_cursor(characters: &[char], cursor: usize) -> Option<(usize, usize)> {
    let ranges = statement_ranges(characters);
    let mut range_index = ranges
        .iter()
        .enumerate()
        .find(|(index, range)| cursor < range.1 || *index == ranges.len() - 1)
        .map(|(index, _)| index)?;

    if cursor > 0 && characters[cursor - 1] == ';' && range_index > 0 {
        range_index -= 1;
    }

    if let Some(selected) = trim_range(characters, ranges[range_index].0, ranges[range_index].1) {
        return Some(selected);
    }
    for range in &ranges[range_index + 1..] {
        if let Some(next) = trim_range(characters, range.0, range.1) {
            return Some(next);
        }
    }
    for range in ranges[..range_index].iter().rev() {
        if let Some(previous) = trim_range(characters, range.0, range.1) {
            return Some(previous);
        }
    }
    None
}

fn statement_ranges(characters: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut statement_start = 0;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut line_comment = false;
    let mut block_comment_depth = 0_usize;
    let mut dollar_quote: Option<String> = None;
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();

        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }

        if block_comment_depth > 0 {
            if character == '/' && next == Some('*') {
                block_comment_depth += 1;
                index += 2;
            } else if character == '*' && next == Some('/') {
                block_comment_depth -= 1;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if let Some(delimiter) = &dollar_quote {
            if starts_with_at(characters, index, delimiter) {
                index += delimiter.chars().count();
                dollar_quote = None;
            } else {
                index += 1;
            }
            continue;
        }

        if single_quoted {
            if character == '\\' || (character == '\'' && next == Some('\'')) {
                index += 2;
            } else {
                if character == '\'' {
                    single_quoted = false;
                }
                index += 1;
            }
            continue;
        }

        if double_quoted {
            if character == '"' && next == Some('"') {
                index += 2;
            } else {
                if character == '"' {
                    double_quoted = false;
                }
                index += 1;
            }
            continue;
        }

        if character == '-' && next == Some('-') {
            line_comment = true;
            index += 2;
        } else if character == '/' && next == Some('*') {
            block_comment_depth = 1;
            index += 2;
        } else if character == '\'' {
            single_quoted = true;
            index += 1;
        } else if character == '"' {
            double_quoted = true;
            index += 1;
        } else if character == '$' {
            match dollar_quote_delimiter(characters, index) {
                Some(delimiter) => {
                    index += delimiter.chars().count();
                    dollar_quote = Some(delimiter);
                }
                None => index += 1,
            }
        } else if character == ';' {
            ranges.push((statement_start, index + 1));
            statement_start = index + 1;
            index += 1;
        } else {
            index += 1;
        }
    }

    ranges.push((statement_start, characters.len()));
    ranges
}

fn line_range_at_cursor(characters: &[char], cursor: usize) -> Option<(usize, usize)> {
    let ranges = line_ranges(characters);
    let mut range_index = ranges
        .iter()
        .enumerate()
        .find(|(index, range)| cursor < range.1 || *index == ranges.len() - 1)
        .map(|(index, _)| index)?;
    if cursor > 0 && characters[cursor - 1] == '\n' && range_index > 0 {
        range_index -= 1;
    }
    if let Some(selected) = trim_range(characters, ranges[range_index].0, ranges[range_index].1) {
        return Some(selected);
    }
    for range in &ranges[range_index + 1..] {
        if let Some(next) = trim_range(characters, range.0, range.1) {
            return Some(next);
        }
    }
    for range in ranges[..range_index].iter().rev() {
        if let Some(previous) = trim_range(characters, range.0, range.1) {
            return Some(previous);
        }
    }
    None
}

fn line_ranges(characters: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut line_start = 0;
    for (index, character) in characters.iter().enumerate() {
        if *character == '\n' {
            ranges.push((line_start, index));
            line_start = index + 1;
        }
    }
    ranges.push((line_start, characters.len()));
    ranges
}

fn dollar_quote_delimiter(characters: &[char], start: usize) -> Option<String> {
    let end = characters[start + 1..]
        .iter()
        .position(|character| *character == '$')?
        + start
        + 1;
    let tag: String = characters[start + 1..end].iter().collect();
    let valid_tag = tag.is_empty()
        || (tag
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && tag
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'));
    if !valid_tag {
        return None;
    }
    Some(characters[start..=end].iter().collect())
}

fn starts_with_at(characters: &[char], index: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    characters.len() >= index + needle.len()
        && characters[index..index + needle.len()] == needle[..]
}

fn trim_range(characters: &[char], from: usize, to: usize) -> Option<(usize, usize)> {
    let mut from = from;
    let mut to = to;
    while from < to && characters[from].is_whitespace() {
        from += 1;
    }
    while to > from && characters[to - 1].is_whitespace() {
        to -= 1;
    }
    (from < to).then_some((from, to))
}

fn clamp(value: usize, minimum: usize, maximum: usize) -> usize {
    value.max(minimum).min(maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn runs_the_selected_sql_exactly_instead_of_the_entire_editor() {
        let sql = "SELECT now();\n\nSELECT 1;";
        let target = sql_execution_target(sql, 15, sql.chars().count()).expect("target");
        assert_eq!(target.from, 15);
        assert_eq!(target.to, sql.chars().count());
        assert_eq!(target.sql, "SELECT 1;");
        assert_eq!(target.kind, TargetKind::Selection);
    }

    #[test]
    fn runs_the_statement_at_the_cursor_when_there_is_no_selection() {
        let sql = "SELECT now();\n\nSELECT 1;";
        let characters = chars(sql);
        let one = characters
            .iter()
            .position(|character| *character == '1')
            .unwrap();
        let semicolon = characters
            .iter()
            .position(|character| *character == ';')
            .unwrap();
        assert_eq!(
            sql_execution_target(sql, one, one).map(|target| target.sql),
            Some("SELECT 1;".to_owned())
        );
        assert_eq!(
            sql_execution_target(sql, semicolon + 1, semicolon + 1).map(|target| target.sql),
            Some("SELECT now();".to_owned())
        );
        let target = sql_execution_target(sql, one, one).expect("target");
        assert_eq!(target.from, 15);
        assert_eq!(target.to, characters.len());
        assert_eq!(target.sql, "SELECT 1;");
        assert_eq!(target.kind, TargetKind::Statement);
    }

    #[test]
    fn ignores_semicolons_inside_postgres_strings_identifiers_comments_and_dollar_quotes() {
        let sql = [
            "SELECT ';' AS \"semi;colon\";",
            "-- comment ;",
            "SELECT $$body;still body$$;",
            "/* outer ; /* nested ; */ done */ SELECT 3;",
        ]
        .join("\n");
        let characters = chars(&sql);
        let body = characters
            .windows(4)
            .position(|window| window == ['b', 'o', 'd', 'y'])
            .unwrap();
        assert!(
            sql_execution_target(&sql, body, body)
                .is_some_and(|target| target.sql.contains("SELECT $$body;still body$$;"))
        );
        let three = characters
            .iter()
            .rposition(|character| *character == '3')
            .unwrap();
        assert!(
            sql_execution_target(&sql, three, three)
                .is_some_and(|target| target.sql.contains("SELECT 3;"))
        );
    }

    #[test]
    fn runs_the_current_redis_command_line_when_there_is_no_selection() {
        let text = "PING\n\nGET greeting";
        let target = line_execution_target(text, 0, 0).expect("target");
        assert_eq!(target.from, 0);
        assert_eq!(target.to, 4);
        assert_eq!(target.sql, "PING");
        assert_eq!(target.kind, TargetKind::Statement);

        let characters = chars(text);
        let get = characters
            .windows(3)
            .position(|window| window == ['G', 'E', 'T'])
            .unwrap();
        let target = line_execution_target(text, get, get).expect("target");
        assert_eq!(target.from, 6);
        assert_eq!(target.to, characters.len());
        assert_eq!(target.sql, "GET greeting");
    }

    #[test]
    fn character_offsets_survive_multibyte_content() {
        let sql = "SELECT 'héllo';";
        let target = sql_execution_target(sql, 8, 8).expect("target");
        assert_eq!(target.sql, "SELECT 'héllo';");
    }
}
