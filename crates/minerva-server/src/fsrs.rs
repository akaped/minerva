//! FSRS (Free Spaced Repetition Scheduler, version 4.5), the scheduler Anki uses:
//! a card's memory is its stability (days until recall drops to 90%) and its
//! difficulty (1–10). Each review updates both from the rating and from how much
//! the card had been forgotten, and the next review comes when recall is expected
//! to fall to the desired retention.
//!
//! The 17 weights start as FSRS's published defaults; `fit` tunes them to one
//! person's review history, as Anki's "Optimize" does.
//! See <https://github.com/open-spaced-repetition/fsrs4anki/wiki/The-Algorithm>.

/// FSRS 4.5's default weights.
pub const DEFAULT_WEIGHTS: [f64; 17] = [
    0.4872, 1.4003, 3.7145, 13.8206, 5.1618, 1.2298, 0.8975, 0.031, 1.6474, 0.1367, 1.0461, 2.1072,
    0.0793, 0.3246, 1.587, 0.2272, 2.8755,
];
/// The range each weight may take while fitting (as in fsrs-rs).
const BOUNDS: [(f64, f64); 17] = [
    (0.1, 100.0),
    (0.1, 100.0),
    (0.1, 100.0),
    (0.1, 100.0),
    (1.0, 10.0),
    (0.1, 5.0),
    (0.1, 5.0),
    (0.0, 0.75),
    (0.0, 4.5),
    (0.0, 0.8),
    (0.01, 3.5),
    (0.1, 5.0),
    (0.01, 0.25),
    (0.01, 0.9),
    (0.01, 4.0),
    (0.0, 1.0),
    (1.0, 6.0),
];
const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
/// The recall probability schedules aim for, unless the person chooses another.
pub const DEFAULT_RETENTION: f64 = 0.9;
/// The desired retentions a person may choose.
pub const RETENTION_RANGE: (f64, f64) = (0.7, 0.97);
const DAY: u64 = 86400;
/// A forgotten card comes back after this many seconds, within the session.
pub const RELEARN: u64 = 600;
const MAX_DAYS: f64 = 3650.0;

