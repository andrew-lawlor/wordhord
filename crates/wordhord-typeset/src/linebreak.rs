//! Knuth–Plass line breaking: every way to break a paragraph is weighed
//! at once, and the one that spaces all its lines most evenly wins. TeX's
//! algorithm (Knuth and Plass, "Breaking Paragraphs into Lines", 1981).
//!
//! A paragraph is a list of items: boxes (words, fixed width), glue
//! (spaces, which stretch and shrink) and penalties (places a line may
//! break: a hyphen, or a forced end).

/// A penalty this high forbids a break; this low forces one.
pub const INFINITY: f32 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    Box {
        width: f32,
    },
    Glue {
        width: f32,
        stretch: f32,
        shrink: f32,
    },
    /// `width` is what appears if the line breaks here (a hyphen).
    Penalty {
        width: f32,
        cost: f32,
        flagged: bool,
    },
}

impl Item {
    pub fn width(&self) -> f32 {
        match self {
            Item::Box { width } | Item::Glue { width, .. } => *width,
            Item::Penalty { .. } => 0.0,
        }
    }
}

/// Where each line ends (the index of the item it breaks at), and how
/// much each line's glue is stretched (+) or shrunk (−), as a ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct Breaks {
    pub at: Vec<usize>,
    pub ratios: Vec<f32>,
}

#[derive(Clone, Copy)]
struct Node {
    /// The item the line breaks at.
    at: usize,
    /// Lines so far.
    line: usize,
    fitness: usize,
    /// Totals up to the start of the next line.
    width: f32,
    stretch: f32,
    shrink: f32,
    demerits: f64,
    ratio: f32,
    previous: Option<usize>,
}

/// Breaks `items` into lines `width(line)` wide. The items must end with a
/// forced break. Tries a strict tolerance first, then a looser one; if
/// neither finds a way, returns `None` and the caller breaks greedily.
pub fn break_lines(items: &[Item], width: &dyn Fn(usize) -> f32) -> Option<Breaks> {
    [2.0, 6.0, 30.0]
        .into_iter()
        .find_map(|tolerance| attempt(items, width, tolerance))
}

