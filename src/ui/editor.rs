//! SQL editor: syntax highlighting, line numbers, current-statement tint, autocomplete,
//! auto-closing brackets, keyword upper-casing and comment toggling.

use super::p;
use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Color32, FontId, Key, Modifiers, TextFormat, Ui};
use std::collections::HashMap;

pub const KEYWORDS: &[&str] = &[
    "ADD", "ALL", "ALTER", "ANALYZE", "AND", "ANY", "AS", "ASC", "AUTO_INCREMENT", "BEGIN", "BETWEEN", "BIGINT", "BOOLEAN", "BY", "CASCADE", "CASE",
    "CAST", "CHAR", "CHECK", "COLLATE", "COLUMN", "COMMENT", "COMMIT", "CONCURRENTLY", "CONFLICT", "CONSTRAINT", "CREATE", "CROSS", "CURRENT_DATE",
    "CURRENT_TIMESTAMP", "DATABASE", "DATE", "DECIMAL", "DEFAULT", "DELETE", "DESC", "DESCRIBE", "DISTINCT", "DO", "DROP", "ELSE", "END", "ENGINE",
    "EXCEPT", "EXISTS", "EXPLAIN", "EXTENSION", "FALSE", "FETCH", "FILTER", "FIRST", "FLOAT", "FOR", "FOREIGN", "FROM", "FULL", "FUNCTION", "GRANT",
    "GROUP", "HAVING", "IF", "ILIKE", "IN", "INDEX", "INNER", "INSERT", "INT", "INTEGER", "INTERSECT", "INTERVAL", "INTO", "IS", "JOIN", "JSON",
    "JSONB", "KEY", "LAST", "LATERAL", "LEFT", "LIKE", "LIMIT", "MATERIALIZED", "MERGE", "NATURAL", "NEXT", "NOT", "NOTHING", "NULL", "NULLS",
    "NUMERIC", "OFFSET", "ON", "ONLY", "OR", "ORDER", "OUTER", "OVER", "PARTITION", "PRAGMA", "PRIMARY", "PROCEDURE", "REAL", "RECURSIVE",
    "REFERENCES", "REFRESH", "RENAME", "REPLACE", "RESTRICT", "RETURNING", "RETURNS", "REVOKE", "RIGHT", "ROLLBACK", "ROW", "ROWS", "SCHEMA",
    "SELECT", "SEQUENCE", "SERIAL", "SET", "SHOW", "SMALLINT", "TABLE", "TEMPORARY", "TEXT", "THEN", "TIME", "TIMESTAMP", "TIMESTAMPTZ", "TO",
    "TOP", "TRANSACTION", "TRIGGER", "TRUE", "TRUNCATE", "UNION", "UNIQUE", "UPDATE", "USE", "USING", "UUID", "VACUUM", "VALUES", "VARCHAR",
    "VIEW", "WHEN", "WHERE", "WINDOW", "WITH",
];

pub const FUNCTIONS: &[&str] = &[
    "ABS", "AVG", "COALESCE", "CONCAT", "COUNT", "DATE_TRUNC", "EXTRACT", "GREATEST", "IFNULL", "JSON_AGG", "JSON_EXTRACT", "LEAST", "LENGTH",
    "LOWER", "MAX", "MIN", "NOW", "NULLIF", "ROUND", "ROW_NUMBER", "STRING_AGG", "SUBSTRING", "SUM", "TO_CHAR", "TRIM", "UPPER",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Plain,
    Keyword,
    Function,
    Single,
    Double,
    Backtick,
    Bracket,
    Number,
    Comment,
    Param,
}

fn is_keyword(w: &str) -> bool {
    let up = w.to_ascii_uppercase();
    KEYWORDS.binary_search(&up.as_str()).is_ok()
}

/// Byte ranges with their token class.
fn tokenize(s: &str) -> Vec<(usize, usize, Tok)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let c = b[i];
        let tok = if c == b'-' && b.get(i + 1) == Some(&b'-') || c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            Tok::Comment
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            Tok::Comment
        } else if c == b'[' {
            i += 1;
            while i < b.len() {
                if b[i] == b']' {
                    i += 1;
                    if b.get(i) == Some(&b']') { i += 1; }
                    else { break; }
                } else { i += 1; }
            }
            Tok::Bracket
        } else if c == b'\'' || c == b'"' || c == b'`' {
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' && c == b'\'' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            match c {
                b'\'' => Tok::Single,
                b'"' => Tok::Double,
                _ => Tok::Backtick,
            }
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            Tok::Number
        } else if (c == b':' && b.get(i + 1).map(|n| n.is_ascii_alphabetic()).unwrap_or(false) && (i == 0 || b[i - 1] != b':')) || (c == b'$' && b.get(i + 1).map(|n| n.is_ascii_digit()).unwrap_or(false)) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Tok::Param
        } else if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] >= 0x80) {
                i += 1;
            }
            let word = &s[start..i];
            let mut j = i;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if is_keyword(word) {
                Tok::Keyword
            } else if b.get(j) == Some(&b'(') {
                Tok::Function
            } else {
                Tok::Plain
            }
        } else {
            // Advance by a whole UTF-8 char.
            i += s[i..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
            Tok::Plain
        };
        out.push((start, i, tok));
    }
    out
}

pub fn highlight(text: &str, font: FontId, current: Option<(usize, usize)>) -> LayoutJob {
    let pal = p();
    let mut job = LayoutJob::default();
    for (s, e, t) in tokenize(text) {
        let color = match t {
            Tok::Plain => pal.editor_text,
            Tok::Keyword => pal.editor_keyword,
            Tok::Function => pal.editor_function,
            Tok::Single => pal.editor_string,
            Tok::Double | Tok::Bracket => pal.editor_dquote,
            Tok::Backtick => pal.editor_backtick,
            Tok::Number => pal.editor_number,
            Tok::Comment => pal.editor_comment,
            Tok::Param => pal.warning,
        };
        let background = match current {
            Some((cs, ce)) if s >= cs && e <= ce => pal.editor_current_query,
            _ => Color32::TRANSPARENT,
        };
        // Keywords stand out by weight, not colour; Spline Sans Mono Medium keeps the same advance.
        let font_id = if t == Tok::Keyword { super::mono_medium(font.size) } else { font.clone() };
        job.append(&text[s..e], 0.0, TextFormat { font_id, color, background, italics: t == Tok::Comment, ..Default::default() });
    }
    job
}

