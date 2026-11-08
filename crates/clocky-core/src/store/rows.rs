//! Row helpers shared by the repositories.

use rusqlite::{Connection, Params, Row};

use super::StoreError;

/// Runs a query and converts each row with `convert`, which may fail with a [`StoreError`]
/// (unlike `query_map`, whose closure can only return a `rusqlite::Error`).
pub(super) fn collect<T, P: Params>(
    conn: &Connection,
    sql: &str,
    params: P,
    mut convert: impl FnMut(&Row<'_>) -> Result<T, StoreError>,
) -> Result<Vec<T>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(convert(row)?);
    }
    Ok(items)
}

/// The first item, or [`StoreError::NotFound`] if there is none.
pub(super) fn first<T>(items: Vec<T>, entity: &'static str, id: i64) -> Result<T, StoreError> {
    items
        .into_iter()
        .next()
        .ok_or(StoreError::NotFound { entity, id })
}

/// `Ok` if the statement changed a row; [`StoreError::NotFound`] if it changed none.
pub(super) fn ensure_found(
    changed: usize,
    entity: &'static str,
    id: i64,
) -> Result<(), StoreError> {
    if changed == 0 {
        return Err(StoreError::NotFound { entity, id });
    }
    Ok(())
}
