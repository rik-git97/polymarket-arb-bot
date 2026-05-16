#[derive(Debug, Clone)]
pub struct PriceSnapshot {
    pub time_ms: i64,
    pub elapsed_sec: f64,
    pub yes_bid: f64,
    pub yes_ask: f64,
    pub no_bid: f64,
    pub no_ask: f64,
    pub yes_last: Option<f64>,
    pub no_last: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Trade {
    pub trade_type: TradeType,
    pub side: Side,
    pub price: f64,
    pub shares: f64,
    pub cost: f64,
    pub elapsed_sec: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TradeType {
    Leg1,
    Leg2Hedge,
    Leg2StopLoss,
    Leg2Final,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Yes,
    No,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BotState {
    Idle,
    Watching,
    Leg1Placed,
    Hedging,
    Complete,
}

#[derive(Debug, Clone)]
pub struct RoundResult {
    pub round_num: u64,
    pub start_time: i64,
    pub result_up: bool,
    pub completed: bool,
    pub trades: Vec<Trade>,
    pub pnl: f64,
    pub roi: f64,
    pub total_cost: f64,
    pub payout: f64,
}
