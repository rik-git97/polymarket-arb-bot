use crate::models::*;
use crate::config::TradingConfig;

/// Derive a meaningful price for a token from its best bid/ask.
/// Returns None when both bid and ask are zero (no liquidity at all).
fn reliable_price(bid: f64, ask: f64) -> Option<f64> {
    match (bid > 0.0, ask > 0.0) {
        (true, true) => Some((bid + ask) / 2.0),
        (true, false) => Some(bid),
        (false, true) => Some(ask),
        (false, false) => None,
    }
}

/// Compute fair Yes/No prices from a snapshot.
/// Prefers last_trade_price when available, then bid/ask midpoint.
/// If one side lacks data, it is inferred from the other (p_yes + p_no = 1).
pub fn compute_prices(
    yes_bid: f64, yes_ask: f64, no_bid: f64, no_ask: f64,
    yes_last: Option<f64>, no_last: Option<f64>,
) -> (f64, f64) {
    // Prefer last_trade_price over midpoint
    let yes_prior = yes_last.or_else(|| reliable_price(yes_bid, yes_ask));
    let no_prior = no_last.or_else(|| reliable_price(no_bid, no_ask));
    match (yes_prior, no_prior) {
        (Some(y), Some(n)) => (y, n),
        (Some(y), None) => (y, (1.0 - y).max(0.0).min(1.0)),
        (None, Some(n)) => ((1.0 - n).max(0.0).min(1.0), n),
        (None, None) => (0.5, 0.5),
    }
}

pub struct DumpHedgeTrader {
    pub state: BotState,
    pub leg1_price: Option<f64>,
    pub leg1_side: Option<Side>,
    pub leg1_time: Option<f64>,
    pub leg2_price: Option<f64>,
    pub leg2_type: Option<TradeType>,
    pub trades: Vec<Trade>,

    // Price history for dump detection
    yes_history: Vec<f64>,
    no_history: Vec<f64>,

    // Config
    move_threshold: f64,
    sum_target: f64,
    window_sec: f64,
    stop_wait: f64,
    shares: f64,
    fee: f64,
}

impl DumpHedgeTrader {
    pub fn new(config: &TradingConfig) -> Self {
        DumpHedgeTrader {
            state: BotState::Idle,
            leg1_price: None,
            leg1_side: None,
            leg1_time: None,
            leg2_price: None,
            leg2_type: None,
            trades: Vec::new(),
            yes_history: Vec::new(),
            no_history: Vec::new(),
            move_threshold: config.dump_move_threshold.unwrap_or(0.15),
            sum_target: config.dump_sum_target.unwrap_or(0.95),
            window_sec: config.dump_window_seconds.unwrap_or(120) as f64,
            stop_wait: config.stop_loss_max_wait_seconds.unwrap_or(300) as f64,
            shares: config.shares_per_leg.unwrap_or(10.0),
            fee: config.fee_rate.unwrap_or(0.02),
        }
    }

    pub fn reset(&mut self) {
        self.state = BotState::Idle;
        self.leg1_price = None;
        self.leg1_side = None;
        self.leg1_time = None;
        self.leg2_price = None;
        self.leg2_type = None;
        self.trades.clear();
        self.yes_history.clear();
        self.no_history.clear();
    }

