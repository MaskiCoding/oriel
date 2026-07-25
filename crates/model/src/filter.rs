use crate::{Window, WindowId};

/// Match quality, best → worst. Compare with [`Tier::rank`] (higher is better).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Exact,
    Prefix,
    WordPrefix,
    Substring,
    Acronym,
    Fuzzy,
}

impl Tier {
    /// Higher is better (Exact = 5 … Fuzzy = 0).
    pub const fn rank(self) -> u8 {
        match self {
            Self::Exact => 5,
            Self::Prefix => 4,
            Self::WordPrefix => 3,
            Self::Substring => 2,
            Self::Acronym => 1,
            Self::Fuzzy => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    App,
    Title,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub tier: Tier,
    pub field: Field,
    pub spans: Vec<(usize, usize)>,
}

/// Folded view of a string with byte-span mapping back into the original.
struct Folded<'a> {
    original: &'a str,
    /// Folded characters.
    chars: Vec<char>,
    /// For each folded char: (`byte_start`, `byte_len`) in `original`.
    origins: Vec<(usize, usize)>,
    /// Whether each folded index starts a word (separators / camelCase / digits).
    word_start: Vec<bool>,
}

impl<'a> Folded<'a> {
    fn new(original: &'a str) -> Self {
        let raw: Vec<(usize, char)> = original.char_indices().collect();
        let mut chars = Vec::new();
        let mut origins = Vec::new();
        let mut word_start = Vec::new();

        for (i, &(byte_start, c)) in raw.iter().enumerate() {
            let byte_len = c.len_utf8();
            let mut is_start = is_word_start(&raw, i);
            for fc in fold_char(c) {
                chars.push(fc);
                origins.push((byte_start, byte_len));
                word_start.push(is_start);
                is_start = false;
            }
        }

        Self {
            original,
            chars,
            origins,
            word_start,
        }
    }

    /// Byte span in original covering folded indices `start..end` (end exclusive).
    fn span(&self, start: usize, end: usize) -> (usize, usize) {
        if start >= end || start >= self.origins.len() {
            return (0, 0);
        }
        let (byte_start, _) = self.origins[start];
        let (last_start, last_len) = self.origins[end - 1];
        (byte_start, last_start + last_len - byte_start)
    }

    fn span_one(&self, i: usize) -> (usize, usize) {
        self.origins[i]
    }
}

fn is_separator(c: char) -> bool {
    matches!(c, ' ' | '-' | '_' | '.' | '/')
}

fn is_word_start(raw: &[(usize, char)], i: usize) -> bool {
    let c = raw[i].1;
    if is_separator(c) {
        return false;
    }
    if i == 0 {
        return true;
    }
    let prev = raw[i - 1].1;
    if is_separator(prev) {
        return true;
    }
    // camelCase hump: lower → Upper
    if prev.is_lowercase() && c.is_uppercase() {
        return true;
    }
    // digit ↔ letter boundary
    let prev_digit = prev.is_ascii_digit();
    let curr_digit = c.is_ascii_digit();
    let prev_alpha = prev.is_alphabetic();
    let curr_alpha = c.is_alphabetic();
    (prev_digit && curr_alpha) || (prev_alpha && curr_digit)
}

/// Latin-ish fold: lowercase + strip common diacritics. May expand (ß → ss).
fn fold_char(c: char) -> Vec<char> {
    if c == 'ß' || c == 'ẞ' {
        return vec!['s', 's'];
    }
    let base = match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'Ā' | 'ā' | 'Ă'
        | 'ă' | 'Ą' | 'ą' | 'Æ' | 'æ' => 'a', // ponytail: æ→a; full Unicode fold later
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Ĉ' | 'ĉ' | 'Ċ' | 'ċ' | 'Č' | 'č' => 'c',
        'Ď' | 'ď' | 'Đ' | 'đ' | 'Ð' | 'ð' => 'd',
        'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' | 'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę'
        | 'ę' | 'Ě' | 'ě' => 'e',
        'Ĝ' | 'ĝ' | 'Ğ' | 'ğ' | 'Ġ' | 'ġ' | 'Ģ' | 'ģ' => 'g',
        'Ĥ' | 'ĥ' | 'Ħ' | 'ħ' => 'h',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' | 'Ĩ' | 'ĩ' | 'Ī' | 'ī' | 'Ĭ' | 'ĭ' | 'Į'
        | 'į' | 'İ' | 'ı' => 'i',
        'Ĵ' | 'ĵ' => 'j',
        'Ķ' | 'ķ' => 'k',
        'Ĺ' | 'ĺ' | 'Ļ' | 'ļ' | 'Ľ' | 'ľ' | 'Ŀ' | 'ŀ' | 'Ł' | 'ł' => 'l',
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ņ' | 'ņ' | 'Ň' | 'ň' => 'n',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ō' | 'ō' | 'Ŏ'
        | 'ŏ' | 'Ő' | 'ő' | 'Œ' | 'œ' => 'o',
        'Ŕ' | 'ŕ' | 'Ŗ' | 'ŗ' | 'Ř' | 'ř' => 'r',
        'Ś' | 'ś' | 'Ŝ' | 'ŝ' | 'Ş' | 'ş' | 'Š' | 'š' => 's',
        'Ţ' | 'ţ' | 'Ť' | 'ť' | 'Ŧ' | 'ŧ' | 'Þ' | 'þ' => 't',
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' | 'Ũ' | 'ũ' | 'Ū' | 'ū' | 'Ŭ' | 'ŭ' | 'Ů'
        | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => 'u',
        'Ŵ' | 'ŵ' => 'w',
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' | 'Ÿ' => 'y',
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => 'z',
        _ => c,
    };
    let lower = if base == c {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        base
    };
    vec![lower]
}

