use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct PriceSnapshot {
    pub time_ms: i64,
    pub elapsed_sec: f64,
    pub yes_bid: f64,
    pub yes_ask: f64,
    pub no_bid: f64,
    pub no_ask: f64,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub trade_type: TradeType,
    pub side: Side,
    pub price: f64,
    pub shares: f64,
    pub cost: f64,
    pub elapsed_sec: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TradeType {
    Leg1,
    Leg2Hedge,
    Leg2StopLoss,
    Leg2Final,
}

#[derive(Debug, Clone, PartialEq)]
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
    pub start_time: DateTime<Utc>,
    pub result_up: bool,
    pub dump_occurred: bool,
    pub caught: bool,
    pub completed: bool,
    pub trades: Vec<Trade>,
    pub pnl: f64,
    pub roi: f64,
    pub total_cost: f64,
    pub payout: f64,
}

#[derive(Debug, Clone)]
pub struct PeriodState {
    pub condition_id: String,
    pub token_id_yes: String,
    pub token_id_no: String,
    pub start_time: i64,
    pub end_time: i64,
}
