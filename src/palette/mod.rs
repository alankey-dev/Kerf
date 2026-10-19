//! The command palette's model (spec 32): what it lists with an empty query, how it ranks candidates
//! against a query, and the history of runs that lifts recently used ones. Pure: the UI builds the
//! candidates from the command table and owns nothing that decides ranking.

pub mod fuzzy;

use serde::{Deserialize, Serialize};

/// Where a palette row comes from; with an empty query, Recent and Suggested head the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Recent,
    Suggested,
    Commands,
    Shapes,
    Fonts,
    Projects,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Recent => "Recent",
            Self::Suggested => "Suggested",
            Self::Commands => "Command",
            Self::Shapes => "Shape",
            Self::Fonts => "Font",
            Self::Projects => "Project",
        }
    }
}

/// What ranking reads from a row. `key` names the row across sessions.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub aliases: &'a [&'static str],
}

/// One run from the palette: the row's key and when, in seconds since the Unix epoch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Use {
    pub key: String,
    pub at: u64,
}

/// Every palette run, newest first, at most [`RECENT_CAP`]. Kept in `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Recents(Vec<Use>);

pub const RECENT_CAP: usize = 50;
/// How many recent rows an empty query lists.
pub const RECENT_SHOWN: usize = 8;
/// What one use is worth when it is fresh. The boost halves every `HALF_LIFE_S` and never exceeds
/// `MAX_BOOST`, so a well-used command rises among equal matches but cannot bury a better one.
const USE_BOOST: f32 = 2.0;
const HALF_LIFE_S: f32 = 3.0 * 24.0 * 3600.0;
const MAX_BOOST: f32 = 6.0;
/// A match through an alias ranks just below the same match on the label.
const ALIAS_PENALTY: i32 = 2;

impl Recents {
    pub fn record(&mut self, key: &str, at: u64) {
        self.0.insert(0, Use { key: key.to_owned(), at });
        self.0.truncate(RECENT_CAP);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Each key once, most recently run first.
    pub fn latest(&self) -> impl Iterator<Item = &str> {
        let mut seen = Vec::new();
        self.0.iter().map(|u| u.key.as_str()).filter(move |k| {
            let fresh = !seen.contains(k);
            if fresh {
                seen.push(*k);
            }
            fresh
        })
    }

    /// The ranking points `key`'s uses earn at `now`.
    pub fn boost(&self, key: &str, now: u64) -> i32 {
        let decayed = |at: u64| USE_BOOST * 0.5_f32.powf(now.saturating_sub(at) as f32 / HALF_LIFE_S);
        let total: f32 = self.0.iter().filter(|u| u.key == key).map(|u| decayed(u.at)).sum();
        total.min(MAX_BOOST).round() as i32
    }
}

/// The candidates matching `query`, best first, as indices into `candidates`. Equal scores keep the
/// candidates' order.
pub fn rank(query: &str, candidates: &[Candidate], recents: &Recents, now: u64) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let label = fuzzy::score(query, c.label);
            let alias = c.aliases.iter().filter_map(|a| fuzzy::score(query, a)).max().map(|s| s - ALIAS_PENALTY);
            Some((label.max(alias)? + recents.boost(c.key, now), i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// What an empty query lists: the recently run rows that still exist, then the suggested ones.
pub fn browse(keys: &[&str], recents: &Recents, suggested: &[usize]) -> Vec<(Section, usize)> {
    let recent = recents.latest().filter_map(|k| keys.iter().position(|key| *key == k)).take(RECENT_SHOWN);
    recent.map(|i| (Section::Recent, i)).chain(suggested.iter().map(|&i| (Section::Suggested, i))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3600;
    const NOW: u64 = 1_800_000_000;

    fn candidates<'a>(labels: &'a [&'a str]) -> Vec<Candidate<'a>> {
        labels.iter().map(|l| Candidate { key: l, label: l, aliases: &[] }).collect()
    }

    fn ranked<'a>(query: &str, labels: &'a [&'a str], recents: &Recents) -> Vec<&'a str> {
        rank(query, &candidates(labels), recents, NOW).into_iter().map(|i| labels[i]).collect()
    }

    #[test]
    fn five_uses_today_lift_an_equal_match() {
        let labels = ["Flip Horizontal", "Flip Vertical"];
        assert_eq!(ranked("flip", &labels, &Recents::default())[0], "Flip Horizontal", "ties keep the list order");
        let mut recents = Recents::default();
        for hour in 0..5 {
            recents.record("Flip Vertical", NOW - hour * 3600);
        }
        assert_eq!(ranked("flip", &labels, &recents)[0], "Flip Vertical");
    }

    #[test]
    fn an_exact_prefix_beats_a_weak_match_used_often() {
        let labels = ["Half Lip", "Flip Vertical"];
        let mut recents = Recents::default();
        for _ in 0..RECENT_CAP {
            recents.record("Half Lip", NOW);
        }
        assert_eq!(ranked("flip", &labels, &recents)[0], "Flip Vertical");
    }

    #[test]
    fn uses_fade_with_age() {
        let mut recents = Recents::default();
        recents.record("Weld", NOW - 30 * DAY);
        assert_eq!(recents.boost("Weld", NOW), 0);
        recents.record("Weld", NOW);
        assert_eq!(recents.boost("Weld", NOW), 2);
        assert_eq!(recents.boost("Slice", NOW), 0);
    }

    #[test]
    fn aliases_match_just_below_the_label() {
        let labels = [Candidate { key: "Weld", label: "Weld", aliases: &[] }, Candidate { key: "Unite", label: "Unite", aliases: &["weld"] }];
        assert_eq!(rank("weld", &labels, &Recents::default(), NOW), [0, 1]);
        let delete = [Candidate { key: "Delete", label: "Delete", aliases: &["remove"] }];
        assert_eq!(rank("remove", &delete, &Recents::default(), NOW), [0]);
    }

    #[test]
    fn recents_keep_the_newest_fifty() {
        let mut recents = Recents::default();
        for i in 0..60 {
            recents.record(&format!("c{i}"), NOW + i);
        }
        assert_eq!(recents.len(), RECENT_CAP);
        assert_eq!(recents.latest().next(), Some("c59"));
        assert_eq!(recents.latest().last(), Some("c10"));
    }

    #[test]
    fn an_empty_query_lists_recent_runs_once_then_suggestions() {
        let mut recents = Recents::default();
        for key in ["Weld", "Gone", "Attach", "Weld"] {
            recents.record(key, NOW);
        }
        let keys = ["Attach", "Weld", "Align Left"];
        let rows = browse(&keys, &recents, &[0, 2]);
        assert_eq!(rows, [(Section::Recent, 1), (Section::Recent, 0), (Section::Suggested, 0), (Section::Suggested, 2)]);
    }
}
