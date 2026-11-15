//! Name normalization, used to detect duplicates and to suggest the closest known name.
//!
//! Two names are considered the same when they only differ by case, accents, apostrophe or
//! hyphen style, a leading French article, or a plural `s`/`x` ending.

/// Leading articles, longest first so that "de la " wins over shorter prefixes.
const ARTICLES: [&str; 11] = [
    "de la ", "de l'", "les ", "des ", "une ", "le ", "la ", "du ", "un ", "l'", "d'",
];

pub fn normalize(name: &str) -> String {
    let folded = fold(name);
    strip_article(&folded)
        .split(' ')
        .map(singular)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns the candidate closest to `name`, if it is close enough to be a plausible typo or
/// spelling variant.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let target = normalize(name);
    let max_distance = if target.chars().count() <= 4 { 1 } else { 2 };
    candidates
        .into_iter()
        .map(|candidate| (levenshtein(&target, &normalize(candidate)), candidate))
        .filter(|(distance, _)| *distance <= max_distance)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

fn fold(name: &str) -> String {
    let mut folded = String::with_capacity(name.len());
    for c in name.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => folded.push('a'),
            'é' | 'è' | 'ê' | 'ë' => folded.push('e'),
            'î' | 'ï' | 'í' | 'ì' => folded.push('i'),
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => folded.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => folded.push('u'),
            'ÿ' | 'ý' => folded.push('y'),
            'ç' => folded.push('c'),
            'ñ' => folded.push('n'),
            'œ' => folded.push_str("oe"),
            'æ' => folded.push_str("ae"),
            '’' | 'ʼ' | '`' => folded.push('\''),
            // Combining marks, left over when the input is in decomposed form (NFD).
            '\u{300}'..='\u{36F}' => {}
            '-' => folded.push(' '),
            c if c.is_whitespace() => folded.push(' '),
            c => folded.push(c),
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_article(name: &str) -> &str {
    for article in ARTICLES {
        if let Some(rest) = name.strip_prefix(article)
            && !rest.is_empty()
        {
            return rest;
        }
    }
    name
}

fn singular(word: &str) -> &str {
    if word.chars().count() > 3
        && let Some(stem) = word.strip_suffix(['s', 'x'])
    {
        return stem;
    }
    word
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(normalize("Pâtes"), normalize("pates"));
        assert_eq!(normalize("Œufs"), normalize("oeufs"));
        assert_eq!(normalize("Crème fraîche"), "creme fraiche");
    }

    #[test]
    fn folds_decomposed_accents() {
        assert_eq!(normalize("pa\u{302}tes"), normalize("pâtes"));
    }

    #[test]
    fn strips_one_leading_article() {
        assert_eq!(normalize("du lait"), normalize("lait"));
        assert_eq!(normalize("de la farine"), normalize("farine"));
        assert_eq!(normalize("l’huile"), normalize("huile"));
        assert_eq!(normalize("l'huile"), normalize("huile"));
        assert_eq!(normalize("des œufs"), normalize("œufs"));
    }

    #[test]
    fn keeps_words_that_only_start_like_an_article() {
        assert_eq!(normalize("lait"), "lait");
        assert_eq!(normalize("lentilles"), "lentille");
        assert_eq!(normalize("la"), "la");
    }

    #[test]
    fn folds_simple_plurals_word_by_word() {
        assert_eq!(normalize("pommes de terre"), normalize("pomme de terre"));
        assert_eq!(normalize("poireaux"), normalize("poireau"));
        assert_eq!(normalize("riz"), "riz");
    }

    #[test]
    fn folds_hyphens_and_spaces() {
        assert_eq!(normalize("croque-monsieur"), normalize("croque  monsieur"));
    }

    #[test]
    fn keeps_distinct_ingredients_apart() {
        assert_ne!(normalize("pâtes"), normalize("coquillettes"));
    }

    #[test]
    fn suggests_spelling_variants_and_typos() {
        let names = ["œufs", "coquillettes", "lait"];
        assert_eq!(closest("oeufs", names), Some("œufs"));
        assert_eq!(closest("coquilettes", names), Some("coquillettes"));
        assert_eq!(closest("chocolat", names), None);
    }

    #[test]
    fn levenshtein_counts_edits() {
        assert_eq!(levenshtein("chat", "chat"), 0);
        assert_eq!(levenshtein("chat", "chats"), 1);
        assert_eq!(levenshtein("chat", "chut"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
    }
}