/// Names available to the editor, including cached SQL Server namespaces.
#[derive(Default, Clone)]
pub struct Catalog {
    pub tables: Vec<String>,
    pub columns: HashMap<String, Vec<String>>,
    pub databases: Vec<String>,
    pub current_database: String,
    pub schemas: Vec<String>,
    pub schema: String,
    pub namespaces: HashMap<Vec<String>, Vec<Suggestion>>,
    pub failed_databases: std::collections::HashSet<String>,
}

impl Catalog {
    pub fn cache_database(&mut self, database: &str, rows: &crate::db::QueryResult) {
        let root = database.to_lowercase();
        self.namespaces.retain(|key, _| key.first() != Some(&root));
        self.failed_databases.remove(&root);
        self.namespaces.insert(vec![root.clone()], vec![]);
        let mut seen = std::collections::HashSet::new();
        let mut add = |key: Vec<String>, text: &str, kind| {
            let items = self.namespaces.entry(key.clone()).or_default();
            if seen.insert((key, text.to_string())) { items.push(Suggestion { text: text.to_string(), kind }); }
        };
        for row in &rows.rows {
            let Some(schema) = row.first().and_then(|v| v.as_deref()) else { continue };
            add(vec![root.clone()], schema, "schema");
            let Some(table) = row.get(1).and_then(|v| v.as_deref()) else { continue };
            let schema_key = vec![root.clone(), schema.to_lowercase()];
            add(schema_key.clone(), table, "table");
            if let Some(column) = row.get(2).and_then(|v| v.as_deref()) {
                let mut table_key = schema_key;
                table_key.push(table.to_lowercase());
                add(table_key, column, "column");
            }
        }
    }

    fn database_request(&self, text: &str, prefix: &str) -> Option<String> {
        let parts = identifier_parts(prefix);
        if parts.len() < 2 { return None; }
        let alias = resolve_alias(text, &parts[0]);
        let resolved = alias.as_deref().map(identifier_parts);
        let root = resolved.as_ref().unwrap_or(&parts).first()?;
        let database = self.databases.iter().find(|name| name.eq_ignore_ascii_case(root)).or_else(|| {
            (self.schemas.iter().any(|name| name.eq_ignore_ascii_case(root)) || self.tables.iter().any(|name| name.eq_ignore_ascii_case(root)))
                .then(|| self.databases.iter().find(|name| name.eq_ignore_ascii_case(&self.current_database))).flatten()
        })?;
        let key = database.to_lowercase();
        (!self.namespaces.contains_key(&vec![key.clone()]) && !self.failed_databases.contains(&key)).then(|| database.clone())
    }

