use clap::Parser;
use polymarket_arb_bot::config::Config;
use polymarket_arb_bot::dashboard;

#[derive(Parser)]
#[command(name = "dashboard", about = "Polymarket Dump-and-Hedge Bot Dashboard")]
struct DashboardCli {
    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(short, long)]
    config: Option<String>,

    #[arg(long, default_value = "0.0.0.0")]
    host: String,
}

#[tokio::main]
async fn main() {
    let cli = DashboardCli::parse();
    let config_path = cli.config.as_deref().unwrap_or("config.json");
    let cfg = Config::load(config_path);

    let app = dashboard::create_router(cfg);

    let addr = format!("{}:{}", cli.host, cli.port);
    println!("Dashboard starting at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