fn fold_query(query: &str) -> Vec<char> {
    query.chars().flat_map(fold_char).collect()
}

/// Best match of `query` against one target string, or None.
pub fn match_str(query: &str, target: &str) -> Option<(Tier, Vec<(usize, usize)>)> {
    if query.is_empty() {
        return Some((Tier::Exact, Vec::new()));
    }
    let q = fold_query(query);
    if q.is_empty() {
        return None;
    }
    let t = Folded::new(target);
    if t.chars.is_empty() {
        return None;
    }

    if let Some(spans) = match_exact(&q, &t) {
        return Some((Tier::Exact, spans));
    }
    if let Some(spans) = match_prefix(&q, &t) {
        return Some((Tier::Prefix, spans));
    }
    if let Some(spans) = match_word_prefix(&q, &t) {
        return Some((Tier::WordPrefix, spans));
    }
    if let Some(spans) = match_substring(&q, &t) {
        return Some((Tier::Substring, spans));
    }
    if let Some(spans) = match_acronym(&q, &t) {
        return Some((Tier::Acronym, spans));
    }
    if let Some(spans) = match_fuzzy(&q, &t) {
        return Some((Tier::Fuzzy, spans));
    }
    None
}

fn match_exact(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    if q == t.chars.as_slice() {
        Some(vec![(0, t.original.len())])
    } else {
        None
    }
}

fn match_prefix(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    if q.len() <= t.chars.len() && t.chars[..q.len()] == *q {
        Some(vec![t.span(0, q.len())])
    } else {
        None
    }
}

fn match_word_prefix(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    for i in 0..t.chars.len() {
        if !t.word_start[i] {
            continue;
        }
        if i == 0 {
            // Index 0 is already covered by Prefix tier when it matches.
            continue;
        }
        let end = i + q.len();
        if end <= t.chars.len() && t.chars[i..end] == *q {
            return Some(vec![t.span(i, end)]);
        }
    }
    None
}

fn match_substring(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    if q.len() > t.chars.len() {
        return None;
    }
    // Skip i==0 (Prefix) and word starts (WordPrefix); find a mid-word hit.
    for i in 1..=t.chars.len().saturating_sub(q.len()) {
        if t.word_start[i] {
            continue;
        }
        if t.chars[i..i + q.len()] == *q {
            return Some(vec![t.span(i, i + q.len())]);
        }
    }
    None
}

