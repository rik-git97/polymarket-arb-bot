use crate::config::{Config, TradingConfig};
use crate::models::*;
use crate::simulator::Simulator;
use crate::trader::DumpHedgeTrader;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
}

pub fn create_router(config: Config) -> Router {
    let state = AppState {
        config: Arc::new(Mutex::new(config)),
    };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/simulate", post(simulate_handler))
        .route("/api/health", get(health_handler))
        .layer(
            tower_http::cors::CorsLayer::permissive()
        )
        .with_state(state)
}

#[derive(Deserialize)]
pub struct SimulateRequest {
    pub threshold: Option<f64>,
    pub sum_target: Option<f64>,
    pub window_sec: Option<u64>,
    pub stop_wait_sec: Option<u64>,
    pub shares: Option<f64>,
    pub num_rounds: Option<u64>,
    pub timeframe_min: Option<u32>,
}

#[derive(Serialize)]
pub struct TradeSample {
    pub round: u64,
    pub leg1_side: String,
    pub leg1_price: f64,
    pub leg2_type: String,
    pub leg2_price: f64,
    pub pnl: f64,
    pub roi: f64,
}

#[derive(Serialize)]
pub struct SimulateResponse {
    pub total_rounds: usize,
    pub rounds_with_trades: usize,
    pub completed: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_pnl: f64,
    pub avg_roi: f64,
    pub est_trades_per_day: f64,
    pub est_pnl_month: f64,
    pub hedge_count: usize,
    pub hedge_pnl: f64,
    pub sl_count: usize,
    pub sl_pnl: f64,
    pub pnl_distribution: Vec<f64>,
    pub roi_distribution: Vec<f64>,
    pub sample_wins: Vec<TradeSample>,
    pub sample_losses: Vec<TradeSample>,
    pub total_cost: f64,
    pub total_payout: f64,
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn index_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

async fn simulate_handler(
    State(state): State<AppState>,
    Json(req): Json<SimulateRequest>,
) -> Result<Json<SimulateResponse>, StatusCode> {
    let cfg = state.config.lock().await;
    let trading = &cfg.trading;

    let round_sec = req.timeframe_min.unwrap_or(trading.timeframe_minutes.unwrap_or(15)) as f64 * 60.0;
    let interval_sec = 1.0;
    let check_interval = 1usize;
    let num_rounds = req.num_rounds.unwrap_or(2000) as usize;
    let shares = req.shares.unwrap_or(trading.shares_per_leg.unwrap_or(10.0));

    let mut sim = Simulator::new(42);

    let trading_config = TradingConfig {
        simulation: trading.simulation,
        asset: trading.asset.clone(),
        timeframe_minutes: Some((round_sec / 60.0) as u32),
        check_interval_ms: trading.check_interval_ms,
        dump_move_threshold: req.threshold.or(trading.dump_move_threshold),
        dump_sum_target: req.sum_target.or(trading.dump_sum_target),
        dump_window_seconds: req.window_sec.or(trading.dump_window_seconds),
        stop_loss_max_wait_seconds: req.stop_wait_sec.or(trading.stop_loss_max_wait_seconds),
        shares_per_leg: Some(shares),
        fee_rate: trading.fee_rate,
        stop_loss_percentage: trading.stop_loss_percentage,
    };

    let mut results = Vec::with_capacity(num_rounds);

    for round_num in 0..num_rounds {
        let mut bot = DumpHedgeTrader::new(&trading_config);
        let (snapshots, result_up) = sim.generate_round_snapshots(
            round_sec, interval_sec, check_interval,
        );

        for snap in &snapshots {
            bot.on_snapshot(snap, round_sec);
        }

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
    }

    let total = results.len();
    let completed_r: Vec<_> = results.iter().filter(|r| r.completed).collect();
    let n_completed = completed_r.len();
    let wins = completed_r.iter().filter(|r| r.pnl > 0.0).count();
    let losses = completed_r.iter().filter(|r| r.pnl <= 0.0).count();
    let total_pnl: f64 = completed_r.iter().map(|r| r.pnl).sum();
    let total_cost: f64 = completed_r.iter().map(|r| r.total_cost).sum();
    let total_payout: f64 = completed_r.iter().map(|r| r.payout).sum();

    let mut hedge_count = 0usize;
    let mut hedge_pnl = 0.0;
    let mut sl_count = 0usize;
    let mut sl_pnl = 0.0;

    for r in &completed_r {
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

    let pnl_distribution: Vec<f64> = completed_r.iter().map(|r| r.pnl).collect();
    let roi_distribution: Vec<f64> = completed_r.iter().map(|r| r.roi).collect();

    let good: Vec<_> = completed_r.iter().filter(|r| r.pnl > 0.0).collect();
    let bad: Vec<_> = completed_r.iter().filter(|r| r.pnl <= -1.0).collect();

    let sample_wins = good.iter().take(10).map(|r| {
        let l1 = r.trades.first().unwrap();
        let l2 = r.trades.iter().find(|t| matches!(t.trade_type, TradeType::Leg2Hedge | TradeType::Leg2StopLoss | TradeType::Leg2Final)).unwrap();
        TradeSample {
            round: r.round_num,
            leg1_side: format!("{:?}", l1.side),
            leg1_price: l1.price,
            leg2_type: format!("{:?}", l2.trade_type),
            leg2_price: l2.price,
            pnl: r.pnl,
            roi: r.roi,
        }
    }).collect();

    let sample_losses = bad.iter().take(10).map(|r| {
        let l1 = r.trades.first().unwrap();
        let l2 = r.trades.iter().find(|t| matches!(t.trade_type, TradeType::Leg2Hedge | TradeType::Leg2StopLoss | TradeType::Leg2Final)).unwrap();
        TradeSample {
            round: r.round_num,
            leg1_side: format!("{:?}", l1.side),
            leg1_price: l1.price,
            leg2_type: format!("{:?}", l2.trade_type),
            leg2_price: l2.price,
            pnl: r.pnl,
            roi: r.roi,
        }
    }).collect();

    let avg_roi = if n_completed > 0 {
        completed_r.iter().map(|r| r.roi).sum::<f64>() / n_completed as f64
    } else {
        0.0
    };

    let daily = n_completed as f64 / total as f64 * (24.0 * 60.0 / (round_sec / 60.0));

    Ok(Json(SimulateResponse {
        total_rounds: total,
        rounds_with_trades: results.iter().filter(|r| r.caught).count(),
        completed: n_completed,
        wins,
        losses,
        win_rate: if n_completed > 0 { wins as f64 / n_completed as f64 * 100.0 } else { 0.0 },
        total_pnl,
        avg_pnl: if n_completed > 0 { total_pnl / n_completed as f64 } else { 0.0 },
        avg_roi,
        est_trades_per_day: daily,
        est_pnl_month: if total > 0 { total_pnl / total as f64 * daily * 30.0 } else { 0.0 },
        hedge_count,
        hedge_pnl,
        sl_count,
        sl_pnl,
        pnl_distribution,
        roi_distribution,
        sample_wins,
        sample_losses,
        total_cost,
        total_payout,
    }))
}

static DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Polymarket Dump-and-Hedge Bot</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4"></script>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0d1117; color: #c9d1d9; padding: 20px; }
.container { max-width: 1200px; margin: 0 auto; }
h1 { color: #58a6ff; margin-bottom: 8px; font-size: 24px; }
.subtitle { color: #8b949e; margin-bottom: 24px; font-size: 14px; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 20px; margin-bottom: 16px; }
.card h2 { color: #f0f6fc; font-size: 16px; margin-bottom: 16px; padding-bottom: 8px; border-bottom: 1px solid #30363d; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px; }
.stat { margin-bottom: 12px; }
.stat-label { color: #8b949e; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; }
.stat-value { color: #f0f6fc; font-size: 22px; font-weight: 600; }
.stat-value.green { color: #3fb950; }
.stat-value.red { color: #f85149; }
.stat-value.yellow { color: #d29922; }
.param-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }
.param-group { display: flex; flex-direction: column; gap: 4px; }
.param-group label { color: #8b949e; font-size: 12px; }
.param-group input { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 8px 12px; color: #c9d1d9; font-size: 14px; }
.param-group input:focus { outline: none; border-color: #58a6ff; }
.btn { background: #238636; color: #ffffff; border: 1px solid #2ea043; border-radius: 6px; padding: 10px 24px; font-size: 14px; font-weight: 600; cursor: pointer; transition: background 0.2s; }
.btn:hover { background: #2ea043; }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.btn-danger { background: #da3633; border-color: #f85149; }
.btn-danger:hover { background: #f85149; }
.chart-container { position: relative; height: 300px; }
table { width: 100%; border-collapse: collapse; font-size: 13px; }
th { text-align: left; padding: 8px 12px; background: #0d1117; color: #8b949e; font-weight: 500; border-bottom: 1px solid #30363d; }
td { padding: 8px 12px; border-bottom: 1px solid #21262d; }
.status-bar { display: flex; gap: 8px; align-items: center; margin-top: 12px; }
.spinner { display: none; width: 16px; height: 16px; border: 2px solid #30363d; border-top-color: #58a6ff; border-radius: 50%; animation: spin 0.8s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.controls { display: flex; gap: 12px; align-items: end; margin-top: 16px; }
.toast { display: none; position: fixed; bottom: 20px; right: 20px; background: #21262d; border: 1px solid #30363d; border-radius: 8px; padding: 12px 20px; color: #c9d1d9; font-size: 13px; }
@media (max-width: 768px) { .grid { grid-template-columns: 1fr 1fr; } }
</style>
</head>
<body>
<div class="container">
  <h1>DUMP-AND-HEDGE BOT</h1>
  <div class="subtitle">Polymarket 15-min BTC Up/Down · Simulation Dashboard</div>

  <div class="card">
    <h2>Parameters</h2>
    <div class="param-grid">
      <div class="param-group"><label>Dump Threshold (%)</label><input type="number" id="threshold" value="15" step="1" min="1" max="50"></div>
      <div class="param-group"><label>Sum Target</label><input type="number" id="sum_target" value="0.95" step="0.01" min="0.5" max="1.5"></div>
      <div class="param-group"><label>Window (sec)</label><input type="number" id="window_sec" value="120" step="10" min="10" max="600"></div>
      <div class="param-group"><label>Stop-Loss (sec)</label><input type="number" id="stop_wait_sec" value="300" step="10" min="30" max="900"></div>
      <div class="param-group"><label>Shares per Leg</label><input type="number" id="shares" value="10" step="1" min="1" max="1000"></div>
      <div class="param-group"><label>Rounds</label><input type="number" id="num_rounds" value="2000" step="500" min="100" max="50000"></div>
    </div>
    <div class="controls">
      <button class="btn" id="runBtn" onclick="runSimulation()">Run Simulation</button>
      <div class="spinner" id="spinner"></div>
    </div>
  </div>

  <div id="results" style="display:none;">
    <div class="card">
      <h2>Summary</h2>
      <div class="grid">
        <div><div class="stat-label">Total Rounds</div><div class="stat-value" id="totalRounds">-</div></div>
        <div><div class="stat-label">Completed</div><div class="stat-value" id="completed">-</div></div>
        <div><div class="stat-label">Win Rate</div><div class="stat-value green" id="winRate">-</div></div>
        <div><div class="stat-label">Total P&amp;L</div><div class="stat-value green" id="totalPnl">-</div></div>
        <div><div class="stat-label">Avg P&amp;L/Trade</div><div class="stat-value" id="avgPnl">-</div></div>
        <div><div class="stat-label">Avg ROI</div><div class="stat-value yellow" id="avgRoi">-</div></div>
        <div><div class="stat-label">Est Trades/Day</div><div class="stat-value" id="estDaily">-</div></div>
        <div><div class="stat-label">Est P&amp;L/Month</div><div class="stat-value green" id="estMonthly">-</div></div>
      </div>
    </div>

    <div class="card">
      <h2>P&amp;L Distribution</h2>
      <div class="chart-container"><canvas id="pnlChart"></canvas></div>
    </div>

    <div class="card">
      <h2>Hedge Type Breakdown</h2>
      <div class="grid">
        <div><div class="stat-label">Hedge Trades</div><div class="stat-value" id="hedgeCount">-</div></div>
        <div><div class="stat-label">Hedge P&amp;L</div><div class="stat-value green" id="hedgePnl">-</div></div>
        <div><div class="stat-label">Stop-Loss Trades</div><div class="stat-value" id="slCount">-</div></div>
        <div><div class="stat-label">Stop-Loss P&amp;L</div><div class="stat-value red" id="slPnl">-</div></div>
      </div>
    </div>

    <div class="card">
      <h2>ROI Distribution</h2>
      <div class="chart-container"><canvas id="roiChart"></canvas></div>
    </div>
  </div>
</div>

<div class="toast" id="toast"></div>

<script>
let pnlChart = null;
let roiChart = null;

function showToast(msg, isError) {
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.style.display = 'block';
  t.style.borderColor = isError ? '#f85149' : '#30363d';
  setTimeout(() => t.style.display = 'none', 4000);
}

async function runSimulation() {
  const btn = document.getElementById('runBtn');
  const spinner = document.getElementById('spinner');
  btn.disabled = true;
  spinner.style.display = 'inline-block';

  const body = {
    threshold: parseFloat(document.getElementById('threshold').value) / 100,
    sum_target: parseFloat(document.getElementById('sum_target').value),
    window_sec: parseInt(document.getElementById('window_sec').value),
    stop_wait_sec: parseInt(document.getElementById('stop_wait_sec').value),
    shares: parseFloat(document.getElementById('shares').value),
    num_rounds: parseInt(document.getElementById('num_rounds').value),
  };

  try {
    const res = await fetch('/api/simulate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const data = await res.json();
    renderResults(data);
    showToast('Simulation complete: ' + data.total_rounds + ' rounds');
  } catch (e) {
    showToast('Error: ' + e.message, true);
  } finally {
    btn.disabled = false;
    spinner.style.display = 'none';
  }
}

function renderResults(d) {
  document.getElementById('results').style.display = 'block';

  document.getElementById('totalRounds').textContent = d.total_rounds;
  document.getElementById('completed').textContent = d.completed + ' (' + (d.rounds_with_trades/d.total_rounds*100).toFixed(1) + '%)';
  document.getElementById('winRate').textContent = d.win_rate.toFixed(1) + '%';
  document.getElementById('totalPnl').textContent = '$' + d.total_pnl.toFixed(2);
  document.getElementById('avgPnl').textContent = '$' + d.avg_pnl.toFixed(4);
  document.getElementById('avgRoi').textContent = d.avg_roi.toFixed(2) + '%';
  document.getElementById('estDaily').textContent = d.est_trades_per_day.toFixed(0);
  document.getElementById('estMonthly').textContent = '$' + d.est_pnl_month.toFixed(0);

  document.getElementById('hedgeCount').textContent = d.hedge_count;
  document.getElementById('hedgePnl').textContent = '$' + d.hedge_pnl.toFixed(2);
  document.getElementById('slCount').textContent = d.sl_count;
  document.getElementById('slPnl').textContent = '$' + d.sl_pnl.toFixed(2);

  // P&L distribution chart
  if (pnlChart) pnlChart.destroy();
  const bins = makeBins(d.pnl_distribution, 30);
  pnlChart = new Chart(document.getElementById('pnlChart'), {
    type: 'bar',
    data: {
      labels: bins.labels,
      datasets: [{
        label: 'Trades',
        data: bins.counts,
        backgroundColor: bins.labels.map(l => l < 0 ? '#f8514980' : '#3fb95080'),
        borderColor: bins.labels.map(l => l < 0 ? '#f85149' : '#3fb950'),
        borderWidth: 1,
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { ticks: { color: '#8b949e', maxTicksLimit: 15 }, grid: { color: '#21262d' } },
        y: { ticks: { color: '#8b949e' }, grid: { color: '#21262d' } }
      }
    }
  });

  // ROI distribution chart
  if (roiChart) roiChart.destroy();
  const roiBins = makeBins(d.roi_distribution, 30);
  roiChart = new Chart(document.getElementById('roiChart'), {
    type: 'bar',
    data: {
      labels: roiBins.labels,
      datasets: [{
        label: 'Trades',
        data: roiBins.counts,
        backgroundColor: '#58a6ff80',
        borderColor: '#58a6ff',
        borderWidth: 1,
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { ticks: { color: '#8b949e', maxTicksLimit: 15, callback: v => v + '%' }, grid: { color: '#21262d' } },
        y: { ticks: { color: '#8b949e' }, grid: { color: '#21262d' } }
      }
    }
  });
}

function makeBins(values, count) {
  if (values.length === 0) return { labels: [], counts: [] };
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const binWidth = range / count;
  const bins = new Array(count).fill(0);
  const labels = [];
  for (let i = 0; i < count; i++) {
    const lo = min + i * binWidth;
    const hi = lo + binWidth;
    labels.push(+(lo + binWidth / 2).toFixed(2));
    for (const v of values) {
      if (v >= lo && (i === count - 1 ? v <= hi : v < hi)) bins[i]++;
    }
  }
  return { labels, counts: bins };
}
</script>
</body>
</html>"##;
