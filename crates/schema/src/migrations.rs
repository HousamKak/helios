//! Embedded migrations — all SQL is bundled into the binary at compile
//! time so the store crate (or any tool needing a fresh DB) can apply
//! migrations without filesystem access.
//!
//! Order matters: migrations are applied in slice order. Add new ones at
//! the end; never reorder.

/// One migration file: `(name, sql)`. Name is purely informational.
pub struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

/// All migrations in order. Append new entries here as new SQL files are
/// added under `crates/schema/migrations/`.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_h_origin",
        sql: include_str!("../migrations/0001_h_origin.sql"),
    },
    Migration {
        name: "0002_os_entities",
        sql: include_str!("../migrations/0002_os_entities.sql"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_loaded_and_non_empty() {
        assert!(!MIGRATIONS.is_empty());
        for m in MIGRATIONS {
            assert!(!m.name.is_empty(), "migration name must be non-empty");
            assert!(!m.sql.is_empty(), "migration sql must be non-empty: {}", m.name);
        }
    }

    #[test]
    fn migrations_in_expected_order() {
        let names: Vec<_> = MIGRATIONS.iter().map(|m| m.name).collect();
        assert_eq!(names, vec!["0001_h_origin", "0002_os_entities"]);
    }
}