    fn namespace(&self, qualifier: &[String]) -> Option<Vec<Suggestion>> {
        let key: Vec<_> = qualifier.iter().map(|part| part.to_lowercase()).collect();
        let mut local_key = vec![self.current_database.to_lowercase()];
        local_key.extend(key.clone());
        if let Some(items) = self.namespaces.get(&key).or_else(|| self.namespaces.get(&local_key)) {
            return Some(items.clone());
        }
        if !self.current_database.is_empty() && key == [self.current_database.to_lowercase()] {
            return Some(self.schemas.iter().map(|name| Suggestion { text: name.clone(), kind: "schema" }).collect());
        }
        if !self.schema.is_empty() && (key == [self.schema.to_lowercase()] || key == [self.current_database.to_lowercase(), self.schema.to_lowercase()]) {
            return Some(self.tables.iter().map(|name| Suggestion { text: name.clone(), kind: "table" }).collect());
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub text: String,
    pub kind: &'static str,
}

struct Completion {
    start_char: usize,
    items: Vec<Suggestion>,
    selected: usize,
    allow_empty: bool,
    bracketed: bool,
}

pub struct SqlEditor {
    pub id: egui::Id,
    completion: Option<Completion>,
    pub cursor_char: usize,
    pub selection_chars: Option<(usize, usize)>,
    pending_cursor: Option<(usize, usize)>,
    dismissed_at: Option<(usize, String)>,
    catalog_pending: bool,
}

#[derive(Default)]
pub struct EditorEvents {
    pub run_current: bool,
    pub run_all: bool,
    pub changed: bool,
    pub completion_database: Option<String>,
}

fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

fn byte_to_char(s: &str, bi: usize) -> usize {
    s[..bi.min(s.len())].chars().count()
}

/// Resolves `alias` to a table name using FROM/JOIN clauses in the text.
fn resolve_alias(text: &str, alias: &str) -> Option<String> {
    let ident = r#"(?:\[(?:[^\]]|\]\])*\]|"(?:[^"]|"")*"|`(?:[^`]|``)*`|[\w$#@]+)"#;
    let re = regex::Regex::new(&format!(r#"(?i)\b(?:from|join|update|into)\s+({ident}(?:\.{ident})*)(?:\s+(?:as\s+)?({ident}))?"#)).ok()?;
    for cap in re.captures_iter(text) {
        let table = cap.get(1)?.as_str();
        let parts = identifier_parts(table);
        let a = cap.get(2).map(|m| identifier_parts(m.as_str()).join("."));
        if a.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(alias)) || parts.last().is_some_and(|short| short.eq_ignore_ascii_case(alias)) {
            return Some(table.to_string());
        }
    }
    None
}

/// Split on qualification dots while preserving dots and escaped brackets inside identifiers.
fn identifier_parts(name: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut quote = None;
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(close) = quote {
            if c == close {
                if chars.peek() == Some(&close) { chars.next(); parts.last_mut().unwrap().push(close); }
                else { quote = None; }
            } else { parts.last_mut().unwrap().push(c); }
        } else {
            match c {
                '[' => quote = Some(']'),
                '"' | '`' => quote = Some(c),
                '.' => parts.push(String::new()),
                _ => parts.last_mut().unwrap().push(c),
            }
        }
    }
    parts
}

fn completion_prefix(before: &str, sql_server: bool) -> (usize, usize) {
    let mut start = 0;
    let mut part_start = 0;
    for (s, e, tok) in tokenize(before) {
        if matches!(tok, Tok::Single | Tok::Double | Tok::Backtick | Tok::Comment) {
            start = e;
            part_start = e;
        } else if !(sql_server && tok == Tok::Bracket) {
            for (offset, c) in before[s..e].char_indices() {
                let i = s + offset;
                if c == '.' { part_start = i + 1; }
                else if !(c.is_alphanumeric() || c == '_' || (sql_server && "@$#".contains(c))) {
                    start = i + c.len_utf8();
                    part_start = start;
                }
            }
        }
    }
    (start, part_start)
}

fn completion_text(suggestion: &Suggestion, catalog: &Catalog, bracketed: bool) -> String {
    if !catalog.databases.is_empty() && matches!(suggestion.kind, "database" | "schema" | "table" | "column") {
        let mut chars = suggestion.text.chars();
        let regular = chars.next().is_some_and(|c| c.is_alphabetic() || "_@#".contains(c)) && chars.all(|c| c.is_alphanumeric() || "_@$#".contains(c));
        if bracketed || !regular || is_keyword(&suggestion.text) {
            return crate::db::sqlgen::quote_ident(crate::model::Dialect::SqlServer, &suggestion.text);
        }
    }
    suggestion.text.clone()
}

pub fn suggestions(text: &str, prefix: &str, catalog: &Catalog) -> Vec<Suggestion> {
    let parts = identifier_parts(prefix);
    if parts.len() > 1 {
        let (part, qualifier) = parts.split_last().unwrap();
        if let Some(items) = catalog.namespace(qualifier) { return rank(items.into_iter(), part); }
        let table = resolve_alias(text, &qualifier.join(".")).map(|name| identifier_parts(&name)).unwrap_or_else(|| qualifier.to_vec());
        if let Some(items) = catalog.namespace(&table) { return rank(items.into_iter(), part); }
        let current = match table.as_slice() {
            [_] => true,
            [schema, _] => schema.eq_ignore_ascii_case(&catalog.schema),
            [database, schema, _] => database.eq_ignore_ascii_case(&catalog.current_database) && schema.eq_ignore_ascii_case(&catalog.schema),
            _ => false,
        };
        let short = if catalog.databases.is_empty() || current { table.last().map(String::as_str).unwrap_or("") } else { "" };
        let cols = catalog.columns.iter().find(|(t, _)| t.eq_ignore_ascii_case(short)).map(|(_, c)| c.clone()).unwrap_or_default();
        return rank(cols.iter().map(|c| Suggestion { text: c.clone(), kind: "column" }), part);
    }
    let mentioned: Vec<String> = catalog.tables.iter().filter(|t| text.to_lowercase().contains(&t.to_lowercase())).cloned().collect();
    let mut pool: Vec<Suggestion> = Vec::new();
    pool.extend(catalog.databases.iter().map(|name| Suggestion { text: name.clone(), kind: "database" }));
    pool.extend(catalog.schemas.iter().map(|name| Suggestion { text: name.clone(), kind: "schema" }));
    for t in &mentioned {
        if let Some(cols) = catalog.columns.get(t) {
            pool.extend(cols.iter().map(|c| Suggestion { text: c.clone(), kind: "column" }));
        }
    }
    pool.extend(catalog.tables.iter().map(|t| Suggestion { text: t.clone(), kind: "table" }));
    pool.extend(KEYWORDS.iter().map(|k| Suggestion { text: k.to_string(), kind: "keyword" }));
    pool.extend(FUNCTIONS.iter().map(|k| Suggestion { text: format!("{k}()"), kind: "function" }));
    rank(pool.into_iter(), &parts[0])
}

fn rank(items: impl Iterator<Item = Suggestion>, prefix: &str) -> Vec<Suggestion> {
    let lp = prefix.to_lowercase();
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for s in items {
        let l = s.text.to_lowercase();
        if l == lp {
            return vec![s];
        }
        if l.starts_with(&lp) {
            starts.push(s);
        } else if lp.len() >= 2 && l.contains(&lp) {
            contains.push(s);
        }
    }
    starts.extend(contains);
    let mut seen = std::collections::HashSet::new();
    starts.retain(|s| seen.insert(s.text.clone()));
    starts.truncate(40);
    starts
}

impl SqlEditor {
    pub fn new(id: egui::Id) -> Self {
        Self { id, completion: None, cursor_char: 0, selection_chars: None, pending_cursor: None, dismissed_at: None, catalog_pending: false }
    }

    /// Byte range of the statement under the cursor.
    pub fn current_statement(&self, text: &str) -> Option<(usize, usize)> {
        let cursor = char_to_byte(text, self.cursor_char);
        let spans = crate::db::split_statements(text);
        spans.iter().copied().find(|(s, e)| cursor >= *s && cursor <= *e + 1).or_else(|| spans.iter().copied().filter(|(_, e)| *e < cursor).last())
    }

    pub fn selected_text<'a>(&self, text: &'a str) -> Option<&'a str> {
        let (a, b) = self.selection_chars?;
        let (a, b) = (char_to_byte(text, a.min(b)), char_to_byte(text, a.max(b)));
        let s = &text[a..b];
        (!s.trim().is_empty()).then_some(s)
    }

    fn toggle_comment(&mut self, text: &mut String) {
        let (a, b) = self.selection_chars.map(|(a, b)| (a.min(b), a.max(b))).unwrap_or((self.cursor_char, self.cursor_char));
        let (ab, bb) = (char_to_byte(text, a), char_to_byte(text, b));
        let line_start = text[..ab].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = text[bb..].find('\n').map(|i| bb + i).unwrap_or(text.len());
        let block = &text[line_start..line_end];
        let all_commented = block.lines().filter(|l| !l.trim().is_empty()).all(|l| l.trim_start().starts_with("--"));
        let new_block: Vec<String> = block
            .lines()
            .map(|l| {
                if all_commented {
                    let idx = l.find("--").unwrap_or(0);
                    let rest = &l[idx + 2..];
                    format!("{}{}", &l[..idx], rest.strip_prefix(' ').unwrap_or(rest))
                } else if l.trim().is_empty() {
                    l.to_string()
                } else {
                    format!("-- {l}")
                }
            })
            .collect();
        let replaced = new_block.join("\n");
        text.replace_range(line_start..line_end, &replaced);
        let end_char = byte_to_char(text, line_start + replaced.len());
        self.pending_cursor = Some((byte_to_char(text, line_start), end_char));
    }

