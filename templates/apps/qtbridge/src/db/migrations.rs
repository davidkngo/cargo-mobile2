//! Versioned schema migrations — the same model Realm Swift uses: a single
//! `SCHEMA_VERSION` plus ordered migration steps that run exactly once per
//! database.
//!
//! SQLite's `PRAGMA user_version` is the stored version. On open, [`run`]
//! applies every step from the database's current version up to
//! [`SCHEMA_VERSION`], inside one transaction, then stamps the new version.
//!
//! ## Adding a migration
//! 1. Append a step to [`steps`] (index `i` migrates `v_i` → `v_{i+1}`).
//! 2. Bump [`SCHEMA_VERSION`] by 1.
//!
//! Each step gets a `&Connection` already inside the migration transaction, so a
//! failure rolls the whole upgrade back — you never end up half-migrated.

use rusqlite::Connection;

/// Current schema version. Must equal `steps().len()`.
pub const SCHEMA_VERSION: i64 = 0;

type Step = fn(&Connection) -> rusqlite::Result<()>;

/// Ordered migrations. `steps()[i]` upgrades a database from version `i` to
/// `i + 1`.
fn steps() -> Vec<Step> {
    vec![
        // v0 -> v1: your first migration goes here, e.g.
        // |c| {
        //     c.execute_batch(
        //         "CREATE TABLE IF NOT EXISTS items (\
        //            id INTEGER PRIMARY KEY, body TEXT NOT NULL, created_at INTEGER)",
        //     )?;
        //     Ok(())
        // },
    ]
}

/// Apply all pending migrations. Cheap no-op once the database is current;
/// safe to call on every open.
pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let steps = steps();
    debug_assert_eq!(
        steps.len() as i64,
        SCHEMA_VERSION,
        "SCHEMA_VERSION must match the number of migration steps"
    );

    let from: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if from >= SCHEMA_VERSION {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;
    for v in from..SCHEMA_VERSION {
        eprintln!("[db] migrating schema v{} -> v{}", v, v + 1);
        (steps[v as usize])(&tx)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}
