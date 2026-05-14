use clap::Parser;
use polymarket_arb_bot::config::Config;

#[derive(Parser, Debug)]
#[command(name = "polymarket-arb-bot")]
struct Cli {
    #[arg(long, default_value = "config.json")]
    config: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let _cfg = Config::load(&cli.config);

    eprintln!("polymarket-arb-bot: live trading mode");
    eprintln!("Not yet implemented. Use `cargo run --bin dashboard` for the web UI.");
}
