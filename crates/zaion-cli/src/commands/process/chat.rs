//! `zaion chat` — a thin wrapper over `cmd_wake` that resolves the default
//! process and forces streaming mode.

use crate::commands::provider::resolve_provider_selection_from_args;
use crate::commands::CliError;
use crate::config::ZaionConfig;

use super::helpers::resolve_default_pid;
use super::wake::cmd_wake;

/// `zaion chat "your message"` — talk to your agent, no PID needed.
///
/// Streams by default. Uses the configured provider and the default process
/// (auto-created on first run by [`resolve_default_pid`]).
pub fn cmd_chat(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_chat_help();
        return Ok(());
    }

    let (message, forwarded_flags) = parse_chat_args(args)?;
    let pid = resolve_default_pid(&cfg)?;
    resolve_provider_selection_from_args(&forwarded_flags, &cfg)?;
    // Delegate to wake logic with streaming on by default.
    let mut wake_args: Vec<String> = vec![
        args[0].clone(),
        "wake".into(),
        pid,
        message,
        "--stream".into(),
    ];
    wake_args.extend(forwarded_flags);
    match cmd_wake(&wake_args) {
        Ok(()) => Ok(()),
        Err(err) => {
            // If the failure looks like an upstream provider problem, leave a
            // breadcrumb so a brand-new user does not bounce off the wall.
            // We match broadly on "HTTP <4xx/5xx>" and "provider error"
            // because different providers phrase outages differently.
            let msg = err.to_string();
            let looks_like_provider_error = msg.contains("HTTP ")
                && (msg.contains("HTTP 4") || msg.contains("HTTP 5"))
                || msg.contains("upstream")
                || msg.contains("provider error")
                || msg.contains("temporarily unavailable");
            if looks_like_provider_error {
                eprintln!();
                eprintln!("hint: the configured provider is not responding cleanly right now.");
                eprintln!(
                    "      try `zaion chat --provider ollama \"Hello\"` to talk to a local model,"
                );
                eprintln!("      or run `zaion provider` to see configured providers and switch.");
            }
            Err(err)
        }
    }
}

fn parse_chat_args(args: &[String]) -> Result<(String, Vec<String>), CliError> {
    let mut message_parts = Vec::new();
    let mut flags = Vec::new();
    let mut iter = args.iter().skip(2).peekable();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-q" | "--query" => {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{} requires a value", arg)))?;
                message_parts.push(value.clone());
            }
            "-m" | "--model" => {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{} requires a value", arg)))?;
                flags.push("--model".to_string());
                flags.push(value.clone());
            }
            "--provider" | "--parser" => {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{} requires a value", arg)))?;
                flags.push(arg.clone());
                flags.push(value.clone());
            }
            "-t" | "--toolsets" | "--resume" | "-r" | "--source" | "--max-turns" => {
                let _ = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{} requires a value", arg)))?;
            }
            "-s" | "--skills" => {
                if iter.peek().is_some_and(|next| !next.starts_with('-')) {
                    let _ = iter.next();
                } else {
                    flags.push("--stream".to_string());
                }
            }
            "--continue" | "-c" => {
                if iter.peek().is_some_and(|next| !next.starts_with('-')) {
                    let _ = iter.next();
                }
            }
            "--stream" | "--mcp" | "--memory" | "--cache" | "--smart-route" | "--compress"
            | "--unified" | "--no-memory" | "--no-mcp" | "--no-compress" | "--no-webhooks" => {
                flags.push(arg.clone())
            }
            "-v" | "--verbose" | "-Q" | "--quiet" | "--worktree" | "-w" | "--checkpoints"
            | "--yolo" | "--pass-session-id" => {}
            "--" => {
                for rest in iter {
                    message_parts.push(rest.clone());
                }
                break;
            }
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown chat flag '{}'. Run: zaion chat --help",
                    other
                )));
            }
            _ => message_parts.push(arg.clone()),
        }
    }

    let message = message_parts.join(" ").trim().to_string();
    if message.is_empty() {
        return Err(CliError::Usage(
            "zaion chat \"your message\" [--provider ollama|anthropic|openai|groq|mistral] [--model <model>] [--mcp]"
                .to_string(),
        ));
    }

    Ok((message, flags))
}

fn print_chat_help() {
    println!("zaion chat - send one message to your default Agentic Process");
    println!();
    println!("USAGE:");
    println!("  zaion chat \"Hello\" [--provider <provider>] [--model <model>] [features]");
    println!("  zaion chat --query \"Hello\" --provider <provider> -m <model>");
    println!();
    println!("FEATURES:");
    println!(
        "  {:<30} enable or explicitly disable memory",
        "--memory | --no-memory"
    );
    println!(
        "  {:<30} enable or explicitly disable MCP tools",
        "--mcp | --no-mcp"
    );
    println!(
        "  {:<30} request or explicitly disable compression",
        "--compress | --no-compress"
    );
    println!("  {:<30} explicitly disable turn webhooks", "--no-webhooks");
    println!(
        "  {:<30} enable cache or smart provider routing",
        "--cache --smart-route"
    );
    println!();
    println!("EXAMPLES:");
    println!("  zaion chat \"Hello\"");
    println!("  zaion chat --provider ollama \"Hello from local model\"");
    println!("  zaion chat \"use tools\" --mcp");
}

#[cfg(test)]
mod tests {
    use super::parse_chat_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn chat_parser_accepts_reference_query_model_and_session_flags() {
        let parsed = parse_chat_args(&args(&[
            "zaion",
            "chat",
            "--query",
            "hello",
            "-m",
            "test-model",
            "--provider",
            "openai-codex",
            "--resume",
            "session-1",
            "--continue",
            "named",
            "--skills",
            "research,summary",
            "--max-turns",
            "5",
            "--source",
            "telegram",
            "--quiet",
        ]))
        .unwrap();
        assert_eq!(parsed.0, "hello");
        assert!(parsed
            .1
            .windows(2)
            .any(|w| w[0] == "--model" && w[1] == "test-model"));
        assert!(parsed
            .1
            .windows(2)
            .any(|w| w[0] == "--provider" && w[1] == "openai-codex"));
    }

    #[test]
    fn chat_parser_forwards_explicit_feature_disables() {
        let parsed = parse_chat_args(&args(&[
            "zaion",
            "chat",
            "hello",
            "--no-memory",
            "--no-mcp",
            "--no-compress",
            "--no-webhooks",
        ]))
        .unwrap();

        assert_eq!(parsed.0, "hello");
        for flag in ["--no-memory", "--no-mcp", "--no-compress", "--no-webhooks"] {
            assert!(parsed.1.iter().any(|forwarded| forwarded == flag));
        }
    }
}
