/// SQL rendering target, fixed when compiling a Lisql module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// MySQL: backtick-quoted identifiers and `?` placeholders.
    MySql,
    /// PostgreSQL: double-quoted identifiers and one-based `$n` placeholders.
    PostgreSql,
    /// SQLite: double-quoted identifiers and `?` placeholders.
    Sqlite,
}

impl Dialect {
    /// Stable spelling used by the compiler CLI and generated-file headers.
    pub fn name(self) -> &'static str {
        match self {
            Self::MySql => "mysql",
            Self::PostgreSql => "postgresql",
            Self::Sqlite => "sqlite",
        }
    }

    pub(crate) fn identifier(self, name: &str) -> String {
        // Render validates the restricted identifier alphabet before reaching
        // this helper.
        let quote = if self == Self::MySql { '`' } else { '"' };
        format!("{quote}{name}{quote}")
    }

    pub(crate) fn placeholder(self, position: usize) -> String {
        // Positions are one-based, matching the binding vector after the
        // current value is pushed.
        match self {
            Self::PostgreSql => format!("${position}"),
            _ => "?".into(),
        }
    }
}