fn match_acronym(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    let initials: Vec<usize> = t
        .word_start
        .iter()
        .enumerate()
        .filter_map(|(i, &ws)| if ws { Some(i) } else { None })
        .collect();
    if q.len() > initials.len() {
        return None;
    }
    for start in 0..=initials.len().saturating_sub(q.len()) {
        let mut ok = true;
        for (k, &qc) in q.iter().enumerate() {
            if t.chars[initials[start + k]] != qc {
                ok = false;
                break;
            }
        }
        if ok {
            let spans = (0..q.len())
                .map(|k| t.span_one(initials[start + k]))
                .collect();
            return Some(spans);
        }
    }
    None
}

fn fuzzy_budget(query_len: usize) -> usize {
    query_len / 2
}

/// In-order subsequence; gaps = unmatched folded chars between first and last match.
fn match_fuzzy(q: &[char], t: &Folded<'_>) -> Option<Vec<(usize, usize)>> {
    let budget = fuzzy_budget(q.len());
    // Greedy leftmost subsequence.
    let mut qi = 0;
    let mut matched: Vec<usize> = Vec::with_capacity(q.len());
    for (ti, &tc) in t.chars.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            matched.push(ti);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    let first = matched[0];
    let last = matched[matched.len() - 1];
    let window = last - first + 1;
    let gaps = window - q.len();
    if gaps > budget {
        return None;
    }
    Some(matched.iter().map(|&i| t.span_one(i)).collect())
}

/// Best match of `query` against a window (`app_name` weighted above title).
pub fn match_window(query: &str, w: &Window) -> Option<Match> {
    if query.is_empty() {
        return Some(Match {
            tier: Tier::Exact,
            field: Field::App,
            spans: Vec::new(),
        });
    }
    let app = match_str(query, &w.app_name).map(|(tier, spans)| Match {
        tier,
        field: Field::App,
        spans,
    });
    let title = match_str(query, &w.title).map(|(tier, spans)| Match {
        tier,
        field: Field::Title,
        spans,
    });
    match (app, title) {
        (Some(a), Some(t)) => match a.tier.rank().cmp(&t.tier.rank()) {
            std::cmp::Ordering::Less => Some(t),
            std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => Some(a),
        },
        (Some(a), None) => Some(a),
        (None, Some(t)) => Some(t),
        (None, None) => None,
    }
}

