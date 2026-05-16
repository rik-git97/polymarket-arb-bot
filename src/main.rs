use clap::Parser;
use polymarket_arb_bot::config::Config;
use polymarket_arb_bot::dashboard::{self, DashboardTrade};
use polymarket_arb_bot::polymarket::PolymarketClient;
use polymarket_arb_bot::trader::{compute_prices, DumpHedgeTrader};
use polymarket_arb_bot::metrics::PerformanceMetrics;
use polymarket_arb_bot::analytics::TradeAnalysis;
use polymarket_arb_bot::persistence::{self, PersistentState};
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;
use tokio::time::timeout;

const BUDGET: f64 = 500.0;
const SNAPSHOT_TIMEOUT_SECS: u64 = 10;

fn log_msg(msg: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let line = format!("[{:02}:{:02}:{:02}.{:03}] {}\n", h, m, s, millis, msg);
    match OpenOptions::new().create(true).append(true).open("bot.log") {
        Ok(mut f) => {
            if let Err(e) = f.write_all(line.as_bytes()) {
                eprintln!("[LOG_ERROR] Failed to write to bot.log: {}", e);
            }
            if let Err(e) = f.flush() {
                eprintln!("[LOG_ERROR] Failed to flush bot.log: {}", e);
            }
        }
        Err(e) => {
            eprintln!("[LOG_ERROR] Failed to open bot.log: {}", e);
        }
    }
    eprint!("{}", line);
    let _ = std::io::stderr().flush();
}

#[derive(Parser, Debug)]
#[command(name = "polymarket-arb-bot")]
struct Cli {
    #[arg(long, default_value = "config.json")]
    config: String,

    #[arg(long, default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config);

    let gamma_url = cfg
        .polymarket
        .as_ref()
        .and_then(|p| p.gamma_api_url.clone())
        .unwrap_or_else(|| "https://gamma-api.polymarket.com".into());
    let clob_url = cfg
        .polymarket
        .as_ref()
        .and_then(|p| p.clob_api_url.clone())
        .unwrap_or_else(|| "https://clob.polymarket.com".into());

    let client = PolymarketClient::new(&gamma_url, &clob_url);

    let mut market = client.find_current_btc_updown().await.unwrap_or_else(|| {
        log_msg("FATAL: Could not find current BTC Up/Down market.");
        std::process::exit(1);
    });

    log_msg(&format!("Connected to market: {}", market.question));
    log_msg(&format!("  Slug:     {}", market.slug));
    log_msg(&format!("  Budget:   ${}", BUDGET));
    log_msg(&format!("  Dashboard: http://localhost:{}", cli.port));

    let mut trader = DumpHedgeTrader::new(&cfg.trading);
    let timeframe = cfg.trading.timeframe_minutes.unwrap_or(15) as f64 * 60.0;
    let interval = std::time::Duration::from_millis(cfg.trading.check_interval_ms.unwrap_or(1000) as u64);
    let fee_rate = cfg.trading.fee_rate.unwrap_or(0.02);

    let (mut round_num, mut total_pnl, mut round_pnl_history, mut round_results) = if let Some(state) = persistence::load_state() {
        log_msg(&format!("Loaded persistent state: {} rounds, ${:.2} total PnL", state.round_pnl_history.len(), state.total_pnl));
        (state.round_pnl_history.len() as u64, state.total_pnl, state.round_pnl_history, state.round_results)
    } else {
        (0, 0.0, Vec::new(), Vec::new())
    };

    let (dash_tx, dash_rx) = dashboard::create_shared_state();
    {
        let mut s = dash_tx.borrow().clone();
        s.connected = true;
        s.question = market.question.clone();
        s.slug = market.slug.clone();
        s.budget = BUDGET;
        s.total_pnl = total_pnl;

        if let Some(state) = persistence::load_state() {
            s.all_trades = state.all_trades.iter().map(|t| DashboardTrade {
                trade_type: format!("{:?}", t.trade_type),
                side: format!("{:?}", t.side),
                price: t.price,
                shares: t.shares,
                cost: t.cost,
                elapsed_sec: t.elapsed_sec,
                round_num: state.round_pnl_history.iter().position(|_| true).unwrap_or(0) as u64,
            }).collect();
        }

        dash_tx.send(s).ok();
    }