/// One person's scheduling: the weights and the recall to aim for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub w: [f64; 17],
    pub retention: f64,
}
impl Default for Params {
    fn default() -> Self {
        Params {
            w: DEFAULT_WEIGHTS,
            retention: DEFAULT_RETENTION,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Memory {
    pub stability: f64,
    pub difficulty: f64,
}

/// Again, Hard, Good, Easy as 1–4.
pub fn grade(rating: &str) -> Option<u8> {
    match rating {
        "again" => Some(1),
        "hard" => Some(2),
        "good" => Some(3),
        "easy" => Some(4),
        _ => None,
    }
}
/// Expected recall after `days` for a memory of `stability` days.
pub fn retrievability(days: f64, stability: f64) -> f64 {
    (1.0 + FACTOR * days / stability).powf(DECAY)
}
fn initial_difficulty(w: &[f64; 17], g: u8) -> f64 {
    (w[4] - (g as f64 - 3.0) * w[5]).clamp(1.0, 10.0)
}
/// The memory after reviewing with grade `g` (1–4), `days` after the previous
/// review; `None` for a new card.
pub fn review(p: &Params, memory: Option<Memory>, g: u8, days: f64) -> Memory {
    let w = &p.w;
    let Some(m) = memory else {
        return Memory {
            stability: w[g as usize - 1].max(0.01),
            difficulty: initial_difficulty(w, g),
        };
    };
    let r = retrievability(days.max(0.0), m.stability);
    let difficulty = {
        let next = m.difficulty - w[6] * (g as f64 - 3.0);
        // Mean reversion towards the difficulty of an "easy" first answer.
        (w[7] * initial_difficulty(w, 4) + (1.0 - w[7]) * next).clamp(1.0, 10.0)
    };
    let stability = if g == 1 {
        let forgotten = w[11]
            * m.difficulty.powf(-w[12])
            * ((m.stability + 1.0).powf(w[13]) - 1.0)
            * (w[14] * (1.0 - r)).exp();
        forgotten.min(m.stability)
    } else {
        let hard = if g == 2 { w[15] } else { 1.0 };
        let easy = if g == 4 { w[16] } else { 1.0 };
        m.stability
            * (1.0
                + w[8].exp()
                    * (11.0 - m.difficulty)
                    * m.stability.powf(-w[9])
                    * ((w[10] * (1.0 - r)).exp() - 1.0)
                    * hard
                    * easy)
    };
    Memory {
        stability: stability.clamp(0.01, 36500.0),
        difficulty,
    }
}
/// Seconds until the next review: a short relearning step after "again",
/// otherwise when recall is expected to reach the desired retention.
pub fn interval(p: &Params, memory: Memory, g: u8) -> u64 {
    if g == 1 {
        return RELEARN;
    }
    let days = memory.stability / FACTOR * (p.retention.powf(1.0 / DECAY) - 1.0);
    (days.round().clamp(1.0, MAX_DAYS) as u64) * DAY
}
/// The memory of a card scheduled before FSRS, from its last interval: stability
/// as that interval, and a middling difficulty.
pub fn from_interval(seconds: u64) -> Option<Memory> {
    (seconds >= DAY).then(|| Memory {
        stability: seconds as f64 / DAY as f64,
        difficulty: 5.0,
    })
}

/// One review in a card's history: when (Unix seconds) and the grade (1–4).
pub type Review = (u64, u8);

/// A card's reviews as FSRS learns from them: the first review of each day,
/// in order, with the days since the previous one.
fn days_of(history: &[Review]) -> Vec<(f64, u8)> {
    let mut out: Vec<(f64, u8)> = vec![];
    let mut last: Option<u64> = None;
    for &(at, g) in history {
        if !(1..=4).contains(&g) {
            continue;
        }
        match last {
            Some(prev) if at / DAY == prev / DAY => continue,
            Some(prev) => out.push(((at.saturating_sub(prev)) as f64 / DAY as f64, g)),
            None => out.push((0.0, g)),
        }
        last = Some(at);
    }
    out
}
/// The memory a card's history leads to.
pub fn replay(p: &Params, history: &[Review]) -> Option<Memory> {
    days_of(history)
        .into_iter()
        .fold(None, |m, (days, g)| Some(review(p, m, g, days)))
}

/// How well `p` predicts the histories: the average log loss of predicting each
/// review's recall (anything but "again") from the memory before it.
fn loss(p: &Params, cards: &[Vec<(f64, u8)>]) -> (f64, usize) {
    let (mut total, mut n) = (0.0, 0);
    for reviews in cards {
        let mut m: Option<Memory> = None;
        for &(days, g) in reviews {
            if let Some(memory) = m {
                let r = retrievability(days, memory.stability).clamp(1e-4, 1.0 - 1e-4);
                total -= if g > 1 { r.ln() } else { (1.0 - r).ln() };
                n += 1;
            }
            m = Some(review(p, m, g, days));
        }
    }
    (if n > 0 { total / n as f64 } else { 0.0 }, n)
}

/// The fewest predicted reviews `fit` needs, as fsrs-rs asks.
pub const MIN_REVIEWS: usize = 400;
/// The most predicted reviews `fit` looks at.
const SAMPLE: usize = 20_000;

/// The result of tuning: the weights, and the loss before and after.
#[derive(Debug)]
pub struct Fitted {
    pub w: [f64; 17],
    pub reviews: usize,
    pub loss_before: f64,
    pub loss_after: f64,
}

/// Tunes the weights to the histories, starting from the defaults: Adam on
/// numerical gradients, with each weight kept in its range and gently pulled
/// back towards its default, so a small history cannot run away. `None` with
/// fewer than `MIN_REVIEWS` predictable reviews.
pub fn fit(histories: &[Vec<Review>]) -> Option<Fitted> {
    let mut cards: Vec<Vec<(f64, u8)>> = histories
        .iter()
        .map(|h| days_of(h))
        .filter(|r| r.len() > 1)
        .collect();
    // A long history is sampled evenly, so fitting takes seconds, not minutes.
    let predicted: usize = cards.iter().map(|c| c.len() - 1).sum();
    if predicted > SAMPLE {
        let every = predicted.div_ceil(SAMPLE);
        cards = cards.into_iter().step_by(every).collect();
    }
    let start = Params::default();
    let (loss_before, reviews) = loss(&start, &cards);
    if reviews < MIN_REVIEWS {
        return None;
    }
    // Pull towards the defaults, weaker with more data.
    let pull = 1.0 / reviews as f64;
    let objective = |w: &[f64; 17]| -> f64 {
        let p = Params { w: *w, ..start };
        let prior: f64 = w
            .iter()
            .zip(DEFAULT_WEIGHTS)
            .zip(BOUNDS)
            .map(|((x, d), (lo, hi))| ((x - d) / (hi - lo)).powi(2))
            .sum();
        loss(&p, &cards).0 + pull * prior
    };
    let mut w = DEFAULT_WEIGHTS;
    let (mut m1, mut m2) = ([0.0; 17], [0.0; 17]);
    let (rate, beta1, beta2) = (0.02, 0.9, 0.999);
    let steps = 120;
    for t in 1..=steps {
        let mut grad = [0.0; 17];
        for i in 0..17 {
            let (lo, hi) = BOUNDS[i];
            let h = (hi - lo) * 1e-4;
            let (mut up, mut down) = (w, w);
            up[i] = (w[i] + h).min(hi);
            down[i] = (w[i] - h).max(lo);
            grad[i] = (objective(&up) - objective(&down)) / (up[i] - down[i]).max(1e-12);
        }
        for i in 0..17 {
            let (lo, hi) = BOUNDS[i];
            // Steps are taken in each weight's own scale.
            let g = grad[i] * (hi - lo);
            m1[i] = beta1 * m1[i] + (1.0 - beta1) * g;
            m2[i] = beta2 * m2[i] + (1.0 - beta2) * g * g;
            let mh = m1[i] / (1.0 - beta1.powi(t));
            let vh = m2[i] / (1.0 - beta2.powi(t));
            w[i] = (w[i] - rate * (hi - lo) * mh / (vh.sqrt() + 1e-8)).clamp(lo, hi);
        }
    }
    let loss_after = loss(&Params { w, ..start }, &cards).0;
    Some(Fitted {
        w,
        reviews,
        loss_before,
        loss_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: Params = Params {
        w: DEFAULT_WEIGHTS,
        retention: DEFAULT_RETENTION,
    };
    #[test]
    fn first_reviews_follow_the_default_parameters() {
        let good = review(&P, None, 3, 0.0);
        assert_eq!(good.stability, DEFAULT_WEIGHTS[2]);
        assert_eq!(interval(&P, good, 3), 4 * DAY);
        assert_eq!(interval(&P, review(&P, None, 4, 0.0), 4), 14 * DAY);
        assert_eq!(interval(&P, review(&P, None, 1, 0.0), 1), RELEARN);
        assert!(review(&P, None, 1, 0.0).difficulty > review(&P, None, 4, 0.0).difficulty);
    }
    #[test]
    fn intervals_grow_with_success_and_shrink_after_lapses() {
        let mut m = review(&P, None, 3, 0.0);
        let mut last = interval(&P, m, 3);
        for _ in 0..5 {
            let days = last as f64 / DAY as f64;
            m = review(&P, Some(m), 3, days);
            let next = interval(&P, m, 3);
            assert!(next > last, "{next} after {last}");
            last = next;
        }
        let lapsed = review(&P, Some(m), 1, last as f64 / DAY as f64);
        assert!(lapsed.stability < m.stability);
        assert!(lapsed.difficulty > m.difficulty);
        // Hard < Good < Easy, from the same memory.
        let days = last as f64 / DAY as f64;
        let [h, g, e] =
            [2, 3, 4].map(|grade| interval(&P, review(&P, Some(m), grade, days), grade));
        assert!(h < g && g < e, "{h} {g} {e}");
        // Reviewing again at once barely changes a memory.
        assert_eq!(review(&P, Some(m), 3, 0.0).stability, m.stability);
        assert!(interval(&P, review(&P, Some(m), 4, 1e9), 4) <= MAX_DAYS as u64 * DAY);
    }
    #[test]
    fn retention_moves_intervals() {
        let m = review(&P, None, 3, 0.0);
        let relaxed = Params {
            retention: 0.8,
            ..P
        };
        let strict = Params {
            retention: 0.95,
            ..P
        };
        assert!(interval(&relaxed, m, 3) > interval(&P, m, 3));
        assert!(interval(&strict, m, 3) < interval(&P, m, 3));
    }
    #[test]
    fn earlier_intervals_become_memories() {
        assert_eq!(from_interval(600), None);
        assert_eq!(from_interval(10 * DAY).unwrap().stability, 10.0);
    }
    #[test]
    fn histories_replay_once_a_day() {
        let h = [(0, 3), (60, 1), (4 * DAY, 3)];
        let m = replay(&P, &h).unwrap();
        let expected = review(&P, Some(review(&P, None, 3, 0.0)), 3, 4.0);
        assert_eq!(m, expected, "the second review that day is left out");
        assert_eq!(replay(&P, &[]), None);
    }

    /// Histories of someone who remembers better than the defaults expect:
    /// reviews at growing intervals, almost always recalled.
    fn strong_memory() -> Vec<Vec<Review>> {
        (0..120u64)
            .map(|card| {
                let mut at = card * 3600;
                let mut out = vec![(at, 3)];
                for (i, days) in [3u64, 9, 25, 60, 140].iter().enumerate() {
                    at += days * DAY;
                    let forgot = (card + i as u64) % 23 == 0;
                    out.push((at, if forgot { 1 } else { 3 }));
                }
                out
            })
            .collect()
    }
    #[test]
    fn fitting_learns_from_a_history() {
        assert!(fit(&strong_memory()[..10]).is_none(), "too few reviews");
        let fitted = fit(&strong_memory()).unwrap();
        assert!(fitted.reviews >= MIN_REVIEWS);
        assert!(
            fitted.loss_after < fitted.loss_before,
            "{} → {}",
            fitted.loss_before,
            fitted.loss_after
        );
        // Stronger memories: a first "good" lasts longer than the default 4 days.
        let p = Params { w: fitted.w, ..P };
        assert!(interval(&p, review(&p, None, 3, 0.0), 3) > 4 * DAY);
        for (x, (lo, hi)) in fitted.w.iter().zip(BOUNDS) {
            assert!((lo..=hi).contains(x));
        }
    }
}
