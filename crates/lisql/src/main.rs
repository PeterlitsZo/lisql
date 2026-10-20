//! CLI boundary: parse options, report source errors, and safely publish
//! generated Rust.

use clap::{Parser, ValueEnum};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
// CLI spellings mapped to the runtime rendering targets.
enum Dialect {
    // Generate SQL using the MySQL dialect.
    Mysql,
    // Generate SQL using the PostgreSQL dialect.
    Postgresql,
    // Generate SQL using the SQLite dialect.
    Sqlite,
}

impl From<Dialect> for lisql::Dialect {
    fn from(value: Dialect) -> Self {
        match value {
            Dialect::Mysql => Self::MySql,
            Dialect::Postgresql => Self::PostgreSql,
            Dialect::Sqlite => Self::Sqlite,
        }
    }
}

// Compilation inputs and output destination, parsed by clap.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Source .lisql file
    input: PathBuf,

    /// SQL dialect fixed in the generated module
    #[arg(long, value_enum)]
    dialect: Dialect,

    /// Output Rust file (stdout if omitted); parent directory must exist
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

fn run(args: Args) -> Result<(), String> {
    // Finish compilation before opening an output file, preserving it on source
    // errors.
    let source =
        fs::read_to_string(&args.input).map_err(|e| format!("{}: {e}", args.input.display()))?;
    let rust = lisql::compile(&source, args.dialect.into())
        .map_err(|e| e.render(&args.input.to_string_lossy(), &source))?;

    // Debug escaping keeps arbitrary filenames on a single comment line.
    let rust = format!("// Source: {:?}\n{rust}", args.input.to_string_lossy());

    if let Some(output) = args.output {
        // Filesystem identity also catches hard links, unlike path string
        // comparison.
        match same_file::is_same_file(&args.input, &output) {
            Ok(true) => {
                return Err("output must not overwrite the input file (including links)".into())
            }
            Ok(false) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("{}: {e}", output.display())),
        }

        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        // Keep the temporary file on the same filesystem so persistence can
        // replace atomically.
        let write = || -> io::Result<()> {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(rust.as_bytes())?;
            temp.as_file().sync_all()?;
            temp.persist(&output).map_err(|e| e.error)?;

            Ok(())
        };
        write().map_err(|e| format!("{}: {e}", output.display()))?;
    } else {
        io::stdout()
            .lock()
            .write_all(rust.as_bytes())
            .map_err(|e| format!("stdout: {e}"))?;
    }

    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lisql: {error}");
            ExitCode::FAILURE
        }
    }
}
