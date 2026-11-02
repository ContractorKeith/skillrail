use skillrail::AppError;
use std::io::Write;
use std::path::Path;

// Flush explicitly so buffered write failures use the same operational contract.
pub fn stdout(text: &str) -> Result<(), AppError> {
    let mut output = std::io::stdout().lock();
    output
        .write_all(text.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|_| AppError::new("output_unavailable", "Cannot write output."))
}

pub fn human_path(path: &Path) -> String {
    skillrail::presentation::safe(&path.to_string_lossy())
}

pub fn finish(result: Result<(), AppError>, json: bool) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let message = if json {
                serde_json::json!({"schema_version": skillrail::SCHEMA_VERSION, "error": error})
                    .to_string()
            } else {
                error.to_string()
            };
            // If stderr also fails, retain the error exit without a second panic.
            let _ = writeln!(std::io::stderr().lock(), "{message}");
            std::process::ExitCode::from(1)
        }
    }
}
