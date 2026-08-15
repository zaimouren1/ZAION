//! sandbox-svc CLI (benchmark sandbox).
use sandbox_svc::{format_item, parse_batch, process_batch, validate_token};
use std::io::{self, BufRead};

fn main() {
    let mut cfg = String::new();
    if let Ok(text) = std::fs::read_to_string("config.toml") {
        cfg = text;
    }
    let cap = parse_cap(&cfg);

    println!("sandbox-svc ready (cap={})", cap);
    let stdin = io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if let Ok(items) = parse_batch(&line) {
            let total = process_batch(items.clone(), cap);
            println!("batch total: {}", total);
            for (i, v) in items.iter().enumerate() {
                println!("{}", format_item(i, *v));
            }
        }
    }
}

fn parse_cap(toml_text: &str) -> usize {
    let v: toml::Value = match toml::from_str(toml_text) {
        Ok(v) => v,
        Err(_) => return 10,
    };
    v.get("service")
        .and_then(|s| s.get("max_batch"))
        .and_then(|m| m.as_integer())
        .map(|n| n as usize)
        .unwrap_or(10)
}

fn _token_check(token: &str) -> bool {
    validate_token(token)
}
