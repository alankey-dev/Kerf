//! Fuzzy subsequence matching for the command palette. Every query letter must appear in the label in
//! order; case, diacritics and the query's spaces are ignored. Consecutive letters, word starts and a
//! match on the label's first letter score higher, and skipped letters cost a little.

const MATCH: i32 = 1;
const CONSECUTIVE: i32 = 6;
const WORD_START: i32 = 8;
const LABEL_START: i32 = 12;
const GAP: i32 = 1;
const MAX_GAP: i32 = 4;
const MAX_LEADING_GAP: i32 = 6;

/// `c` lowercased with its accent removed, so "é" matches "e".
pub fn fold(c: char) -> char {
    const ACCENTED: [(&str, char); 16] = [
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏő", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşš", 's'),
        ("ţťŧ", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    let lower = c.to_lowercase().next().unwrap_or(c);
    ACCENTED.iter().find(|(from, _)| from.contains(lower)).map_or(lower, |&(_, to)| to)
}

/// How well `query` matches `label`, higher is better; `None` when it does not match at all. An empty
/// query matches everything with 0.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(fold).collect();
    if q.is_empty() {
        return Some(0);
    }
    let raw: Vec<char> = label.chars().collect();
    let t: Vec<char> = raw.iter().copied().map(fold).collect();
    let word_start = |j: usize| j == 0 || !raw[j - 1].is_alphanumeric() || (raw[j].is_uppercase() && raw[j - 1].is_lowercase());
    let bonus = |j: usize| MATCH + if word_start(j) { WORD_START } else { 0 } + if j == 0 { LABEL_START } else { 0 };

    // best[j]: the best score with the query so far matched and its last letter at label position j.
    let mut best: Vec<Option<i32>> = (0..t.len()).map(|j| (t[j] == q[0]).then(|| bonus(j) - (j as i32 * GAP).min(MAX_LEADING_GAP))).collect();
    for &qc in &q[1..] {
        let mut next = vec![None; t.len()];
        // The best earlier match two or more letters back, less a gap cost per skipped letter that
        // stops growing at `MAX_GAP`, so a word start further on can still win.
        let (mut carry, mut floor): (Option<i32>, Option<i32>) = (None, None);
        for j in 1..t.len() {
            let consecutive = best[j - 1].map(|s| s + CONSECUTIVE);
            if t[j] == qc {
                next[j] = consecutive.max(carry).max(floor).map(|s| s + bonus(j));
            }
            carry = carry.max(best[j - 1]).map(|s| s - GAP);
            floor = floor.max(best[j - 1].map(|s| s - MAX_GAP));
        }
        best = next;
    }
    best.into_iter().flatten().max().map(|s| s - t.len() as i32 / 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top<'a>(query: &str, labels: &[&'a str]) -> Vec<&'a str> {
        let mut scored: Vec<_> = labels.iter().filter_map(|l| score(query, l).map(|s| (s, *l))).collect();
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        scored.into_iter().map(|(_, l)| l).collect()
    }

    #[test]
    fn letters_must_appear_in_order() {
        assert!(score("wld", "Weld").is_some());
        assert!(score("dlw", "Weld").is_none());
        assert!(score("weldx", "Weld").is_none());
        assert_eq!(score("", "Weld"), Some(0));
        assert_eq!(score("  ", "Weld"), Some(0));
    }

    #[test]
    fn case_diacritics_and_spaces_are_ignored() {
        assert_eq!(score("WELD", "weld"), score("weld", "Weld"));
        assert_eq!(score("creme", "Crème Brûlée"), score("crème", "Crème Brûlée"));
        assert!(score("brulee", "Crème Brûlée").is_some());
        assert_eq!(score("al c", "Align Centre"), score("alc", "Align Centre"));
    }

    #[test]
    fn consecutive_letters_and_word_starts_win() {
        assert!(score("weld", "Weld").unwrap() > score("weld", "Wavy Edge Line Dash").unwrap(), "consecutive beats scattered word starts");
        assert!(score("dh", "Distribute Horizontal").unwrap() > score("dh", "Dashed").unwrap(), "word starts beat inner letters");
        assert!(score("flip", "Flip Vertical").unwrap() > score("flip", "Half Lip").unwrap(), "a match at the start of the label wins");
    }

    #[test]
    fn camel_case_humps_are_word_starts() {
        assert!(score("sg", "SpaceGrotesk").unwrap() > score("sg", "Sausage").unwrap());
    }

    #[test]
    fn ranks_the_palette_examples() {
        let labels = ["Undo", "Move Forward", "Detach", "Weld", "Wavy Cut", "Show", "Align Left", "Align Centre", "Align Vertical", "Select All"];
        assert_eq!(top("wld", &labels)[0], "Weld");
        let al_c = top("al c", &labels);
        assert_eq!(al_c[0], "Align Centre");
        let pos = |l: &str| al_c.iter().position(|x| *x == l);
        assert!(pos("Align Left").is_none_or(|left| left > 0), "{al_c:?}");
    }
}
