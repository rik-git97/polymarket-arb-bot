use clap::Parser;
use serde::Deserialize;
use std::fs;

#[derive(Parser, Debug)]
#[command(name = "polymarket-arb-bot")]
pub struct Cli {
    #[arg(long, default_value = "config.json")]
    pub config: String,

    #[arg(long)]
    pub simulation: bool,

    #[arg(long)]
    pub production: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub polymarket: Option<PolymarketConfig>,
    pub trading: TradingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolymarketConfig {
    pub gamma_api_url: Option<String>,
    pub clob_api_url: Option<String>,
    pub private_key: Option<String>,
    pub proxy_wallet: Option<String>,
    pub signature_type: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradingConfig {
    pub simulation: Option<bool>,
    pub asset: Option<String>,
    pub timeframe_minutes: Option<u32>,
    pub check_interval_ms: Option<u64>,
    pub dump_move_threshold: Option<f64>,
    pub dump_sum_target: Option<f64>,
    pub dump_window_seconds: Option<u64>,
    pub stop_loss_max_wait_seconds: Option<u64>,
    pub stop_loss_percentage: Option<f64>,
    pub shares_per_leg: Option<f64>,
    pub fee_rate: Option<f64>,
}

impl Config {
    pub fn load(path: &str) -> Self {
        let content = fs::read_to_string(path).unwrap_or_else(|_| {
            eprintln!("Config file not found at {}. Using defaults.", path);
            "{}".to_string()
        });
        serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("Failed to parse config: {}. Using defaults.", e);
            Config::default()
        })
    }

    pub fn default() -> Self {
        Config {
            polymarket: None,
            trading: TradingConfig {
                simulation: Some(true),
                asset: Some("btc".into()),
                timeframe_minutes: Some(15),
                check_interval_ms: Some(1000),
                dump_move_threshold: Some(0.15),
                dump_sum_target: Some(0.95),
                dump_window_seconds: Some(120),
                stop_loss_max_wait_seconds: Some(300),
                stop_loss_percentage: Some(0.20),
                shares_per_leg: Some(10.0),
                fee_rate: Some(0.02),
            },
        }
    }
}
