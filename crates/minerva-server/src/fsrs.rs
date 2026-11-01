//! FSRS (Free Spaced Repetition Scheduler, version 4.5), the scheduler Anki uses:
//! a card's memory is its stability (days until recall drops to 90%) and its
//! difficulty (1–10). Each review updates both from the rating and from how much
//! the card had been forgotten, and the next review comes when recall is expected
//! to fall to the desired retention. Parameters are FSRS's published defaults.
//! See <https://github.com/open-spaced-repetition/fsrs4anki/wiki/The-Algorithm>.

const W: [f64; 17] = [
    0.4872, 1.4003, 3.7145, 13.8206, 5.1618, 1.2298, 0.8975, 0.031, 1.6474, 0.1367, 1.0461, 2.1072,
    0.0793, 0.3246, 1.587, 0.2272, 2.8755,
];
const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
/// Recall probability the schedule aims for.
const RETENTION: f64 = 0.9;
const DAY: u64 = 86400;
/// A forgotten card comes back after this many seconds, within the session.
pub const RELEARN: u64 = 600;
const MAX_DAYS: f64 = 3650.0;

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
fn initial_difficulty(g: u8) -> f64 {
    (W[4] - (g as f64 - 3.0) * W[5]).clamp(1.0, 10.0)
}
/// The memory after reviewing with grade `g` (1–4), `days` after the previous
/// review; `None` for a new card.
pub fn review(memory: Option<Memory>, g: u8, days: f64) -> Memory {
    let Some(m) = memory else {
        return Memory {
            stability: W[g as usize - 1],
            difficulty: initial_difficulty(g),
        };
    };
    let r = retrievability(days.max(0.0), m.stability);
    let difficulty = {
        let next = m.difficulty - W[6] * (g as f64 - 3.0);
        // Mean reversion towards the difficulty of an "easy" first answer.
        (W[7] * initial_difficulty(4) + (1.0 - W[7]) * next).clamp(1.0, 10.0)
    };
    let stability = if g == 1 {
        let forgotten = W[11]
            * m.difficulty.powf(-W[12])
            * ((m.stability + 1.0).powf(W[13]) - 1.0)
            * (W[14] * (1.0 - r)).exp();
        forgotten.min(m.stability)
    } else {
        let hard = if g == 2 { W[15] } else { 1.0 };
        let easy = if g == 4 { W[16] } else { 1.0 };
        m.stability
            * (1.0
                + W[8].exp()
                    * (11.0 - m.difficulty)
                    * m.stability.powf(-W[9])
                    * ((W[10] * (1.0 - r)).exp() - 1.0)
                    * hard
                    * easy)
    };
    Memory {
        stability: stability.max(0.01),
        difficulty,
    }
}
/// Seconds until the next review: a short relearning step after "again",
/// otherwise when recall is expected to reach the desired retention.
pub fn interval(memory: Memory, g: u8) -> u64 {
    if g == 1 {
        return RELEARN;
    }
    let days = memory.stability / FACTOR * (RETENTION.powf(1.0 / DECAY) - 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_reviews_follow_the_default_parameters() {
        let good = review(None, 3, 0.0);
        assert_eq!(good.stability, W[2]);
        assert_eq!(interval(good, 3), 4 * DAY);
        assert_eq!(interval(review(None, 4, 0.0), 4), 14 * DAY);
        assert_eq!(interval(review(None, 1, 0.0), 1), RELEARN);
        assert!(review(None, 1, 0.0).difficulty > review(None, 4, 0.0).difficulty);
    }
    #[test]
    fn intervals_grow_with_success_and_shrink_after_lapses() {
        let mut m = review(None, 3, 0.0);
        let mut last = interval(m, 3);
        for _ in 0..5 {
            let days = last as f64 / DAY as f64;
            m = review(Some(m), 3, days);
            let next = interval(m, 3);
            assert!(next > last, "{next} after {last}");
            last = next;
        }
        let lapsed = review(Some(m), 1, last as f64 / DAY as f64);
        assert!(lapsed.stability < m.stability);
        assert!(lapsed.difficulty > m.difficulty);
        // Hard < Good < Easy, from the same memory.
        let days = last as f64 / DAY as f64;
        let [h, g, e] = [2, 3, 4].map(|grade| interval(review(Some(m), grade, days), grade));
        assert!(h < g && g < e, "{h} {g} {e}");
        // Reviewing again at once barely changes a memory.
        assert_eq!(review(Some(m), 3, 0.0).stability, m.stability);
        assert!(interval(review(Some(m), 4, 1e9), 4) <= MAX_DAYS as u64 * DAY);
    }
    #[test]
    fn earlier_intervals_become_memories() {
        assert_eq!(from_interval(600), None);
        assert_eq!(from_interval(10 * DAY).unwrap().stability, 10.0);
    }
}