/// Reorder candidates: matches first (best tier, App over Title, then stable input
/// order), non-matches after in input order.
pub fn filter(query: &str, windows: &[Window]) -> Vec<(WindowId, Option<Match>)> {
    if query.is_empty() {
        return windows
            .iter()
            .map(|w| {
                (
                    w.id,
                    Some(Match {
                        tier: Tier::Exact,
                        field: Field::App,
                        spans: Vec::new(),
                    }),
                )
            })
            .collect();
    }

    let mut matched: Vec<(usize, WindowId, Match)> = Vec::new();
    let mut unmatched: Vec<(WindowId, Option<Match>)> = Vec::new();

    for (i, w) in windows.iter().enumerate() {
        match match_window(query, w) {
            Some(m) => matched.push((i, w.id, m)),
            None => unmatched.push((w.id, None)),
        }
    }

    matched.sort_by(|a, b| {
        b.2.tier.rank().cmp(&a.2.tier.rank()).then_with(|| {
            let field_rank = |f: Field| match f {
                Field::App => 1u8,
                Field::Title => 0,
            };
            field_rank(b.2.field)
                .cmp(&field_rank(a.2.field))
                .then_with(|| a.0.cmp(&b.0))
        })
    });

    let mut out: Vec<(WindowId, Option<Match>)> = matched
        .into_iter()
        .map(|(_, id, m)| (id, Some(m)))
        .collect();
    out.extend(unmatched);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowState;

    fn win(id: u32, app_name: &str, title: &str) -> Window {
        Window {
            id: WindowId(id),
            app: 1,
            app_name: app_name.into(),
            title: title.into(),
            state: WindowState::decode(0x0300_0001_0008_2401, 0x3),
            fullscreen: false,
            space: Some(1),
            space_visible: true,
            space_ordinal: 1,
            screen: 0,
            created: u64::from(id),
            is_main: false,
            windowless: false,
        }
    }

    #[test]
    fn empty_query_keeps_all_input_order_no_spans() {
        let windows = [win(1, "A", "x"), win(2, "B", "y"), win(3, "C", "z")];
        let out = filter("", &windows);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].0, WindowId(1));
        assert_eq!(out[1].0, WindowId(2));
        assert_eq!(out[2].0, WindowId(3));
        for (_, m) in &out {
            let m = m.as_ref().expect("empty query marks all as matches");
            assert!(m.spans.is_empty());
        }
    }

    #[test]
    fn exact_prefix_substring_tiers_rank_in_order() {
        assert_eq!(match_str("code", "code").unwrap().0, Tier::Exact);
        assert_eq!(match_str("co", "code").unwrap().0, Tier::Prefix);
        assert_eq!(match_str("od", "code").unwrap().0, Tier::Substring);
        assert!(Tier::Exact.rank() > Tier::Prefix.rank());
        assert!(Tier::Prefix.rank() > Tier::WordPrefix.rank());
        assert!(Tier::WordPrefix.rank() > Tier::Substring.rank());
        assert!(Tier::Substring.rank() > Tier::Acronym.rank());
        assert!(Tier::Acronym.rank() > Tier::Fuzzy.rank());
    }

    #[test]
    fn camel_case_word_boundary_prefix() {
        // "code" hits the "Code" word at a camelCase hump.
        let (tier, spans) = match_str("code", "VisualStudioCode").unwrap();
        assert_eq!(tier, Tier::WordPrefix);
        let s = "VisualStudioCode";
        let (start, len) = spans[0];
        assert_eq!(&s[start..start + len], "Code");

        // "VSC" hits humps as successive word initials.
        let (tier, spans) = match_str("VSC", "VisualStudioCode").unwrap();
        assert_eq!(tier, Tier::Acronym);
        assert_eq!(spans.len(), 3);
        assert_eq!(&s[spans[0].0..spans[0].0 + spans[0].1], "V");
        assert_eq!(&s[spans[1].0..spans[1].0 + spans[1].1], "S");
        assert_eq!(&s[spans[2].0..spans[2].0 + spans[2].1], "C");
    }

    #[test]
    fn separator_word_boundary() {
        // "mp" → initials of "my-project"
        let (tier, spans) = match_str("mp", "my-project").unwrap();
        assert_eq!(tier, Tier::Acronym);
        assert_eq!(spans.len(), 2);
        let s = "my-project";
        assert_eq!(&s[spans[0].0..spans[0].0 + spans[0].1], "m");
        assert_eq!(&s[spans[1].0..spans[1].0 + spans[1].1], "p");

        // WordPrefix via separator: "pro" prefixes "project"
        let (tier, spans) = match_str("pro", "my-project").unwrap();
        assert_eq!(tier, Tier::WordPrefix);
        let (start, len) = spans[0];
        assert_eq!(&s[start..start + len], "pro");
    }

    #[test]
    fn acronym_google_chrome_with_initial_spans() {
        let (tier, spans) = match_str("gc", "Google Chrome").unwrap();
        assert_eq!(tier, Tier::Acronym);
        assert_eq!(spans.len(), 2);
        let s = "Google Chrome";
        assert_eq!(&s[spans[0].0..spans[0].0 + spans[0].1], "G");
        assert_eq!(&s[spans[1].0..spans[1].0 + spans[1].1], "C");
    }

    #[test]
    fn fuzzy_within_budget_matches_beyond_does_not() {
        // one gap, budget 1 → Fuzzy
        let (tier, spans) = match_str("ac", "abc").unwrap();
        assert_eq!(tier, Tier::Fuzzy);
        assert_eq!(spans.len(), 2);
        assert_eq!(match_str("ab", "aXb").unwrap().0, Tier::Fuzzy);

        // two or more gaps, budget 1 → no match
        assert!(match_str("abc", "aXbYc").is_none());
        assert!(match_str("ace", "abcde").is_none());
        assert!(match_str("ae", "abcde").is_none());
    }

    #[test]
    fn diacritic_and_case_folding_spans_index_original() {
        let (tier, spans) = match_str("cafe", "Café").unwrap();
        assert_eq!(tier, Tier::Exact);
        // Whole original string.
        assert_eq!(spans, vec![(0, "Café".len())]);

        // Prefix with diacritic in target mid-way: "cafe" vs "Café Latte" → Prefix
        let (tier, spans) = match_str("cafe", "Café Latte").unwrap();
        assert_eq!(tier, Tier::Prefix);
        let s = "Café Latte";
        let (start, len) = spans[0];
        assert_eq!(&s[start..start + len], "Café");

        // Case fold
        assert_eq!(match_str("CODE", "code").unwrap().0, Tier::Exact);
    }

    #[test]
    fn app_name_outranks_same_tier_title() {
        let w = win(1, "Chrome", "Firefox notes");
        // "chrome" exact on app, also substring/etc on title? title "Firefox notes" — no.
        let m = match_window("chrome", &w).unwrap();
        assert_eq!(m.field, Field::App);
        assert_eq!(m.tier, Tier::Exact);

        // Same tier on both: exact app vs exact title → App wins.
        let w = win(2, "Notes", "Notes");
        let m = match_window("notes", &w).unwrap();
        assert_eq!(m.field, Field::App);
        assert_eq!(m.tier, Tier::Exact);

        // Title has better tier than app → Title wins.
        let w = win(3, "zzz", "Chrome");
        let m = match_window("chrome", &w).unwrap();
        assert_eq!(m.field, Field::Title);
        assert_eq!(m.tier, Tier::Exact);

        // Same Prefix tier on both → App wins.
        let w = win(4, "Terminal", "Terraform docs");
        let m = match_window("ter", &w).unwrap();
        assert_eq!(m.field, Field::App);
        assert_eq!(m.tier, Tier::Prefix);
    }

    #[test]
    fn matches_sort_above_non_matches_non_matches_keep_order() {
        let windows = [
            win(1, "zzz", "nope"),
            win(2, "Chrome", "x"),
            win(3, "yyy", "nope"),
            win(4, "Chromium", "y"),
        ];
        let out = filter("chrom", &windows);
        let ids: Vec<_> = out.iter().map(|(id, _)| *id).collect();
        // 2 and 4 match (Prefix), 1 and 3 do not — non-matches keep input order.
        assert_eq!(
            ids,
            vec![WindowId(2), WindowId(4), WindowId(1), WindowId(3)]
        );
        assert!(out[0].1.is_some());
        assert!(out[1].1.is_some());
        assert!(out[2].1.is_none());
        assert!(out[3].1.is_none());
    }

    #[test]
    fn spans_align_to_original_string() {
        let (tier, spans) = match_str("cafe", "Café").unwrap();
        assert_eq!(tier, Tier::Exact);
        assert_eq!(spans, vec![(0, 5)]); // C a f é = 1+1+1+2

        let (tier, spans) = match_str("é", "Café").unwrap();
        assert_eq!(tier, Tier::Substring);
        // 'é' starts at byte 3
        assert_eq!(spans, vec![(3, 2)]);
        assert_eq!(&"Café"[3..5], "é");
    }

    #[test]
    fn tie_break_stability_equal_tier_keeps_mru_order() {
        // Two Prefix matches on app_name — input (MRU) order preserved.
        let windows = [
            win(1, "Chrome", "a"),
            win(2, "Chromium", "b"),
            win(3, "Chronicle", "c"),
        ];
        let out = filter("chro", &windows);
        let ids: Vec<_> = out.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![WindowId(1), WindowId(2), WindowId(3)]);
        for (_, m) in &out {
            let m = m.as_ref().unwrap();
            assert_eq!(m.tier, Tier::Prefix);
            assert_eq!(m.field, Field::App);
        }
    }
}
