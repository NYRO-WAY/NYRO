#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    Sqlite,
    Postgres,
    MySql,
}

impl SqlDialect {
    pub fn placeholder(self, index: usize) -> String {
        match self {
            SqlDialect::Postgres => format!("${index}"),
            SqlDialect::Sqlite | SqlDialect::MySql => "?".to_string(),
        }
    }

    pub fn supports_returning(self) -> bool {
        matches!(self, SqlDialect::Sqlite | SqlDialect::Postgres)
    }

    pub fn upsert_keyword(self) -> &'static str {
        match self {
            SqlDialect::MySql => "ON DUPLICATE KEY UPDATE",
            SqlDialect::Sqlite | SqlDialect::Postgres => "ON CONFLICT",
        }
    }
}
