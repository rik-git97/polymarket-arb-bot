use clap::Parser;
use polymarket_arb_bot::config::{Cli, Config};
use polymarket_arb_bot::models::*;
use polymarket_arb_bot::trader::DumpHedgeTrader;
use polymarket_arb_bot::simulator::Simulator;
use std::io::Write;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mut cfg = Config::load(&cli.config);

    // CLI flags override config
    if cli.simulation {
        cfg.trading.simulation = Some(true);
    }
    if cli.production {
        cfg.trading.simulation = Some(false);
    }

    if cfg.trading.simulation.unwrap_or(true) {
        run_simulation(&cfg).await;
    } else {
        eprintln!("Live mode not yet implemented. Run with --simulation for backtest.");
    }
}

async fn run_simulation(cfg: &Config) {
    let trading = &cfg.trading;
    let round_sec = trading.timeframe_minutes.unwrap_or(15) as f64 * 60.0;
    let interval_sec = 1.0;
    let check_interval = 1usize;
    let num_rounds = 2000;
    let shares = trading.shares_per_leg.unwrap_or(10.0);

    let mut sim = Simulator::new(42);
    let mut bot = DumpHedgeTrader::new(trading);

    let mut results = Vec::new();

    println!("{}", "=".repeat(70));
    println!("  POLYMARKET DUMP-AND-HEDGE SIMULATION");
    println!("  Timeframe: {} min  |  Threshold: {:.0}%  |  Sum target: {:.2}",
        trading.timeframe_minutes.unwrap_or(15),
        trading.dump_move_threshold.unwrap_or(0.15) * 100.0,
        trading.dump_sum_target.unwrap_or(0.95));
    println!("  Window: {}s  |  Stop-loss: {}s  |  Shares: {:.0}",
        trading.dump_window_seconds.unwrap_or(120),
        trading.stop_loss_max_wait_seconds.unwrap_or(300),
        shares);
    println!("{}", "=".repeat(70));

    let _total_shares: f64 = shares * 500.0 / (shares * 0.5 * 2.0); // estimate capacity

    for round_num in 0..num_rounds {
        bot.reset();
        let (snapshots, result_up) = sim.generate_round_snapshots(
            round_sec, interval_sec, check_interval,
        );

        for snap in &snapshots {
            bot.on_snapshot(snap, round_sec);
        }

        // Finalize
        if matches!(bot.state, BotState::Hedging) {
            bot.state = BotState::Complete;
        }

        let (pnl, roi, cost, payout) = bot.compute_pnl(result_up);
        let has_trades = !bot.trades.is_empty();
        let completed = matches!(bot.state, BotState::Complete) && bot.trades.len() >= 2;

        results.push(RoundResult {
            round_num: round_num as u64,
            start_time: chrono::Utc::now(),
            result_up,
            dump_occurred: false,
            caught: has_trades,
            completed,
            trades: bot.trades.clone(),
            pnl,
            roi,
            total_cost: cost,
            payout,
        });

        if round_num % 500 == 0 && round_num > 0 {
            print!(".");
            std::io::stdout().flush().ok();
        }
    }

    // Analysis
    let total = results.len();
    let completed: Vec<_> = results.iter().filter(|r| r.completed).collect();
    let n_completed = completed.len();
    let wins = completed.iter().filter(|r| r.pnl > 0.0).count();
    let losses = completed.iter().filter(|r| r.pnl <= 0.0).count();
    let total_pnl: f64 = completed.iter().map(|r| r.pnl).sum();

    // Hedge type breakdown
    let mut hedge_count = 0usize;
    let mut hedge_pnl = 0.0;
    let mut sl_count = 0usize;
    let mut sl_pnl = 0.0;

    for r in &completed {
        let has_hedge = r.trades.iter().any(|t| matches!(t.trade_type, TradeType::Leg2Hedge));
        let has_sl = r.trades.iter().any(|t| matches!(t.trade_type, TradeType::Leg2StopLoss));
        if has_hedge {
            hedge_count += 1;
            hedge_pnl += r.pnl;
        } else if has_sl {
            sl_count += 1;
            sl_pnl += r.pnl;
        }
    }

    println!();
    println!("{}", "=".repeat(70));
    println!("  RESULTS ({} rounds simulated)", total);
    println!("{}", "=".repeat(70));
    println!("  Rounds with trades:   {} ({:.1}%)", results.iter().filter(|r| r.caught).count(),
        results.iter().filter(|r| r.caught).count() as f64 / total as f64 * 100.0);
    println!("  Completed (2 legs):   {} ({:.1}% of total)", n_completed,
        n_completed as f64 / total as f64 * 100.0);
    if n_completed > 0 {
        println!("  Wins / Losses:        {} / {}", wins, losses);
        println!("  Win rate:             {:.1}%", wins as f64 / n_completed as f64 * 100.0);
        println!("  Total P&L:            ${:.2}", total_pnl);
        println!("  Avg P&L/trade:        ${:.4}", total_pnl / n_completed as f64);
        println!("  Avg ROI/trade:        {:.2}%",
            completed.iter().map(|r| r.roi).sum::<f64>() / n_completed as f64);
        let daily = n_completed as f64 / total as f64 * 96.0;
        println!("  Est. trades/day:      {:.0}", daily);
        println!("  Est. P&L/month:       ${:.0}", total_pnl / total as f64 * 96.0 * 30.0);

        println!("{}HEDGE TYPE{}", "-".repeat(33), "-".repeat(22));
        println!("  HEDGE trades:  {}  (${:.2} total, ${:.4}/trade)", hedge_count, hedge_pnl,
            if hedge_count > 0 { hedge_pnl / hedge_count as f64 } else { 0.0 });
        println!("  STOPLOSS:       {}  (${:.2} total, ${:.4}/trade)", sl_count, sl_pnl,
            if sl_count > 0 { sl_pnl / sl_count as f64 } else { 0.0 });

        // Sample trades
        let good: Vec<_> = completed.iter().filter(|r| r.pnl > 0.0).collect();
        let bad: Vec<_> = completed.iter().filter(|r| r.pnl <= -1.0).collect();

        println!();
        println!("  SAMPLE PROFITABLE TRADES ({} shown):", good.len().min(5));
        for r in good.iter().take(5) {
            let l1 = r.trades.first().unwrap();
            let l2 = r.trades.iter().find(|t| matches!(t.trade_type, TradeType::Leg2Hedge | TradeType::Leg2StopLoss | TradeType::Leg2Final)).unwrap();
            println!("    R{}: {:?} @ {:.3} -> {:?} @ {:.3} = ${:.2} ({:.1}%)",
                r.round_num, l1.side, l1.price, l2.trade_type, l2.price, r.pnl, r.roi);
        }

        println!();
        println!("  SAMPLE LOSING TRADES ({} shown):", bad.len().min(5));
        for r in bad.iter().take(5) {
            let l1 = r.trades.first().unwrap();
            let l2 = r.trades.iter().find(|t| matches!(t.trade_type, TradeType::Leg2Hedge | TradeType::Leg2StopLoss | TradeType::Leg2Final)).unwrap();
            println!("    R{}: {:?} @ {:.3} -> {:?} @ {:.3} = ${:.2} ({:.1}%)",
                r.round_num, l1.side, l1.price, l2.trade_type, l2.price, r.pnl, r.roi);
        }
    }
    println!("{}", "=".repeat(70));
}
