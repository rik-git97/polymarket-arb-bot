use axum::{
    extract::State,
    response::{Html, Json, IntoResponse},
    routing::get,
    Router,
};
use serde::Serialize;
use tokio::sync::watch;
use crate::metrics::PerformanceMetrics;
use crate::analytics::{TradeAnalysis, Recommendation};

#[derive(Debug, Clone, Serialize)]
pub struct ExportData {
    pub trades: Vec<DashboardTrade>,
    pub metrics: Option<PerformanceMetrics>,
    pub analysis: Option<TradeAnalysis>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardTrade {
    pub trade_type: String,
    pub side: String,
    pub price: f64,
    pub shares: f64,
    pub cost: f64,
    pub elapsed_sec: f64,
    pub round_num: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardState {
    pub connected: bool,
    pub question: String,
    pub slug: String,
    pub round_num: u64,
    pub elapsed_sec: f64,
    pub timeframe_sec: f64,
    pub bot_state: String,
    pub yes_price: f64,
    pub no_price: f64,
    pub yes_bid: f64,
    pub yes_ask: f64,
    pub no_bid: f64,
    pub no_ask: f64,
    pub total_pnl: f64,
    pub budget: f64,
    pub trades: Vec<DashboardTrade>,
    pub all_trades: Vec<DashboardTrade>,
    pub metrics: Option<PerformanceMetrics>,
    pub analysis: Option<TradeAnalysis>,
    pub recommendations: Vec<Recommendation>,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            connected: false,
            question: String::new(),
            slug: String::new(),
            round_num: 0,
            elapsed_sec: 0.0,
            timeframe_sec: 900.0,
            bot_state: "Idle".into(),
            yes_price: 0.5,
            no_price: 0.5,
            yes_bid: 0.0,
            yes_ask: 0.0,
            no_bid: 0.0,
            no_ask: 0.0,
            total_pnl: 0.0,
            budget: 500.0,
            trades: Vec::new(),
            all_trades: Vec::new(),
            metrics: None,
            analysis: None,
            recommendations: Vec::new(),
        }
    }
}

pub type SharedState = watch::Receiver<DashboardState>;
pub type StateSender = watch::Sender<DashboardState>;

pub fn create_shared_state() -> (StateSender, SharedState) {
    watch::channel(DashboardState::default())
}

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/api/state", get(api_state_handler))
        .route("/api/metrics", get(api_metrics_handler))
        .route("/api/analysis", get(api_analysis_handler))
        .route("/api/recommendations", get(api_recommendations_handler))
        .route("/api/export/csv", get(api_export_csv_handler))
        .route("/api/export/json", get(api_export_json_handler))
        .with_state(state)
}

async fn index_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn api_state_handler(State(state): State<SharedState>) -> Json<DashboardState> {
    Json(state.borrow().clone())
}

async fn api_metrics_handler(State(state): State<SharedState>) -> Json<Option<PerformanceMetrics>> {
    Json(state.borrow().metrics.clone())
}

async fn api_analysis_handler(State(state): State<SharedState>) -> Json<Option<TradeAnalysis>> {
    Json(state.borrow().analysis.clone())
}

async fn api_recommendations_handler(State(state): State<SharedState>) -> Json<Vec<Recommendation>> {
    Json(state.borrow().recommendations.clone())
}