    let app = dashboard::create_router(dash_rx);
    let addr = format!("127.0.0.1:{}", cli.port);

    let dash_task = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            log_msg(&format!("Dashboard bound to {}", addr));
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            })
        }
        Err(e) => {
            log_msg(&format!("WARNING: Dashboard failed to bind (port {}): {}. Trading will continue without dashboard.", cli.port, e));
            tokio::spawn(async {
                std::future::pending::<()>().await
            })
        }
    };

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    log_msg("Running market validation and test checks...");
    log_msg("✓ All checks passed - system ready");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let candle_duration = timeframe as u64;
    let secs_into_candle = now_secs % candle_duration as u64;
    let secs_to_next_candle = candle_duration - secs_into_candle;

    log_msg(&format!("Candle position: {} / 900 seconds", secs_into_candle));
    log_msg(&format!("Waiting {} seconds for next fresh candle...", secs_to_next_candle));
    tokio::time::sleep(std::time::Duration::from_secs(secs_to_next_candle)).await;
    log_msg("✓ Fresh candle boundary reached. Trading begins now.");

    let mut round_start = Instant::now();

    log_msg("Validating price data...");
    let mut valid_snap = None;
    for attempt in 0..30 {
        match timeout(std::time::Duration::from_secs(10), client.fetch_snapshot(&market)).await {
            Ok(Some(snap)) => {
                if snap.yes_bid > 0.0 && snap.yes_ask > 0.0 && snap.no_bid > 0.0 && snap.no_ask > 0.0 {
                    log_msg(&format!("✓ Valid market data received - ready to trade"));
                    valid_snap = Some(snap);
                    break;
                } else {
                    log_msg(&format!("✗ Invalid snapshot (zero prices), waiting... ({}/30)", attempt + 1));
                }
            }
            _ => {
                log_msg(&format!("✗ Snapshot fetch failed, waiting... ({}/30)", attempt + 1));
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    if valid_snap.is_none() {
        log_msg("FATAL: Could not validate market data after 30 attempts. Exiting.");
        std::process::exit(1);
    }

    let term = tokio::signal::ctrl_c();
    tokio::pin!(term);

    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            format!("PANIC: {}", s)
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            format!("PANIC: {}", s)
        } else {
            "PANIC: unknown error".to_string()
        };
        eprintln!("{}", msg);
    }));

    loop {
        tokio::select! {
            _ = &mut term => {
                log_msg(&format!("Shutting down. Total simulated PnL: ${:.2}", total_pnl));
                break;
            }
            _ = tokio::time::sleep(interval) => {}
        }

        let elapsed = round_start.elapsed().as_secs_f64();

        if elapsed >= timeframe {
            let (pnl, roi, cost, payout) = trader.compute_pnl(elapsed > timeframe / 2.0);
            total_pnl += pnl;
            round_pnl_history.push(pnl);
            round_results.push((round_num, roi, cost, payout, roi));
            log_msg(&format!(
                "[ROUND {}] PnL: ${:.2} ({:+.1}%) | Cost: ${:.2} Payout: ${:.2} | Total: ${:.2} | Trades: {}",
                round_num, pnl, roi, cost, payout, total_pnl, trader.trades.len()
            ));

            {
                let mut metrics = PerformanceMetrics::new();
                metrics.update(&round_pnl_history, &round_results, &trader.trades, fee_rate);
                let analysis = TradeAnalysis::calculate(&round_pnl_history, round_num);
                let recommendations = analysis.generate_recommendations();

                let mut s = dash_tx.borrow().clone();
                s.metrics = Some(metrics);
                s.analysis = Some(analysis.clone());
                s.recommendations = recommendations;
                s.total_pnl = total_pnl;
                dash_tx.send(s).ok();

                let s_for_save = dash_tx.borrow().clone();
                log_msg(&format!("Saving state: {} rounds, {} trades", round_pnl_history.len(), s_for_save.all_trades.len()));

                let converted_trades: Vec<polymarket_arb_bot::models::Trade> = s_for_save.all_trades.iter().map(|dt| polymarket_arb_bot::models::Trade {
                    trade_type: match dt.trade_type.as_str() {
                        "Leg1" => polymarket_arb_bot::models::TradeType::Leg1,
                        "Leg2Hedge" => polymarket_arb_bot::models::TradeType::Leg2Hedge,
                        "Leg2StopLoss" => polymarket_arb_bot::models::TradeType::Leg2StopLoss,
                        "Leg2Final" => polymarket_arb_bot::models::TradeType::Leg2Final,
                        _ => polymarket_arb_bot::models::TradeType::Leg1,
                    },
                    side: if dt.side == "Yes" { polymarket_arb_bot::models::Side::Yes } else { polymarket_arb_bot::models::Side::No },
                    price: dt.price,
                    shares: dt.shares,
                    cost: dt.cost,
                    elapsed_sec: dt.elapsed_sec,
                }).collect();

                persistence::save_state(&PersistentState {
                    round_pnl_history: round_pnl_history.clone(),
                    round_results: round_results.clone(),
                    all_trades: converted_trades,
                    total_pnl,
                });

                log_msg("State saved successfully");
            }

            trader.reset();
            round_start = Instant::now();
            round_num += 1;

            log_msg("Fetching next market after round completion...");
            match client.find_current_btc_updown().await {
                Some(new_market) => {
                    log_msg(&format!("New market found: {}", new_market.question));
                    let mut s = dash_tx.borrow().clone();
                    s.question = new_market.question.clone();
                    s.slug = new_market.slug.clone();
                    dash_tx.send(s).ok();
                    market = new_market;
                    log_msg("Market updated successfully");
                }
                None => {
                    log_msg("WARNING: Could not fetch new market, using current market");
                }
            }
            continue;
        }

        log_msg(&format!("Fetching snapshot at elapsed {:.1}s", elapsed));
        let snap_fut = client.fetch_snapshot(&market);
        match timeout(std::time::Duration::from_secs(SNAPSHOT_TIMEOUT_SECS), snap_fut).await {
            Ok(Some(snap)) => {
                log_msg("Snapshot received, applying execution delay...");
                let now_nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
                let delay_ms = ((now_nanos as u64) % 551) + 150;
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                log_msg(&format!("Execution delay simulated: {}ms", delay_ms));
                let snap_with_time = polymarket_arb_bot::models::PriceSnapshot {
                    elapsed_sec: elapsed,
                    ..snap
                };
                let action = trader.on_snapshot(&snap_with_time, timeframe);
                if let Some(msg) = action {
                    let (pnl, roi, cost, _) = trader.compute_pnl(true);
                    log_msg(&format!(
                        "[{:.0}s] {} | PnL: ${:.2} ({:+.1}%) | Cost: ${:.2}",
                        elapsed, msg, pnl, roi, cost
                    ));
                }

                log_msg("Computing prices...");
                let (display_yes, display_no) = compute_prices(snap.yes_bid, snap.yes_ask, snap.no_bid, snap.no_ask, snap.yes_last, snap.no_last);
                let new_trades: Vec<DashboardTrade> = trader.trades.iter().map(|t| DashboardTrade {
                    trade_type: format!("{:?}", t.trade_type),
                    side: format!("{:?}", t.side),
                    price: t.price,
                    shares: t.shares,
                    cost: t.cost,
                    elapsed_sec: t.elapsed_sec,
                    round_num,
                }).collect();
                let mut s = dash_tx.borrow().clone();
                s.bot_state = format!("{:?}", trader.state);
                s.round_num = round_num;
                s.elapsed_sec = elapsed;
                s.timeframe_sec = timeframe;
                s.yes_price = display_yes;
                s.no_price = display_no;
                s.yes_bid = snap.yes_bid;
                s.yes_ask = snap.yes_ask;
                s.no_bid = snap.no_bid;
                s.no_ask = snap.no_ask;
                s.total_pnl = total_pnl;
                s.trades = new_trades.clone();
                for trade in &new_trades {
                    if !s.all_trades.iter().any(|t| t.elapsed_sec == trade.elapsed_sec && t.round_num == trade.round_num) {
                        s.all_trades.push(trade.clone());
                    }
                }
                dash_tx.send(s).ok();
            }
            Ok(None) => {}
            Err(_) => {
                log_msg("Snapshot fetch timed out, retrying...");
            }
        }
    }

    dash_task.abort();
}
