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
    Review,
    ReviewPopup,
    Paste,
    ConfirmReview {
        #[arg(long)]
        id: String,
    },
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
        Command::Review => herdr_comments::review_action(),
        Command::ReviewPopup => herdr_comments::review_popup(),
        Command::Paste => herdr_comments::paste_action(),
        Command::ConfirmReview { id } => herdr_comments::confirm_review(&id),
    }
}

fn notify_error(error: &anyhow::Error) {
    let Some(bin) = std::env::var_os("HERDR_BIN_PATH") else {
        return;
    };
    let Some(socket_path) = std::env::var_os("HERDR_SOCKET_PATH") else {
        return;
    };
    let _ = CliHerdr::new(bin, socket_path).notify("Herdr Comments", &error.to_string());
}