    pub fn on_snapshot(&mut self, snap: &PriceSnapshot, round_duration_sec: f64) -> Option<String> {
        let e = snap.elapsed_sec;

        match self.state {
            BotState::Idle => self.state = BotState::Watching,

            BotState::Watching => {
                if e > self.window_sec {
                    return None;
                }

                let (yes_price, no_price) = compute_prices(snap.yes_bid, snap.yes_ask, snap.no_bid, snap.no_ask, snap.yes_last, snap.no_last);
                self.yes_history.push(yes_price);
                self.no_history.push(no_price);
                if self.yes_history.len() > 5 {
                    self.yes_history.remove(0);
                    self.no_history.remove(0);
                }

                if self.yes_history.len() >= 3 {
                    let old_y = self.yes_history[0];
                    let old_n = self.no_history[0];
                    let cur_y = yes_price;
                    let cur_n = no_price;

                    let y_drop = if old_y > 0.001 {
                        (old_y - cur_y) / old_y
                    } else {
                        0.0
                    };
                    let n_drop = if old_n > 0.001 {
                        (old_n - cur_n) / old_n
                    } else {
                        0.0
                    };

                    if y_drop >= self.move_threshold {
                        self.state = BotState::Leg1Placed;
                        self.leg1_price = Some(cur_y);
                        self.leg1_side = Some(Side::Yes);
                        self.leg1_time = Some(e);
                        let cost = cur_y * self.shares;
                        self.trades.push(Trade {
                            trade_type: TradeType::Leg1,
                            side: Side::Yes,
                            price: cur_y,
                            shares: self.shares,
                            cost,
                            elapsed_sec: e,
                        });
                        return Some(format!("LEG1 BUY YES @ {:.3} (drop {:.1}%)", cur_y, y_drop * 100.0));
                    } else if n_drop >= self.move_threshold {
                        self.state = BotState::Leg1Placed;
                        self.leg1_price = Some(cur_n);
                        self.leg1_side = Some(Side::No);
                        self.leg1_time = Some(e);
                        let cost = cur_n * self.shares;
                        self.trades.push(Trade {
                            trade_type: TradeType::Leg1,
                            side: Side::No,
                            price: cur_n,
                            shares: self.shares,
                            cost,
                            elapsed_sec: e,
                        });
                        return Some(format!("LEG1 BUY NO @ {:.3} (drop {:.1}%)", cur_n, n_drop * 100.0));
                    }
                }
            }

            BotState::Leg1Placed => {
                let wait = e - self.leg1_time.unwrap();
                let (yes_price, no_price) = compute_prices(snap.yes_bid, snap.yes_ask, snap.no_bid, snap.no_ask, snap.yes_last, snap.no_last);
                let opp_price = if self.leg1_side.as_ref().unwrap() == &Side::Yes {
                    no_price
                } else {
                    yes_price
                };
                let combined = self.leg1_price.unwrap() + opp_price;
                let hedge_side = if self.leg1_side.as_ref().unwrap() == &Side::Yes {
                    Side::No
                } else {
                    Side::Yes
                };
                let remaining = round_duration_sec - e;

                if combined <= self.sum_target {
                    self.state = BotState::Hedging;
                    self.leg2_price = Some(opp_price);
                    self.leg2_type = Some(TradeType::Leg2Hedge);
                    let cost = opp_price * self.shares;
                    self.trades.push(Trade {
                        trade_type: TradeType::Leg2Hedge,
                        side: hedge_side.clone(),
                        price: opp_price,
                        shares: self.shares,
                        cost,
                        elapsed_sec: e,
                    });
                    return Some(format!("LEG2 HEDGE {:?} @ {:.3} (combined {:.3})", hedge_side, opp_price, combined));
                }

                if wait >= self.stop_wait || remaining <= 15.0 {
                    let typ = if wait >= self.stop_wait {
                        TradeType::Leg2StopLoss
                    } else {
                        TradeType::Leg2Final
                    };
                    self.state = BotState::Hedging;
                    self.leg2_price = Some(opp_price);
                    self.leg2_type = Some(typ.clone());
                    let cost = opp_price * self.shares;
                    self.trades.push(Trade {
                        trade_type: typ.clone(),
                        side: hedge_side.clone(),
                        price: opp_price,
                        shares: self.shares,
                        cost,
                        elapsed_sec: e,
                    });
                    return Some(format!("LEG2 {:?} {:?} @ {:.3} (combined {:.3})", typ, hedge_side, opp_price, combined));
                }
            }

            BotState::Hedging => {
                if e >= round_duration_sec - 1.0 {
                    self.state = BotState::Complete;
                    return Some("ROUND COMPLETE".into());
                }
            }

            BotState::Complete => {}
        }

        None
    }

    pub fn compute_pnl(&self, result_up: bool) -> (f64, f64, f64, f64) {
        let leg1 = self.trades.iter().find(|t| t.trade_type == TradeType::Leg1);
        let leg2 = self.trades.iter().find(|t| matches!(t.trade_type, TradeType::Leg2Hedge | TradeType::Leg2StopLoss | TradeType::Leg2Final));

        match (leg1, leg2) {
            (Some(l1), Some(l2)) => {
                let total_cost = l1.cost + l2.cost;
                let win_side = if result_up { &Side::Yes } else { &Side::No };

                let mut payout = 0.0;
                for t in &[l1, l2] {
                    if t.side == *win_side {
                        payout += t.shares * 1.0;
                    }
                }

                let gross_pnl = payout - total_cost;
                let fee = total_cost * self.fee;
                let net_pnl = gross_pnl - fee;
                let roi = if total_cost > 0.0 { net_pnl / total_cost * 100.0 } else { 0.0 };

                (net_pnl, roi, total_cost, payout)
            }
            _ => (0.0, 0.0, 0.0, 0.0),
        }
    }
}
