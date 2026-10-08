//! FSRS, the Free Spaced Repetition Scheduler (version 4.5, with its
//! published default weights): how well a word is held, and when to see it
//! again so it's recalled nine times in ten.
//!
//! A word's state is worked out afresh from its reviews, so the log is the
//! only record and nothing here is stored.

const W: [f32; 17] = [
    0.4872, 1.4003, 3.7145, 13.8206, 5.1618, 1.2298, 0.8975, 0.031, 1.6474, 0.1367, 1.0461,
    2.1072, 0.0793, 0.3246, 1.587, 0.2272, 2.8755,
];
const DECAY: f32 = -0.5;
const FACTOR: f32 = 19.0 / 81.0;
pub const DAY: u64 = 86_400;

/// How a review went: again, hard, good, easy.
pub const GRADES: [&str; 4] = ["again", "hard", "good", "easy"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct State {
    /// Days until recall falls to nine in ten.
    pub stability: f32,
    /// One to ten.
    pub difficulty: f32,
    /// When last reviewed, in seconds since 1970.
    pub last: u64,
}

fn first_difficulty(grade: u8) -> f32 {
    (W[4] - (grade as f32 - 3.0) * W[5]).clamp(1.0, 10.0)
}

/// The chance of recall `days` after a review.
fn recall(days: f32, stability: f32) -> f32 {
    (1.0 + FACTOR * days / stability).powf(DECAY)
}

/// The state after a review graded 1 to 4 at `time`.
pub fn next(state: Option<State>, grade: u8, time: u64) -> State {
    let grade = grade.clamp(1, 4);
    let Some(s) = state else {
        return State {
            stability: W[grade as usize - 1],
            difficulty: first_difficulty(grade),
            last: time,
        };
    };
    let days = time.saturating_sub(s.last) as f32 / DAY as f32;
    let r = recall(days, s.stability);
    let d = s.difficulty;
    let stability = if grade == 1 {
        let forgot = W[11]
            * d.powf(-W[12])
            * ((s.stability + 1.0).powf(W[13]) - 1.0)
            * (W[14] * (1.0 - r)).exp();
        forgot.min(s.stability)
    } else {
        let hard = if grade == 2 { W[15] } else { 1.0 };
        let easy = if grade == 4 { W[16] } else { 1.0 };
        s.stability
            * (1.0
                + W[8].exp()
                    * (11.0 - d)
                    * s.stability.powf(-W[9])
                    * ((W[10] * (1.0 - r)).exp() - 1.0)
                    * hard
                    * easy)
    };
    let moved = d - W[6] * (grade as f32 - 3.0);
    let difficulty = (W[7] * first_difficulty(3) + (1.0 - W[7]) * moved).clamp(1.0, 10.0);
    State {
        stability: stability.max(0.01),
        difficulty,
        last: time,
    }
}

/// Whole days to the next review: at nine in ten, the stability itself.
pub fn interval(state: &State) -> u64 {
    (state.stability.round() as u64).clamp(1, 36_500)
}

/// The day, counted from 1970, the word is next due.
pub fn due_day(state: &State) -> u64 {
    state.last / DAY + interval(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_answers_space_out_and_again_brings_it_back() {
        let mut t = 0;
        let mut s = next(None, 3, t);
        let mut gaps = Vec::new();
        for _ in 0..4 {
            t += interval(&s) * DAY;
            s = next(Some(s), 3, t);
            gaps.push(interval(&s));
        }
        assert!(gaps.windows(2).all(|g| g[1] > g[0]), "{gaps:?}");
        let held = s.stability;
        t += interval(&s) * DAY;
        let lapsed = next(Some(s), 1, t);
        assert!(lapsed.stability < held / 2.0);
        assert!(lapsed.difficulty > s.difficulty);
        assert_eq!(interval(&next(None, 1, 0)), 1);
        assert!(interval(&next(None, 4, 0)) > interval(&next(None, 3, 0)));
    }
}
