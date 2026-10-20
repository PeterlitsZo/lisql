# Lisql

Lisql compiles Lisp-style queries into callable Rust functions that return SQL
and ordered bind parameters for MySQL, PostgreSQL, or SQLite. No database driver
is included.

Save this query as `users.lisql`:

```lisp
(def-query find-users (min-age :i32 limit :i32)
  (-> (from :users)
      (where (:> :age min-age))
      (select (:id :name))
      (limit limit)))
```

Install and generate a Rust module:

```sh
cargo install --locked --path crates/lisql
lisql users.lisql --dialect postgresql -o users.rs
```

Add the runtime to your application's `Cargo.toml` (adjust the checkout path):

```toml
[dependencies]
lisql-runtime = { path = "../lisql/crates/lisql-runtime", version = "=0.1.0" }
```

Include `users.rs` as a module and call `users::find_users(18, 10)?` to obtain
`Query { sql, params }`. Pass the SQL and parameters to your database driver.
Supported parameter types: `:i32`, `:i64`, `:bool`, and `:string`. Supported
operations: `from`, `where`, `select`, `order-by`, `limit`, and `offset`.
The compiler library also exposes `lex`, `parse`, and source-spanned AST types.

Check development changes with `cargo fmt --all --check` and
`cargo clippy --locked --workspace --all-targets -- -D warnings`.

Licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
