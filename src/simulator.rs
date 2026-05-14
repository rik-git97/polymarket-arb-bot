use crate::models::PriceSnapshot;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand_distr::StandardNormal;

pub struct Simulator {
    rng: StdRng,
    vol_estimate: f64,
    noise_std: f64,
}

impl Simulator {
    pub fn new(seed: u64) -> Self {
        Simulator {
            rng: StdRng::seed_from_u64(seed),
            vol_estimate: 0.004,  // 0.4% expected vol per 15-min
            noise_std: 0.015,
        }
    }

    pub fn generate_round_snapshots(
        &mut self,
        round_duration_sec: f64,
        interval_sec: f64,
        check_interval: usize,
    ) -> (Vec<PriceSnapshot>, bool) {
        let num_steps = (round_duration_sec / interval_sec) as usize;
        let mut snapshots = Vec::new();

        // Simulate BTC price path with a random walk + drift
        let open_price = 100.0;  // normalized
        let mut current_price: f64 = open_price;
        let mut peak = open_price;
        let mut trough = open_price;
        let mut peak_idx = 0usize;
        let mut trough_idx = 0usize;
        let mut btc_path = vec![open_price];

        for i in 1..num_steps {
            let step_vol = self.vol_estimate * (round_duration_sec / 300.0).sqrt();
            let _drift = 0.0;  // no drift
            let ret: f64 = self.rng.sample(StandardNormal);
            current_price *= 1.0 + ret * step_vol;
            current_price = current_price.max(open_price * 0.98).min(open_price * 1.02);
            btc_path.push(current_price);

            if current_price > peak {
                peak = current_price;
                peak_idx = i;
            }
            if current_price < trough {
                trough = current_price;
                trough_idx = i;
            }
        }

        // Detect: did BTC experience a "flip" (reversal)?
        // A flip = went up >0.08% then reversed >30% of the move OR vice versa
        let _pct_change = (current_price - open_price) / open_price;
        let up_move = peak - open_price;
        let down_move = open_price - trough;
        let has_flip;
        let bump_time;

        if up_move / open_price > 0.0008 && peak_idx < num_steps / 2 {
            let drop = peak - current_price;
            has_flip = drop > up_move * 0.3;
            bump_time = Some(peak_idx);
        } else if down_move / open_price > 0.0008 && trough_idx < num_steps / 2 {
            let rise = current_price - trough;
            has_flip = rise > down_move * 0.3;
            bump_time = Some(trough_idx);
        } else {
            has_flip = false;
            bump_time = None;
        }

        // Generate Polymarket orderbook snapshots
        for i in (0..num_steps).step_by(check_interval) {
            let sec = i as f64 * interval_sec;
            let progress = sec / round_duration_sec;
            let px = btc_path[i];
            let pct = (px - open_price) / open_price * 100.0;

            // Base fair odds using growing sensitivity
            let sensitivity = 200.0 + progress * 250.0;
            let mut fair_yes = (50.0 + pct * sensitivity).max(1.0).min(99.0) / 100.0;

            // Add flip effect
            if has_flip {
                let flip_time = bump_time.unwrap() as f64 * interval_sec;
                if sec >= flip_time {
                    let dist = (sec - flip_time) / 15.0;
                    if dist <= 1.0 {
                        let effect = (0.30 + 0.15) * (1.0 - dist * 0.5);
                        // Dump on the side that was winning before the flip
                        fair_yes += effect; // simplified: one side gets pumped temporarily
                    }
                }
                // Hedge opportunity: overshoot on opposite side
                let hedge_time = bump_time.unwrap() as f64 * interval_sec + 2.0;
                if sec >= hedge_time && sec < hedge_time + 25.0 {
                    let dist = (sec - hedge_time) / 23.0;
                    if dist <= 1.0 {
                        let hedge_effect = 0.08 * (1.0 - dist);
                        fair_yes -= hedge_effect;
                    }
                }
            }

            // Noise
            let noise: f64 = self.rng.sample(StandardNormal);
            fair_yes = (fair_yes + noise * self.noise_std).clamp(0.01, 0.99);
            let fair_no = 1.0 - fair_yes;

            let spread = 0.015 + (fair_yes - 0.5).abs() * 0.02;
            let hs = spread / 2.0;

            snapshots.push(PriceSnapshot {
                time_ms: (sec * 1000.0) as i64,
                elapsed_sec: sec,
                yes_bid: (fair_yes - hs).clamp(0.001, 0.999),
                yes_ask: (fair_yes + hs).clamp(0.001, 0.999),
                no_bid: (fair_no - hs).clamp(0.001, 0.999),
                no_ask: (fair_no + hs).clamp(0.001, 0.999),
            });
        }

        let result_up = current_price >= open_price;
        (snapshots, result_up)
    }
}
