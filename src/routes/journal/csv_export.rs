//! The lines of the Journal as a spreadsheet file, under the filters of the
//! page.

use axum::extract::{Query, State};
use axum::response::Response;

use super::JournalQuery;
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::RequireAdmin;
use crate::middleware::headers::attachment;
use crate::models::JournalEntry;
use crate::repositories::JournalRepository;
use crate::state::AppState;

/// Lets a spreadsheet read the file as UTF-8.
const BOM: &str = "\u{FEFF}";

pub async fn export(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    Query(query): Query<JournalQuery>,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let entries = JournalRepository::list_page(&state.pool, &query.export_filters()).await?;
    let filename = format!("journal-{}.csv", clock::today().format("%Y-%m-%d"));
    Ok(attachment(
        "text/csv; charset=utf-8",
        &filename,
        render_csv(&entries, &i18n),
    ))
}

fn render_csv(entries: &[JournalEntry], i18n: &I18n) -> String {
    let mut csv = String::from(BOM);
    csv.push_str(&csv_row(&header_cells(i18n)));
    for entry in entries {
        csv.push_str(&csv_row(&entry_cells(entry, i18n)));
    }
    csv
}

fn header_cells(i18n: &I18n) -> [String; 7] {
    [
        i18n.t("journal.export_when").to_string(),
        i18n.t("journal.export_type").to_string(),
        i18n.t("journal.export_essential").to_string(),
        i18n.t("journal.export_member").to_string(),
        i18n.t("journal.export_what").to_string(),
        i18n.t("journal.export_detail").to_string(),
        i18n.t("journal.export_ip").to_string(),
    ]
}

fn entry_cells(entry: &JournalEntry, i18n: &I18n) -> [String; 7] {
    let essential = if entry.kind.is_essential() {
        i18n.t("journal.export_yes")
    } else {
        i18n.t("journal.export_no")
    };
    [
        clock::local(&entry.created_at)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        i18n.t(entry.kind.category().label_key()).to_string(),
        essential.to_string(),
        entry.actor_name.clone().unwrap_or_default(),
        entry.sentence(i18n),
        entry.detail.clone().unwrap_or_default(),
        entry.ip.clone().unwrap_or_default(),
    ]
}

fn csv_row(cells: &[String]) -> String {
    let escaped: Vec<String> = cells.iter().map(|cell| csv_cell(cell)).collect();
    format!("{}\r\n", escaped.join(","))
}

/// One cell, safe to open in a spreadsheet: a leading formula character is
/// neutralised with an apostrophe, then the cell is quoted when it needs to be.
fn csv_cell(value: &str) -> String {
    let guarded = if starts_like_a_formula(value) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if !guarded.contains([',', '"', '\r', '\n']) {
        return guarded;
    }
    format!("\"{}\"", guarded.replace('"', "\"\""))
}

fn starts_like_a_formula(value: &str) -> bool {
    value.starts_with(['=', '+', '-', '@', '\t', '\r'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_cell_is_left_alone() {
        assert_eq!(csv_cell("Disk full"), "Disk full");
    }

    #[test]
    fn a_comma_a_quote_or_a_line_break_quotes_the_cell() {
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("one\ntwo"), "\"one\ntwo\"");
    }

    #[test]
    fn a_leading_formula_character_gets_an_apostrophe() {
        assert_eq!(csv_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_cell("-2"), "'-2");
        assert_eq!(csv_cell("@cmd"), "'@cmd");
        assert_eq!(csv_cell("+1"), "'+1");
        assert_eq!(csv_cell("\tx"), "'\tx");
    }

    #[test]
    fn the_apostrophe_comes_before_the_quoting() {
        assert_eq!(csv_cell("=A1,B1"), "\"'=A1,B1\"");
    }

    #[test]
    fn a_row_ends_with_a_carriage_return_and_a_line_feed() {
        let row = csv_row(&["a".to_string(), "b,c".to_string()]);
        assert_eq!(row, "a,\"b,c\"\r\n");
    }
}
