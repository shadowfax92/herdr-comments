use anyhow::Result;
use clap::{Parser, Subcommand};
use herdr_comments::herdr::{CliHerdr, HerdrClient};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Capture,
    CapturePopup,
}

fn main() {
    if let Err(error) = run() {
        notify_error(&error);
        eprintln!("herdr-comments: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Capture => herdr_comments::capture_action(),
        Command::CapturePopup => herdr_comments::capture_popup(),
    }
}

fn notify_error(error: &anyhow::Error) {
    let Some(bin) = std::env::var_os("HERDR_BIN_PATH") else {
        return;
    };
    let _ = CliHerdr::new(bin).notify("Herdr Comments", &error.to_string());
}