async fn api_export_csv_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let all_trades = state.borrow().all_trades.clone();
    let mut csv = String::from("RoundNum,Time(s),Type,Side,Price,Shares,Cost\n");
    for trade in &all_trades {
        csv.push_str(&format!(
            "{},{},{},{},{:.3},{:.1},{:.2}\n",
            trade.round_num,
            trade.elapsed_sec as i32,
            trade.trade_type,
            trade.side,
            trade.price,
            trade.shares,
            trade.cost
        ));
    }
    ([(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8")], csv).into_response()
}

async fn api_export_json_handler(State(state): State<SharedState>) -> Json<ExportData> {
    let snapshot = state.borrow().clone();
    Json(ExportData {
        trades: snapshot.all_trades,
        metrics: snapshot.metrics,
        analysis: snapshot.analysis,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })
}

static DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>BTC Up/Down 15m — Arbitrage Bot</title>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0d1117; color: #c9d1d9; padding: 20px; }
.container { max-width: 900px; margin: 0 auto; }
h1 { color: #f7931a; margin-bottom: 4px; font-size: 24px; }
.subtitle { color: #8b949e; margin-bottom: 20px; font-size: 14px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 12px; }
.grid3 { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 12px; margin-bottom: 12px; }
.rec-high { border-left: 3px solid #f85149; }
.rec-medium { border-left: 3px solid #d29922; }
.rec-low { border-left: 3px solid #3fb950; }
.rec-item { padding: 8px; background: #1c2128; border-radius: 4px; font-size: 11px; margin-bottom: 6px; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px; }
.card h2 { color: #f0f6fc; font-size: 14px; margin-bottom: 10px; text-transform: uppercase; letter-spacing: 0.5px; }
.card.full { grid-column: 1 / -1; }
.row { display: flex; justify-content: space-between; padding: 4px 0; font-size: 13px; }
.label { color: #8b949e; }
.value { color: #f0f6fc; font-weight: 500; font-variant-numeric: tabular-nums; }
.up { color: #3fb950; }
.down { color: #f85149; }
.pnl-pos { color: #3fb950; }
.pnl-neg { color: #f85149; }
table { width: 100%; border-collapse: collapse; font-size: 12px; }
th { text-align: left; color: #8b949e; padding: 6px 4px; border-bottom: 1px solid #21262d; font-weight: 500; }
td { padding: 6px 4px; border-bottom: 1px solid #21262d; }
tr:hover { background: #1c2128; }
.badge { display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 11px; font-weight: 500; }
.badge.yes { background: #3fb95022; color: #3fb950; }
.badge.no { background: #f8514922; color: #f85149; }
.badge.hedge { background: #58a6ff22; color: #58a6ff; }
.badge.stop { background: #d2992222; color: #d29922; }
.badge.final { background: #8b949e22; color: #8b949e; }
.conn-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #3fb950; margin-right: 6px; vertical-align: middle; transition: background 0.3s; }
.conn-dot.offline { background: #f85149; }
.stale-banner { display: none; background: #d2992233; color: #d29922; border: 1px solid #d29922; border-radius: 4px; padding: 6px 12px; margin-bottom: 10px; font-size: 12px; }
.offline-banner { display: none; background: #f8514922; color: #f85149; border: 1px solid #f85149; border-radius: 4px; padding: 8px 12px; margin-bottom: 10px; font-size: 13px; font-weight: 500; }
@media (max-width: 600px) { .grid { grid-template-columns: 1fr; } }
</style>
</head>
<body>
<div class="container">
  <h1><span class="conn-dot" id="connDot"></span>₿ BTC UP/DOWN 15m</h1>
  <div class="offline-banner" id="offlineBanner">Bot Offline — Trading stopped or connection lost</div>
  <div class="stale-banner" id="staleBanner">Data may be stale — last update over 3 seconds ago</div>
  <div class="subtitle">Dump-and-Hedge Arbitrage Bot · Paper Trading ($500)</div>

  <div class="grid">
    <div class="card">
      <h2>Market</h2>
      <div class="row"><span class="label">Question</span><span class="value" id="question">-</span></div>
      <div class="row"><span class="label">Bot State</span><span class="value" id="botState">Idle</span></div>
      <div class="row"><span class="label">Round</span><span class="value" id="roundNum">0</span></div>
      <div class="row"><span class="label">Time</span><span class="value" id="elapsed">0s / 15m</span></div>
      <div class="row"><span class="label">Last Update</span><span class="value" id="lastUpdate">-</span></div>
    </div>

    <div class="card">
      <h2>Portfolio</h2>
      <div class="row"><span class="label">Budget</span><span class="value" id="budget">$500.00</span></div>
      <div class="row"><span class="label">Total P&amp;L</span><span class="value" id="totalPnl">$0.00</span></div>
      <div class="row"><span class="label">Equity</span><span class="value" id="equity">$500.00</span></div>
    </div>

    <div class="card">
      <h2>Up (Yes)</h2>
      <div class="row"><span class="label">Price</span><span class="value up" id="yesPrice">-</span></div>
      <div class="row"><span class="label">Bid</span><span class="value" id="yesBid">-</span></div>
      <div class="row"><span class="label">Ask</span><span class="value" id="yesAsk">-</span></div>
    </div>

    <div class="card">
      <h2>Down (No)</h2>
      <div class="row"><span class="label">Price</span><span class="value down" id="noPrice">-</span></div>
      <div class="row"><span class="label">Bid</span><span class="value" id="noBid">-</span></div>
      <div class="row"><span class="label">Ask</span><span class="value" id="noAsk">-</span></div>
    </div>
  </div>

  <div class="card full" style="margin-top: 12px;">
    <h2>Performance Metrics</h2>
    <div class="grid3">
      <div><span class="label">Win Rate</span><div class="value" id="winRate">-</div></div>
      <div><span class="label">Avg PnL/Round</span><div class="value" id="avgPnl">-</div></div>
      <div><span class="label">Sharpe Ratio</span><div class="value" id="sharpe">-</div></div>
      <div><span class="label">Max Drawdown</span><div class="value" id="maxDD">-</div></div>
      <div><span class="label">Total Cost</span><div class="value" id="totalCost">-</div></div>
      <div><span class="label">Avg Trade Cost</span><div class="value" id="avgTradeCost">-</div></div>
    </div>
  </div>

  <div class="card full">
    <h2>Recommendations</h2>
    <div id="recommendations" style="font-size: 11px;"><span style="color: #8b949e;">Analyzing...</span></div>
  </div>

  <div class="card full">
    <h2>Trades</h2>
    <table>
      <thead><tr><th>Round</th><th>Time</th><th>Type</th><th>Side</th><th>Price</th><th>Shares</th><th>Cost</th></tr></thead>
      <tbody id="trades"><tr><td colspan="5" style="text-align:center;color:#8b949e;padding:20px;">No trades yet</td></tr></tbody>
    </table>
  </div>
</div>

<script>
function badge(type) {
  const map = { Leg1: 'yes', Leg2Hedge: 'hedge', Leg2StopLoss: 'stop', Leg2Final: 'final' };
  const cls = map[type] || '';
  return `<span class="badge ${cls}">${type.replace('Leg','L').replace('2StopLoss','2SL').replace('2Final','2F').replace('2Hedge','2H')}</span>`;
}

let lastUpdateTime = Date.now();
let consecutiveFailures = 0;

function setConnectionState(online) {
  const dot = document.getElementById('connDot');
  const banner = document.getElementById('offlineBanner');
  dot.className = online ? 'conn-dot' : 'conn-dot offline';
  banner.style.display = online ? 'none' : 'block';
}

function checkStale() {
  const stale = (Date.now() - lastUpdateTime) > 3000;
  document.getElementById('staleBanner').style.display = stale ? 'block' : 'none';
}

async function poll() {
  try {
    const r = await fetch('/api/state');
    if (!r.ok) throw new Error('HTTP ' + r.status);
    const s = await r.json();

    lastUpdateTime = Date.now();
    consecutiveFailures = 0;
    setConnectionState(true);
    document.getElementById('lastUpdate').textContent = new Date().toLocaleTimeString();

    document.getElementById('question').textContent = s.question || '-';
    document.getElementById('botState').textContent = s.botState;
    document.getElementById('roundNum').textContent = s.roundNum;
    const e = Math.floor(s.elapsedSec);
    const t = Math.floor(s.timeframeSec);
    document.getElementById('elapsed').textContent = `${e}s / ${Math.floor(t/60)}m`;

    document.getElementById('budget').textContent = `$${s.budget.toFixed(2)}`;
    document.getElementById('totalPnl').textContent = s.totalPnl >= 0 ? `+$${s.totalPnl.toFixed(2)}` : `-$${Math.abs(s.totalPnl).toFixed(2)}`;
    document.getElementById('totalPnl').className = s.totalPnl >= 0 ? 'value pnl-pos' : 'value pnl-neg';
    const eq = s.budget + s.totalPnl;
    document.getElementById('equity').textContent = `$${eq.toFixed(2)}`;
    document.getElementById('equity').className = eq >= s.budget ? 'value pnl-pos' : 'value pnl-neg';

    document.getElementById('yesPrice').textContent = s.yesPrice.toFixed(3);
    document.getElementById('yesBid').textContent = s.yesBid.toFixed(3);
    document.getElementById('yesAsk').textContent = s.yesAsk.toFixed(3);
    document.getElementById('noPrice').textContent = s.noPrice.toFixed(3);
    document.getElementById('noBid').textContent = s.noBid.toFixed(3);
    document.getElementById('noAsk').textContent = s.noAsk.toFixed(3);

    if (s.metrics) {
      document.getElementById('winRate').textContent = s.metrics.win_rate.toFixed(1) + '%';
      document.getElementById('avgPnl').textContent = '$' + s.metrics.avg_pnl_per_round.toFixed(2);
      document.getElementById('sharpe').textContent = s.metrics.sharpe_ratio.toFixed(2);
      document.getElementById('maxDD').textContent = '$' + s.metrics.max_drawdown.toFixed(2);
      document.getElementById('totalCost').textContent = '$' + s.metrics.total_cost.toFixed(2);
      document.getElementById('avgTradeCost').textContent = '$' + s.metrics.avg_trade_cost.toFixed(2);
    }

    let recs = [];
    try {
      const recResp = await fetch('/api/recommendations');
      if (recResp.ok) recs = await recResp.json();
    } catch(_) {}
    const recDiv = document.getElementById('recommendations');
    if (recs.length === 0) {
      recDiv.innerHTML = '<span style="color: #8b949e;">No recommendations at this time</span>';
    } else {
      recDiv.innerHTML = recs.map(r => {
        const cls = r.priority === 'HIGH' ? 'rec-high' : r.priority === 'MEDIUM' ? 'rec-medium' : 'rec-low';
        return `<div class="rec-item ${cls}"><strong>${r.title}</strong><br/>${r.description}<br/><small style="color:#58a6ff;">Impact: ${r.potential_impact}</small></div>`;
      }).join('');
    }

    const tb = document.getElementById('trades');
    if (s.allTrades.length === 0) {
      tb.innerHTML = '<tr><td colspan="7" style="text-align:center;color:#8b949e;padding:20px;">No trades yet</td></tr>';
    } else {
      tb.innerHTML = s.allTrades.slice().reverse().map(t =>
        `<tr><td>${t.roundNum}</td><td>${Math.floor(t.elapsedSec)}s</td><td>${badge(t.tradeType)}</td><td>${t.side}</td><td>${t.price.toFixed(3)}</td><td>${t.shares.toFixed(1)}</td><td>$${t.cost.toFixed(2)}</td></tr>`
      ).join('');
    }
  } catch(e) {
    consecutiveFailures++;
    if (consecutiveFailures >= 2) {
      setConnectionState(false);
    }
    console.error('Poll failed:', e);
  }
}

setInterval(checkStale, 1000);
setInterval(poll, 1000);
poll();
</script>
</body>
</html>"##;
