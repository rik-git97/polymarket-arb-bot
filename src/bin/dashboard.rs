use clap::Parser;
use polymarket_arb_bot::dashboard;

#[derive(Parser)]
#[command(name = "dashboard", about = "Polymarket Dump-and-Hedge Bot Dashboard")]
struct DashboardCli {
    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(long, default_value = "0.0.0.0")]
    host: String,
}

#[tokio::main]
async fn main() {
    let cli = DashboardCli::parse();

    let app = dashboard::create_router();

    let addr = format!("{}:{}", cli.host, cli.port);
    println!("Dashboard: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
