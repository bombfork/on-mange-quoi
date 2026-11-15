//! User-facing text. `locales/fr.json` is embedded in the binary and deserialized into
//! [`Messages`], so templates and code reference keys as fields checked at compile time.

use std::fmt::{Display, Write};
use std::sync::LazyLock;

use serde::Deserialize;

const SOURCE: &str = include_str!("../locales/fr.json");

/// Every key of the locale file. Unknown or missing keys fail deserialization.
pub static MESSAGES: LazyLock<Messages> = LazyLock::new(|| {
    serde_json::from_str(SOURCE).expect("locales/fr.json must match the Messages struct")
});

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Messages {
    pub app: AppMessages,
    pub landing: LandingMessages,
    pub cli: CliMessages,
    pub catalog: CatalogMessages,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppMessages {
    pub title: String,
    pub tagline: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandingMessages {
    pub intro: String,
    pub status: String,
    pub catalog_summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliMessages {
    pub about: String,
    pub catalog: String,
    pub host: String,
    pub port: String,
    pub catalog_not_a_directory: String,
    pub bind_failed: String,
    pub listening: String,
    pub validate: String,
    pub catalog_loaded: String,
    pub catalog_valid: String,
    pub catalog_invalid: String,
}

/// Catalog problems, rendered by `report::describe`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogMessages {
    pub problem: String,
    pub unreadable: String,
    pub missing: String,
    pub syntax: String,
    pub unknown_file_type: String,
    pub several_files_of_kind: String,
    pub unexpected_file: String,
    pub invalid_recipe_id: String,
    pub empty_name: String,
    pub no_ingredients: String,
    pub duplicate_ingredient: String,
    pub duplicate_tag: String,
    pub duplicate_recipe_name: String,
    pub repeated_in_recipe: String,
    pub unknown_ingredient: String,
    pub unknown_tag: String,
    pub did_you_mean: String,
    pub ingredient_in_use: String,
    pub tag_in_use: String,
}

/// Replaces every `{name}` placeholder with the matching parameter. Unknown placeholders are
/// kept as is, so a missing parameter stays visible.
pub fn interpolate(template: &str, params: &[(&str, &dyn Display)]) -> String {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after_brace = &rest[start + 1..];
        let value = after_brace.find('}').and_then(|end| {
            let name = &after_brace[..end];
            params
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (value, end))
        });
        match value {
            Some((value, end)) => {
                let _ = write!(output, "{value}");
                rest = &after_brace[end + 1..];
            }
            None => {
                output.push('{');
                rest = after_brace;
            }
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(value: &serde_json::Value, path: String, found: &mut Vec<(String, String)>) {
        match value {
            serde_json::Value::String(text) => found.push((path, text.clone())),
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    strings(child, format!("{path}.{key}"), found);
                }
            }
            _ => panic!("{path}: locale values must be strings or objects"),
        }
    }

    fn all_strings() -> Vec<(String, String)> {
        let value: serde_json::Value = serde_json::from_str(SOURCE).unwrap();
        let mut found = Vec::new();
        strings(&value, String::new(), &mut found);
        found
    }

    #[test]
    fn locale_matches_messages_struct() {
        LazyLock::force(&MESSAGES);
    }

    #[test]
    fn locale_strings_are_not_empty() {
        for (path, text) in all_strings() {
            assert!(!text.trim().is_empty(), "{path} is empty");
        }
    }

    #[test]
    fn locale_follows_french_typography() {
        for (path, text) in all_strings() {
            assert!(
                !text.contains('\''),
                "{path}: use the typographic apostrophe ’"
            );
            let chars: Vec<char> = text.chars().collect();
            for pair in chars.windows(2) {
                let (space, mark) = (pair[0], pair[1]);
                if !space.is_whitespace() {
                    continue;
                }
                match mark {
                    '?' | '!' | ';' => assert_eq!(
                        space, '\u{202F}',
                        "{path}: narrow no-break space (U+202F) expected before {mark}"
                    ),
                    ':' => assert_eq!(
                        space, '\u{A0}',
                        "{path}: no-break space (U+00A0) expected before :"
                    ),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn interpolate_replaces_known_placeholders() {
        assert_eq!(
            interpolate("{a} et {b}", &[("a", &"pain"), ("b", &2)]),
            "pain et 2"
        );
    }

    #[test]
    fn interpolate_keeps_unknown_placeholders() {
        assert_eq!(interpolate("{a} {inconnu}", &[("a", &1)]), "1 {inconnu}");
    }

    #[test]
    fn interpolate_handles_unbalanced_braces() {
        assert_eq!(interpolate("{a", &[("a", &1)]), "{a");
        assert_eq!(interpolate("a}", &[]), "a}");
    }
}
