use qqnt_adapters::{open_authorized, QQNTAdapter, StructuralAdapter};
use std::{path::PathBuf, sync::atomic::AtomicBool};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("Usage: cargo run -p qqnt-scanner --example scan -- <authorized-directory>")?;
    let (_, dir) = open_authorized(&path)?;
    let summary = qqnt_scanner::scan(
        &dir,
        &StructuralAdapter.detect(&dir),
        &AtomicBool::new(false),
        |_| {},
        |_| Ok(()),
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
