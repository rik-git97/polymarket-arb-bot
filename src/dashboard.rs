use axum::{
    response::Html,
    routing::get,
    Router,
};

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(index_handler))
}

async fn index_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

static DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Polymarket Dump-and-Hedge Bot</title>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0d1117; color: #c9d1d9; padding: 20px; }
.container { max-width: 800px; margin: 0 auto; }
h1 { color: #58a6ff; margin-bottom: 8px; font-size: 24px; }
.subtitle { color: #8b949e; margin-bottom: 24px; font-size: 14px; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 20px; margin-bottom: 16px; }
.card h2 { color: #f0f6fc; font-size: 16px; margin-bottom: 12px; }
.status-dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 8px; }
.status-dot.off { background: #f85149; }
.status-dot.on { background: #3fb950; }
.info-row { display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #21262d; font-size: 14px; }
.info-row:last-child { border-bottom: none; }
.info-label { color: #8b949e; }
.info-value { color: #f0f6fc; font-weight: 500; }
.placeholder { text-align: center; padding: 40px 0; color: #8b949e; }
.placeholder h3 { color: #f0f6fc; margin-bottom: 8px; font-size: 18px; }
.placeholder p { font-size: 14px; line-height: 1.6; }
code { background: #0d1117; padding: 2px 6px; border-radius: 4px; font-size: 13px; }
</style>
</head>
<body>
<div class="container">
  <h1>DUMP-AND-HEDGE BOT</h1>
  <div class="subtitle">Polymarket 15-min BTC Up/Down · Control Panel</div>

  <div class="card">
    <h2><span class="status-dot off"></span>Status</h2>
    <div class="info-row">
      <span class="info-label">Mode</span>
      <span class="info-value">Offline</span>
    </div>
    <div class="info-row">
      <span class="info-label">Active Markets</span>
      <span class="info-value">0</span>
    </div>
    <div class="info-row">
      <span class="info-label">Open Positions</span>
      <span class="info-value">0</span>
    </div>
    <div class="info-row">
      <span class="info-label">Today P&amp;L</span>
      <span class="info-value">--</span>
    </div>
  </div>

  <div class="card">
    <h2>Configuration</h2>
    <div class="info-row">
      <span class="info-label">Config File</span>
      <span class="info-value"><code>config.json</code></span>
    </div>
    <div class="info-row">
      <span class="info-label">Logging</span>
      <span class="info-value">stdout</span>
    </div>
  </div>

  <div class="card">
    <div class="placeholder">
      <h3>Live Trading Dashboard</h3>
      <p>Connect to Polymarket API endpoints and start monitoring markets.<br>
      Run the CLI bot to begin: <code>cargo run -- --config config.json</code></p>
    </div>
  </div>

  <div class="card">
    <h2>Quick Reference</h2>
    <div class="info-row">
      <span class="info-label">CLI Bot</span>
      <span class="info-value"><code>cargo run</code></span>
    </div>
    <div class="info-row">
      <span class="info-label">Dashboard</span>
      <span class="info-value"><code>cargo run --bin dashboard</code></span>
    </div>
    <div class="info-row">
      <span class="info-label">Release Build</span>
      <span class="info-value"><code>cargo build --release</code></span>
    </div>
  </div>
</div>
</body>
</html>"##;
