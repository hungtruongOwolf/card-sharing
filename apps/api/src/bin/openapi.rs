//! Prints the OpenAPI document to stdout (no config, no server):
//! `cargo run -q -p api --bin openapi > packages/api-client/openapi.json`

use std::io::Write;

fn main() -> anyhow::Result<()> {
    let json = api::openapi::spec().to_pretty_json()?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(json.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}