    pub fn format(&mut self, text: &mut String, uppercase: bool, indent: usize) {
        let opts = sqlformat::FormatOptions { indent: sqlformat::Indent::Spaces(indent as u8), uppercase: Some(uppercase), lines_between_queries: 2, ..Default::default() };
        *text = sqlformat::format(text, &sqlformat::QueryParams::None, &opts);
    }

    pub fn show(&mut self, ui: &mut Ui, text: &mut String, catalog: &Catalog, font_size: f32, uppercase_keywords: bool, auto_close: bool) -> EditorEvents {
        let pal = p();
        let mut ev = EditorEvents::default();
        let font = FontId::monospace(font_size);
        let has_focus = ui.memory(|m| m.has_focus(self.id));
        let explicit_completion = has_focus && ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Space));
        if explicit_completion { self.dismissed_at = None; }
        let previous_cursor = self.cursor_char;
        let mut accepted = false;
        let mut reveal_completion = false;

        if has_focus {
            ev.run_all = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter));
            ev.run_current = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Slash)) {
                self.toggle_comment(text);
                ev.changed = true;
            }
        }

        // Completion navigation must be consumed before the TextEdit sees the keys.
        let mut accept: Option<Suggestion> = None;
        if has_focus {
            if let Some(c) = &mut self.completion {
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                    c.selected = (c.selected + 1) % c.items.len().max(1);
                    reveal_completion = true;
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                    c.selected = (c.selected + c.items.len().max(1) - 1) % c.items.len().max(1);
                    reveal_completion = true;
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter)) {
                    accept = c.items.get(c.selected).cloned();
                }
                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    self.dismissed_at = Some((self.cursor_char, text.clone()));
                    self.completion = None;
                    self.catalog_pending = false;
                }
            }
        }
        if let Some((s, c)) = accept.and_then(|s| self.completion.take().map(|c| (s, c))) {
            let start = char_to_byte(text, c.start_char);
            let mut end = char_to_byte(text, self.cursor_char);
            if c.bracketed && text[end..].starts_with(']') { end += 1; }
            let replacement = completion_text(&s, catalog, c.bracketed);
            text.replace_range(start..end, &replacement);
            let mut caret = c.start_char + replacement.chars().count();
            if s.kind == "function" && s.text.ends_with("()") {
                caret -= 1;
            }
            self.pending_cursor = Some((caret, caret));
            self.dismissed_at = Some((caret, text.clone()));
            accepted = true;
            ev.changed = true;
        }

        let typed: Vec<String> = if has_focus {
            ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect())
        } else {
            vec![]
        };

        if let Some((a, b)) = self.pending_cursor.take() {
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), self.id) {
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                state.store(ui.ctx(), self.id);
            }
            ui.memory_mut(|m| m.request_focus(self.id));
        }

        let current = self.current_statement(text);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let job = highlight(buf.as_str(), font.clone(), current);
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let line_count = text.lines().count().max(1) + usize::from(text.ends_with('\n'));
        let gutter_w = 14.0 + (line_count.to_string().len() as f32) * font_size * 0.62;
        let mut output = None;
        egui::ScrollArea::both().id_salt(self.id.with("scroll")).auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (gutter, _) = ui.allocate_exact_size(egui::vec2(gutter_w, ui.available_height().max(10.0)), egui::Sense::hover());
                let te = egui::TextEdit::multiline(text)
                    .id(self.id)
                    .font(font.clone())
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .lock_focus(true)
                    .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
                    .desired_width(f32::INFINITY)
                    .desired_rows(12)
                    .layouter(&mut layouter);
                let out = te.show(ui);
                let galley = out.galley.clone();
                let mut line = 1;
                let mut start_of_line = true;
                for row in &galley.rows {
                    if start_of_line {
                        let y = out.galley_pos.y + row.pos.y;
                        let is_current = current.is_some();
                        let _ = is_current;
                        ui.painter().text(egui::pos2(gutter.right() - 8.0, y), egui::Align2::RIGHT_TOP, line.to_string(), font.clone(), pal.text_faint);
                        line += 1;
                    }
                    start_of_line = row.ends_with_newline;
                }
                output = Some(out);
            });
        });
        let Some(out) = output else { return ev };

        if out.response.changed() {
            ev.changed = true;
        }
        if let Some(r) = out.cursor_range {
            self.cursor_char = r.primary.index.0;
            self.selection_chars = (r.primary.index.0 != r.secondary.index.0).then_some((r.secondary.index.0, r.primary.index.0));
        }

        if out.response.changed() && !typed.is_empty() {
            let last = typed.last().cloned().unwrap_or_default();
            if auto_close && self.selection_chars.is_none() {
                let close = match last.as_str() {
                    "(" => Some(")"),
                    "[" => Some("]"),
                    "{" => Some("}"),
                    "'" => Some("'"),
                    "\"" => Some("\""),
                    "`" => Some("`"),
                    _ => None,
                };
                let at = char_to_byte(text, self.cursor_char);
                let next = text[at..].chars().next();
                if let Some(cl) = close {
                    let quote = matches!(last.as_str(), "'" | "\"" | "`");
                    let prev_is_word = text[..at.saturating_sub(1)].chars().last().map(|c| c.is_alphanumeric()).unwrap_or(false);
                    if next.map(|n| n.to_string() == cl).unwrap_or(false) && quote {
                        // Typed over an auto-inserted closing quote: drop the duplicate.
                        text.remove(at);
                    } else if !(quote && prev_is_word) && next.map(|n| n.is_whitespace() || ")]},;".contains(n)).unwrap_or(true) {
                        text.insert_str(at, cl);
                        self.pending_cursor = Some((self.cursor_char, self.cursor_char));
                    }
                } else if matches!(last.as_str(), ")" | "]" | "}") && next.map(|n| n.to_string() == last).unwrap_or(false) {
                    text.remove(at);
                }
            }
            if uppercase_keywords && last.chars().all(|c| c.is_whitespace() || c == '(' || c == ',' || c == ';') {
                let at = char_to_byte(text, self.cursor_char.saturating_sub(1));
                let before = &text[..at];
                let word_start = before.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map(|i| i + 1).unwrap_or(0);
                let word = &before[word_start..];
                let in_string = tokenize(before).last().map(|t| matches!(t.2, Tok::Single | Tok::Double | Tok::Comment)).unwrap_or(false);
                if !word.is_empty() && is_keyword(word) && word != word.to_ascii_uppercase() && !in_string {
                    let up = word.to_ascii_uppercase();
                    text.replace_range(word_start..at, &up);
                }
            }
        }

        // Refresh completion list from the word before the caret.
        let has_focus = out.response.has_focus();
        if has_focus && !accepted && (out.response.changed() || self.completion.is_some() || explicit_completion || self.catalog_pending) {
            let at = char_to_byte(text, self.cursor_char);
            let before = &text[..at];
            let (start, part_start) = completion_prefix(before, !catalog.databases.is_empty());
            let prefix = &before[start..];
            let in_string = tokenize(before).last().map(|t| matches!(t.2, Tok::Single | Tok::Double | Tok::Comment) && t.1 >= at).unwrap_or(false);
            let start_char = byte_to_char(text, part_start);
            let dismissed = self.dismissed_at.as_ref().is_some_and(|(cursor, value)| *cursor == self.cursor_char && value == text);
            let allow_empty = explicit_completion || (previous_cursor == self.cursor_char && !out.response.changed() && self.completion.as_ref().is_some_and(|c| c.allow_empty && c.start_char == start_char));
            if (prefix.is_empty() && !allow_empty) || in_string || dismissed || self.selection_chars.is_some() || prefix.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                self.completion = None;
                self.catalog_pending = false;
            } else {
                let statement = self.current_statement(text).map(|(s, e)| &text[s..e]).unwrap_or(text);
                ev.completion_database = catalog.database_request(statement, prefix);
                self.catalog_pending = ev.completion_database.is_some();
                let items = suggestions(statement, prefix, catalog);
                let keep = if out.response.changed() || previous_cursor != self.cursor_char { 0 } else { self.completion.as_ref().map(|c| c.selected).unwrap_or(0) };
                self.completion = (!items.is_empty()).then(|| Completion { start_char, selected: keep.min(items.len() - 1), items, allow_empty, bracketed: before[part_start..].starts_with('[') });
            }
        }
        let over_popup = ui.ctx().pointer_hover_pos().is_some_and(|pos| ui.ctx().layer_id_at(pos) == Some(egui::LayerId::new(egui::Order::Foreground, self.id.with("completion"))));
        if !has_focus && !over_popup {
            self.completion = None;
            self.catalog_pending = false;
        }

        // Popup under the caret.
        if let Some(c) = &mut self.completion {
            let caret = out.galley.pos_from_cursor(CCursor::new(self.cursor_char));
            let viewport = ui.ctx().content_rect().shrink(8.0);
            let kind_font = FontId::proportional((font_size - 2.0).max(10.0));
            let (label_width, kind_width, text_height) = ui.fonts_mut(|fonts| {
                let mut sizes = (0.0_f32, 0.0_f32, 0.0_f32);
                for s in &c.items {
                    let label = fonts.layout_no_wrap(s.text.clone(), font.clone(), pal.text).size();
                    let kind = fonts.layout_no_wrap(s.kind.into(), kind_font.clone(), pal.text).size();
                    sizes.0 = sizes.0.max(label.x);
                    sizes.1 = sizes.1.max(kind.x);
                    sizes.2 = sizes.2.max(label.y).max(kind.y);
                }
                sizes
            });
            let row_height = text_height.ceil() + 4.0;
            let frame = egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4));
            let margin = frame.total_margin().sum();
            let scroll_width = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin + ui.spacing().scroll.bar_outer_margin;
            let width = (label_width + kind_width + 16.0 + scroll_width + margin.x).ceil().min(viewport.width());
            let mut height = row_height * c.items.len().min(5) as f32;
            let mut anchor = out.galley_pos + caret.left_bottom().to_vec2() + egui::vec2(0.0, 4.0);
            anchor.x = anchor.x.clamp(viewport.left(), (viewport.right() - width).max(viewport.left()));
            let above = (out.galley_pos.y + caret.top() - 4.0 - viewport.top()).max(0.0);
            let below = (viewport.bottom() - anchor.y).max(0.0);
            if below < height + margin.y && above > below {
                height = height.min((above - margin.y).max(0.0));
                anchor.y = out.galley_pos.y + caret.top() - 4.0 - height - margin.y;
            } else {
                height = height.min((below - margin.y).max(0.0));
            }
            let mut clicked = None;
            egui::Area::new(self.id.with("completion")).order(egui::Order::Foreground).fixed_pos(anchor).show(ui.ctx(), |ui| {
                ui.spacing_mut().button_padding = egui::vec2(4.0, 2.0);
                ui.spacing_mut().interact_size.y = row_height;
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.spacing_mut().scroll.floating = false;
                frame.show(ui, |ui| {
                    ui.set_width((width - margin.x).max(0.0));
                    egui::ScrollArea::vertical().id_salt(self.id.with("completion-scroll")).max_height(height).show(ui, |ui| {
                        for (i, s) in c.items.iter().enumerate() {
                            let selected = i == c.selected;
                            let color = match s.kind {
                                "keyword" => pal.editor_keyword,
                                "table" => pal.accent,
                                "column" => pal.success,
                                _ => pal.editor_function,
                            };
                            let button = egui::Button::selectable(selected, egui::RichText::new(&s.text).font(font.clone()))
                                .right_text(egui::RichText::new(s.kind).font(kind_font.clone()).color(color))
                                .gap(8.0).truncate();
                            let resp = ui.add_sized([ui.available_width(), row_height], button).on_hover_text(&s.text);
                            if selected && (reveal_completion || out.response.changed() || explicit_completion) {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
                });
            });
            if let Some(i) = clicked {
                c.selected = i;
                let s = c.items[i].clone();
                let start = char_to_byte(text, c.start_char);
                let mut end = char_to_byte(text, self.cursor_char);
                if c.bracketed && text[end..].starts_with(']') { end += 1; }
                let replacement = completion_text(&s, catalog, c.bracketed);
                text.replace_range(start..end, &replacement);
                let caret = c.start_char + replacement.chars().count() - usize::from(s.kind == "function" && s.text.ends_with("()"));
                self.pending_cursor = Some((caret, caret));
                self.dismissed_at = Some((caret, text.clone()));
                self.completion = None;
                ui.memory_mut(|m| m.request_focus(self.id));
                ev.changed = true;
            }
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, editor: &mut SqlEditor, sql: &mut String, events: Vec<egui::Event>) -> EditorEvents {
        frame_output(ctx, editor, sql, events).0
    }

    fn frame_output(ctx: &egui::Context, editor: &mut SqlEditor, sql: &mut String, events: Vec<egui::Event>) -> (EditorEvents, Vec<egui::epaint::ClippedShape>) {
        catalog_frame_output(ctx, editor, sql, &Catalog::default(), events)
    }

    fn catalog_frame_output(ctx: &egui::Context, editor: &mut SqlEditor, sql: &mut String, catalog: &Catalog, events: Vec<egui::Event>) -> (EditorEvents, Vec<egui::epaint::ClippedShape>) {
        let mut result = EditorEvents::default();
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0))), events, ..Default::default() }, |ui| {
            result = editor.show(ui, sql, catalog, 14.0, false, false);
        });
        output.textures_delta.clear();
        (result, output.shapes)
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn completion_survives_idle_frames_and_accepts_without_reopening() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("persistent-completion"));
        let mut sql = String::new();
        frame(&ctx, &mut editor, &mut sql, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text("sel".into())]);
        assert!(editor.completion.is_some());
        for _ in 0..3 { frame(&ctx, &mut editor, &mut sql, vec![]); }
        assert!(editor.completion.is_some(), "idle frames must retain suggestions");
        frame(&ctx, &mut editor, &mut sql, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(sql, "SELECT");
        assert!(editor.completion.is_none());
    }

    #[test]
    fn explicit_completion_reopens_after_escape_and_run_shortcut_is_preserved() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("explicit-completion"));
        let mut sql = String::new();
        frame(&ctx, &mut editor, &mut sql, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text("sel".into())]);
        frame(&ctx, &mut editor, &mut sql, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(editor.completion.is_none());
        frame(&ctx, &mut editor, &mut sql, vec![key(Key::Space, Modifiers::CTRL)]);
        assert!(editor.completion.is_some());
        let result = frame(&ctx, &mut editor, &mut sql, vec![key(Key::Enter, Modifiers::COMMAND)]);
        assert!(result.run_current);
        assert_eq!(sql, "sel");
    }

    #[test]
    fn comments_and_strings_do_not_offer_completion() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("string-completion"));
        for input in ["'sel", "-- sel", "/* sel"] {
            let mut sql = String::new();
            frame(&ctx, &mut editor, &mut sql, vec![]);
            ctx.memory_mut(|m| m.request_focus(editor.id));
            frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text(input.into())]);
            assert!(editor.completion.is_none(), "{input}");
        }
    }

    #[test]
    fn mouse_acceptance_keeps_focus_and_places_function_caret_inside_parentheses() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("mouse-completion"));
        let mut sql = String::new();
        frame(&ctx, &mut editor, &mut sql, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text("abs".into())]);
        let (_, shapes) = frame_output(&ctx, &mut editor, &mut sql, vec![]);
        fn find(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "ABS()" => Some(text.pos + text.galley.size() * 0.5),
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(find),
                _ => None,
            }
        }
        let point = shapes.iter().find_map(|shape| find(&shape.shape)).expect("function suggestion rendered");
        let button = |pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::PointerMoved(point), button(true)]);
        frame(&ctx, &mut editor, &mut sql, vec![button(false)]);
        assert_eq!(sql, "ABS()");
        assert!(ctx.memory(|m| m.has_focus(editor.id)));
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text("1".into())]);
        assert_eq!(sql, "ABS(1)");
    }

    #[test]
    fn explicit_completion_after_whitespace_survives_idle_frames() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("empty-prefix-completion"));
        let mut sql = String::new();
        frame(&ctx, &mut editor, &mut sql, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        frame(&ctx, &mut editor, &mut sql, vec![egui::Event::Text("SELECT ".into())]);
        frame(&ctx, &mut editor, &mut sql, vec![key(Key::Space, Modifiers::CTRL)]);
        assert!(editor.completion.is_some());
        for _ in 0..3 { frame(&ctx, &mut editor, &mut sql, vec![]); }
        assert!(editor.completion.is_some());
    }

    #[test]
    fn keywords_are_sorted_for_binary_search() {
        let mut sorted = KEYWORDS.to_vec();
        sorted.sort();
        assert_eq!(sorted, KEYWORDS.to_vec());
    }

    #[test]
    fn tokenizer_classifies_sql() {
        let s = "SELECT count(*), 'x' FROM t -- note\nWHERE id = :id";
        let toks: Vec<(&str, Tok)> = tokenize(s).into_iter().filter(|t| t.2 != Tok::Plain).map(|(a, b, t)| (&s[a..b], t)).collect();
        assert!(toks.contains(&("SELECT", Tok::Keyword)));
        assert!(toks.contains(&("count", Tok::Function)));
        assert!(toks.contains(&("'x'", Tok::Single)));
        assert!(toks.contains(&("-- note", Tok::Comment)));
        assert!(toks.contains(&(":id", Tok::Param)));
    }

    #[test]
    fn postgres_casts_are_not_params() {
        let s = "select a::text";
        assert!(!tokenize(s).iter().any(|t| t.2 == Tok::Param));
    }

    #[test]
    fn alias_completion_resolves_columns() {
        let mut cat = Catalog::default();
        cat.tables = vec!["users".into(), "orders".into()];
        cat.columns.insert("users".into(), vec!["id".into(), "email".into()]);
        let text = "select u. from public.users u";
        let s = suggestions(text, "u.em", &cat);
        assert_eq!(s.first().map(|s| s.text.as_str()), Some("email"));
    }

    #[test]
    fn prefix_matches_rank_before_substring_matches() {
        let cat = Catalog { tables: vec!["user_roles".into(), "app_users".into()], ..Default::default() };
        let s = suggestions("", "user", &cat);
        assert_eq!(s[0].text, "user_roles");
        assert!(s.iter().any(|x| x.text == "app_users"));
    }

    #[test]
    fn exact_completion_narrows_to_one_case_insensitive_match() {
        let mut cat = Catalog { tables: vec!["ATMB".into(), "ATMB_archive".into(), "old_ATMB".into()], ..Default::default() };
        assert_eq!(suggestions("", "atmb", &cat), vec![Suggestion { text: "ATMB".into(), kind: "table" }]);
        cat.columns.insert("ATMB".into(), vec!["id".into(), "id_parent".into()]);
        assert_eq!(suggestions("select a. from ATMB a", "a.ID", &cat), vec![Suggestion { text: "id".into(), kind: "column" }]);
        assert_eq!(suggestions("", "select", &cat), vec![Suggestion { text: "SELECT".into(), kind: "keyword" }]);
    }

    fn sqlserver_catalog() -> Catalog {
        let mut catalog = Catalog {
            databases: vec!["ATMBersama".into(), "OtherDb".into()], current_database: "ATMBersama".into(),
            schemas: vec!["dbo".into()], schema: "dbo".into(), tables: vec!["AJ_000".into()], ..Default::default()
        };
        catalog.columns.insert("AJ_000".into(), vec!["id".into()]);
        catalog
    }

    #[test]
    fn sqlserver_suggests_database_schema_table_and_qualified_columns() {
        let mut catalog = sqlserver_catalog();
        let first = suggestions("select * from at", "at", &catalog);
        assert_eq!(first[0], Suggestion { text: "ATMBersama".into(), kind: "database" });
        assert_eq!(suggestions("", "atmbersama.d", &catalog), vec![Suggestion { text: "dbo".into(), kind: "schema" }]);
        assert_eq!(suggestions("", "ATMBersama.dbo.AJ", &catalog), vec![Suggestion { text: "AJ_000".into(), kind: "table" }]);
        assert_eq!(suggestions("", "dbo.AJ", &catalog), vec![Suggestion { text: "AJ_000".into(), kind: "table" }]);
        catalog.cache_database("OtherDb", &crate::db::QueryResult {
            rows: vec![vec![Some("audit".into()), Some("AJ_123".into()), Some("remote_id".into())]], ..Default::default()
        });
        assert_eq!(suggestions("", "OtherDb.a", &catalog), vec![Suggestion { text: "audit".into(), kind: "schema" }]);
        assert_eq!(suggestions("", "OtherDb.audit.AJ", &catalog), vec![Suggestion { text: "AJ_123".into(), kind: "table" }]);
        assert_eq!(suggestions("select r. from OtherDb.audit.AJ_123 r", "r.rem", &catalog), vec![Suggestion { text: "remote_id".into(), kind: "column" }]);
        assert!(suggestions("", "OtherDb.dbo.AJ", &catalog).is_empty(), "another database must not borrow current tables");
    }

    #[test]
    fn qualified_acceptance_preserves_database_and_schema() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("qualified-completion"));
        let mut sql = String::new();
        let catalog = sqlserver_catalog();
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![egui::Event::Text("select * from at".into())]);
        for suffix in ["", ".", ".AJ"] {
            if !suffix.is_empty() { catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![egui::Event::Text(suffix.into())]); }
            catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![key(Key::Tab, Modifiers::NONE)]);
        }
        assert_eq!(sql, "select * from ATMBersama.dbo.AJ_000");
    }

    #[test]
    fn asynchronous_database_catalog_reopens_completion_without_retyping() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("pending-database-completion"));
        let mut sql = String::new();
        let mut catalog = sqlserver_catalog();
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        let (events, _) = catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![egui::Event::Text("select * from OtherDb.audit.AJ".into())]);
        assert_eq!(events.completion_database.as_deref(), Some("OtherDb"));
        assert!(editor.completion.is_none());
        catalog.cache_database("OtherDb", &crate::db::QueryResult {
            rows: vec![vec![Some("audit".into()), Some("AJ_123".into()), Some("remote_id".into())]], ..Default::default()
        });
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        assert!(editor.completion.is_some(), "metadata arrival must reopen suggestions on an idle frame");
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(sql, "select * from OtherDb.audit.AJ_123");
        assert_eq!(catalog.current_database, "ATMBersama");
    }

    #[test]
    fn metadata_requests_resolve_aliases_and_current_database_schemas() {
        let mut catalog = sqlserver_catalog();
        catalog.schemas.push("audit".into());
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("alias-catalog-request"));
        let mut sql = "select r. from OtherDb.audit.AJ_123 r".to_string();
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        editor.pending_cursor = Some((9, 9));
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        let (events, _) = catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![key(Key::Space, Modifiers::CTRL)]);
        assert_eq!(events.completion_database.as_deref(), Some("OtherDb"));
        assert_eq!(catalog.database_request("", "audit."), Some("ATMBersama".into()));
    }

    #[test]
    fn sqlserver_completion_preserves_brackets_and_quotes_special_names() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::unique("quoted-database-completion"));
        let mut sql = String::new();
        let mut catalog = sqlserver_catalog();
        catalog.databases.push("Sales-Archive.2026".into());
        catalog.cache_database("Sales-Archive.2026", &crate::db::QueryResult {
            rows: vec![vec![Some("dbo".into()), Some("AJ] 000".into()), Some("id".into())]], ..Default::default()
        });
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![]);
        ctx.memory_mut(|m| m.request_focus(editor.id));
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![egui::Event::Text("select * from Sal".into())]);
        catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(sql, "select * from [Sales-Archive.2026]");
        for suffix in [".[db", ".[AJ"] {
            catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![egui::Event::Text(suffix.into())]);
            catalog_frame_output(&ctx, &mut editor, &mut sql, &catalog, vec![key(Key::Tab, Modifiers::NONE)]);
        }
        assert_eq!(sql, "select * from [Sales-Archive.2026].[dbo].[AJ]] 000]");
        assert_eq!(suggestions("select r. from [Sales-Archive.2026].[dbo].[AJ]] 000] r", "r.", &catalog), vec![Suggestion { text: "id".into(), kind: "column" }]);
    }

    #[test]
    fn completion_prefix_ignores_brackets_inside_strings_and_comments() {
        for sql in ["SELECT '[';\nSELECT * FROM at", "-- [\nSELECT * FROM at", "/* [ */ SELECT * FROM at"] {
            let (start, part) = completion_prefix(sql, true);
            assert_eq!(&sql[start..], "at");
            assert_eq!(&sql[part..], "at");
        }
        let sql = "SELECT * FROM [Sales'Archive].dbo.[AJ";
        let (start, part) = completion_prefix(sql, true);
        assert_eq!(&sql[start..], "[Sales'Archive].dbo.[AJ");
        assert_eq!(&sql[part..], "[AJ");
    }

    #[test]
    fn bracketed_mouse_and_keyboard_acceptance_consumes_auto_closed_bracket() {
        for mouse in [false, true] {
            let ctx = egui::Context::default();
            let id = egui::Id::unique(("auto-closed-completion", mouse));
            let mut editor = SqlEditor::new(id);
            let mut sql = String::new();
            let catalog = sqlserver_catalog();
            let mut draw = |events| {
                let mut output = ctx.run_ui(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0))), events, ..Default::default()
                }, |ui| { editor.show(ui, &mut sql, &catalog, 14.0, false, true); });
                output.textures_delta.clear();
                output.shapes
            };
            draw(vec![]);
            ctx.memory_mut(|m| m.request_focus(id));
            for text in ["select * from dbo.", "[", "AJ"] { draw(vec![egui::Event::Text(text.into())]); }
            let mut shapes = Vec::new();
            for _ in 0..3 { shapes = draw(vec![]); }
            if mouse {
                let (_, rect) = popup_text_rects(&shapes).into_iter().find(|(text, _)| text == "AJ_000").expect("table suggestion");
                let point = rect.center();
                for pressed in [true, false] {
                    draw(vec![egui::Event::PointerMoved(point), egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
                }
            } else { draw(vec![key(Key::Tab, Modifiers::NONE)]); }
            assert_eq!(sql, "select * from dbo.[AJ_000]", "mouse={mouse}");
        }
    }

    fn popup_text_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, clip: egui::Rect, rects: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                    if clip.contains_rect(rect) { rects.push((text.galley.text().to_string(), rect)); }
                }
                egui::epaint::Shape::Vec(shapes) => for shape in shapes { collect(shape, clip, rects); },
                _ => {}
            }
        }
        let mut rects = Vec::new();
        for shape in shapes { collect(&shape.shape, shape.clip_rect, &mut rects); }
        rects
    }

    #[test]
    fn completion_popup_shows_five_compact_rows_and_scrolls() {
        for palette in [crate::theme::Palette::night(), crate::theme::Palette::day()] {
            for font_size in [14.0, 22.0] {
                let ctx = egui::Context::default();
                palette.apply(&ctx, 1.0);
                let mut editor = SqlEditor::new(egui::Id::unique("compact-completion"));
                let mut sql = String::new();
                let cat = Catalog { tables: (0..12).map(|i| format!("at_table_{i:02}")).collect(), ..Default::default() };
                let mut draw = |events| {
                    let mut output = ctx.run_ui(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), events, ..Default::default()
                    }, |ui| { editor.show(ui, &mut sql, &cat, font_size, false, false); });
                    output.textures_delta.clear();
                    output.shapes
                };
                draw(vec![]);
                ctx.memory_mut(|m| m.request_focus(egui::Id::unique("compact-completion")));
                draw(vec![egui::Event::Text("at".into())]);
                let mut shapes = Vec::new();
                for _ in 0..3 { shapes = draw(vec![]); }
                let rows: Vec<_> = popup_text_rects(&shapes).into_iter().filter(|(text, _)| text.starts_with("at_table_")).collect();
                assert_eq!(rows.len(), 5, "five fully visible rows at font size {font_size}: {rows:?}");
                assert!(rows[1].1.top() - rows[0].1.top() <= rows[0].1.height() + 5.0, "row height must track text height");
                let popup = ctx.memory(|m| m.area_rect(egui::Id::unique("compact-completion").with("completion"))).unwrap();
                assert!(popup.width() < font_size * 14.0, "popup width should scale with its labels and font: {popup:?}");
                for _ in 0..7 { draw(vec![key(Key::ArrowDown, Modifiers::NONE)]); }
                for _ in 0..20 { shapes = draw(vec![]); }
                assert!(popup_text_rects(&shapes).iter().any(|(text, _)| text == "at_table_07"), "keyboard navigation must reveal rows below the first five");
                let before: Vec<_> = popup_text_rects(&shapes).into_iter().filter(|(text, _)| text.starts_with("at_table_")).map(|(text, _)| text).collect();
                draw(vec![egui::Event::PointerMoved(popup.center())]);
                draw(vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -120.0),
                    phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Unknown, modifiers: Modifiers::NONE,
                }]);
                for _ in 0..20 { shapes = draw(vec![]); }
                let after: Vec<_> = popup_text_rects(&shapes).into_iter().filter(|(text, _)| text.starts_with("at_table_")).map(|(text, _)| text).collect();
                assert_ne!(before, after, "mouse wheel must scroll the completion list");
                draw(vec![egui::Event::Text("_table_07".into())]);
                for _ in 0..3 { shapes = draw(vec![]); }
                let single = ctx.memory(|m| m.area_rect(egui::Id::unique("compact-completion").with("completion"))).unwrap();
                let rows: Vec<_> = popup_text_rects(&shapes).into_iter().filter(|(text, rect)| text.starts_with("at_table_") && single.contains_rect(*rect)).collect();
                assert_eq!(rows.len(), 1, "the exact match must remain visible after filtering a scrolled list");
                assert_eq!(rows[0].0, "at_table_07");
                assert!(single.height() < popup.height() / 2.0, "popup must shrink with results: {single:?}");
            }
        }
    }

    #[test]
    fn completion_popup_fits_long_identifiers_on_one_line() {
        let ctx = egui::Context::default();
        crate::theme::Palette::night().apply(&ctx, 1.0);
        let id = egui::Id::unique("long-completion");
        let mut editor = SqlEditor::new(id);
        let mut sql = String::new();
        let name = "at_LogPortalQRISOnUs_match_copy1_with_details";
        let cat = Catalog { tables: vec![name.into()], ..Default::default() };
        let mut draw = |events| {
            let mut output = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0))), events, ..Default::default()
            }, |ui| { editor.show(ui, &mut sql, &cat, 14.0, false, false); });
            output.textures_delta.clear();
            output.shapes
        };
        draw(vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        draw(vec![egui::Event::Text("at_Log".into())]);
        let mut shapes = Vec::new();
        for _ in 0..3 { shapes = draw(vec![]); }
        let popup = ctx.memory(|m| m.area_rect(id.with("completion"))).unwrap();
        let (_, label) = popup_text_rects(&shapes).into_iter().find(|(text, _)| text == name).expect("full identifier must fit");
        assert!(popup.contains_rect(label));
        assert!(label.height() < 22.0, "identifier must not wrap");
        assert!(popup.width() > 300.0 && popup.width() < 784.0, "popup must fit the name within the viewport: {popup:?}");
    }
}