fn attempt(items: &[Item], width: &dyn Fn(usize) -> f32, tolerance: f32) -> Option<Breaks> {
    // Running totals before each item.
    let n = items.len();
    let (mut sum_w, mut sum_y, mut sum_z) = (
        vec![0.0f32; n + 1],
        vec![0.0f32; n + 1],
        vec![0.0f32; n + 1],
    );
    for (k, item) in items.iter().enumerate() {
        sum_w[k + 1] = sum_w[k] + item.width();
        let (y, z) = match item {
            Item::Glue {
                stretch, shrink, ..
            } => (*stretch, *shrink),
            _ => (0.0, 0.0),
        };
        sum_y[k + 1] = sum_y[k] + y;
        sum_z[k + 1] = sum_z[k] + z;
    }
    let mut nodes = vec![Node {
        at: 0,
        line: 0,
        fitness: 1,
        width: 0.0,
        stretch: 0.0,
        shrink: 0.0,
        demerits: 0.0,
        ratio: 0.0,
        previous: None,
    }];
    let mut active: Vec<usize> = vec![0];

    for b in 0..n {
        let (legal, cost, flagged, extra) = match items[b] {
            Item::Penalty {
                width,
                cost,
                flagged,
            } if cost < INFINITY => (true, cost, flagged, width),
            Item::Glue { .. } if b > 0 && matches!(items[b - 1], Item::Box { .. }) => {
                (true, 0.0, false, 0.0)
            }
            _ => (false, 0.0, false, 0.0),
        };
        if !legal {
            continue;
        }
        // The best new node for each fitness class.
        let mut best: [Option<Node>; 4] = [None; 4];
        let mut keep = Vec::with_capacity(active.len());
        for &a in &active {
            let node = nodes[a];
            let length = sum_w[b] - node.width + extra;
            let line_width = width(node.line);
            let ratio = if length < line_width {
                let stretch = sum_y[b] - node.stretch;
                if stretch > 0.0 {
                    (line_width - length) / stretch
                } else {
                    INFINITY
                }
            } else if length > line_width {
                let shrink = sum_z[b] - node.shrink;
                if shrink > 0.0 {
                    (line_width - length) / shrink
                } else {
                    -INFINITY
                }
            } else {
                0.0
            };
            // A line from here would be too tight, now and later; and a
            // forced break ends every line that reaches it.
            let forced = cost <= -INFINITY;
            if ratio >= -1.0 && !forced {
                keep.push(a);
            }
            if (-1.0..=tolerance).contains(&ratio) {
                let badness = 100.0 * (ratio.abs() as f64).powi(3);
                let mut demerits = if cost >= 0.0 {
                    (1.0 + badness + cost as f64).powi(2)
                } else if cost > -INFINITY {
                    (1.0 + badness).powi(2) - (cost as f64).powi(2)
                } else {
                    (1.0 + badness).powi(2)
                };
                let fitness: usize = if ratio < -0.5 {
                    0
                } else if ratio <= 0.5 {
                    1
                } else if ratio <= 1.0 {
                    2
                } else {
                    3
                };
                // Two hyphens in a row, or a tight line after a loose one.
                if flagged {
                    if let Item::Penalty { flagged: true, .. } = items[node.at] {
                        demerits += 3000.0;
                    }
                }
                if fitness.abs_diff(node.fitness) > 1 {
                    demerits += 3000.0;
                }
                let total = node.demerits + demerits;
                if best[fitness].is_none_or(|b| total < b.demerits) {
                    best[fitness] = Some(Node {
                        at: b,
                        line: node.line + 1,
                        fitness,
                        width: 0.0,
                        stretch: 0.0,
                        shrink: 0.0,
                        demerits: total,
                        ratio,
                        previous: Some(a),
                    });
                }
            }
        }
        active = keep;
        // The next line starts after the break, past any glue and
        // penalties there.
        let mut start = b;
        if matches!(items[b], Item::Glue { .. } | Item::Penalty { .. }) {
            start = b + 1;
            while start < n
                && match items[start] {
                    Item::Glue { .. } => true,
                    Item::Penalty { cost, .. } => cost > -INFINITY,
                    Item::Box { .. } => false,
                }
            {
                start += 1;
            }
        }
        for mut node in best.into_iter().flatten() {
            node.width = sum_w[start];
            node.stretch = sum_y[start];
            node.shrink = sum_z[start];
            nodes.push(node);
            active.push(nodes.len() - 1);
        }
        if active.is_empty() {
            return None;
        }
    }
    // The best way to the end.
    let last = n - 1;
    let end = active
        .iter()
        .copied()
        .filter(|&a| nodes[a].at == last)
        .min_by(|&a, &b| nodes[a].demerits.total_cmp(&nodes[b].demerits))?;
    let mut at = Vec::new();
    let mut ratios = Vec::new();
    let mut cursor = Some(end);
    while let Some(k) = cursor {
        if nodes[k].previous.is_none() {
            break;
        }
        at.push(nodes[k].at);
        ratios.push(nodes[k].ratio);
        cursor = nodes[k].previous;
    }
    at.reverse();
    ratios.reverse();
    Some(Breaks { at, ratios })
}

/// The glue and penalty that end a paragraph: the last line is set loose,
/// and must break.
pub fn finish(items: &mut Vec<Item>) {
    items.push(Item::Glue {
        width: 0.0,
        stretch: INFINITY,
        shrink: 0.0,
    });
    items.push(Item::Penalty {
        width: 0.0,
        cost: -INFINITY,
        flagged: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(widths: &[f32]) -> Vec<Item> {
        let mut items = Vec::new();
        for (k, w) in widths.iter().enumerate() {
            if k > 0 {
                items.push(Item::Glue {
                    width: 10.0,
                    stretch: 5.0,
                    shrink: 3.0,
                });
            }
            items.push(Item::Box { width: *w });
        }
        finish(&mut items);
        items
    }

    #[test]
    fn a_short_paragraph_is_one_line() {
        let items = words(&[30.0, 40.0, 20.0]);
        let b = break_lines(&items, &|_| 500.0).unwrap();
        assert_eq!(b.at, [items.len() - 1]);
    }

    #[test]
    fn lines_never_overflow_and_the_last_is_loose() {
        let widths: Vec<f32> = (0..60).map(|k| 20.0 + (k * 37 % 50) as f32).collect();
        let items = words(&widths);
        let b = break_lines(&items, &|_| 300.0).unwrap();
        assert!(b.at.len() > 3);
        for r in &b.ratios[..b.ratios.len() - 1] {
            assert!((-1.0..=2.0).contains(r), "ratio {r}");
        }
        assert_eq!(*b.at.last().unwrap(), items.len() - 1);
    }

    #[test]
    fn it_prefers_even_lines_over_greedy_ones() {
        // Greedy would fill the first line and leave the second nearly
        // empty-tight; total fit balances them.
        let items = words(&[50.0, 50.0, 50.0, 50.0, 50.0, 10.0]);
        let b = break_lines(&items, &|_| 185.0).unwrap();
        assert_eq!(b.at.len(), 2);
    }
}
