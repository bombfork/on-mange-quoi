//! "Que cuisiner ?": the recipes that can be cooked with the ingredients in stock, and those
//! that miss only a few. Pantry staples always count as in stock.

use std::collections::HashSet;

use crate::catalog::normalize::fold;
use crate::catalog::{Catalog, Recipe};
use crate::menu::Criteria;

/// A recipe missing at most this many ingredients is still suggested, apart.
pub const MAX_MISSING: usize = 2;

#[derive(Debug)]
pub struct Suggestion<'a> {
    pub recipe: &'a Recipe,
    /// Ingredients to buy, in recipe order.
    pub missing: Vec<&'a str>,
    /// Ingredients of the recipe that come from the stock (staples excluded).
    pub used: usize,
}

#[derive(Debug, Default)]
pub struct Suggestions<'a> {
    /// Everything in stock: the recipes using the most of the stock first.
    pub feasible: Vec<Suggestion<'a>>,
    /// One or two ingredients missing, and at least one taken from the stock: the fewest
    /// missing first.
    pub near: Vec<Suggestion<'a>>,
}

pub fn suggest<'a>(catalog: &'a Catalog, criteria: &Criteria, stock: &[String]) -> Suggestions<'a> {
    let staples: HashSet<&str> = catalog
        .ingredients
        .iter()
        .filter(|ingredient| ingredient.staple)
        .map(|ingredient| ingredient.name.as_str())
        .collect();
    let mut suggestions = Suggestions::default();
    for recipe in catalog.recipes.iter().filter(|r| criteria.matches(r)) {
        let needed: Vec<&str> = recipe
            .ingredients
            .iter()
            .map(String::as_str)
            .filter(|name| !staples.contains(name))
            .collect();
        let missing: Vec<&str> = needed
            .iter()
            .copied()
            .filter(|name| !stock.iter().any(|stocked| stocked == name))
            .collect();
        let used = needed.len() - missing.len();
        let suggestion = Suggestion {
            recipe,
            missing,
            used,
        };
        match suggestion.missing.len() {
            0 => suggestions.feasible.push(suggestion),
            n if n <= MAX_MISSING && used > 0 => suggestions.near.push(suggestion),
            _ => {}
        }
    }
    let name = |s: &Suggestion| fold(&s.recipe.name);
    suggestions
        .feasible
        .sort_by(|a, b| b.used.cmp(&a.used).then_with(|| name(a).cmp(&name(b))));
    suggestions.near.sort_by(|a, b| {
        (a.missing.len().cmp(&b.missing.len()))
            .then(b.used.cmp(&a.used))
            .then_with(|| name(a).cmp(&name(b)))
    });
    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Ingredient;
    use crate::menu::TagFilter;

    fn catalog() -> Catalog {
        let ingredient = |name: &str, staple| Ingredient {
            name: name.into(),
            staple,
        };
        let recipe = |id: &str, ingredients: &[&str], tags: &[&str]| Recipe {
            id: id.into(),
            name: id.to_uppercase(),
            ingredients: ingredients.iter().map(|s| s.to_string()).collect(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        };
        Catalog {
            ingredients: ["œufs", "lait", "farine", "jambon", "fromage", "tomates"]
                .into_iter()
                .map(|name| ingredient(name, false))
                .chain([ingredient("sel", true), ingredient("beurre", true)])
                .collect(),
            tags: vec!["végétarien".into(), "viande".into()],
            choices: vec![vec!["végétarien".into(), "viande".into()]],
            recipes: vec![
                recipe("omelette", &["œufs", "sel", "beurre"], &["végétarien"]),
                recipe(
                    "crepes",
                    &["œufs", "lait", "farine", "sel"],
                    &["végétarien"],
                ),
                recipe("croque", &["jambon", "fromage", "beurre"], &["viande"]),
                recipe(
                    "quiche",
                    &["œufs", "lait", "farine", "jambon", "fromage"],
                    &["viande"],
                ),
                recipe("salade", &["tomates"], &["végétarien"]),
            ],
        }
    }

    fn stock(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn ids(suggestions: &[Suggestion]) -> Vec<String> {
        suggestions.iter().map(|s| s.recipe.id.clone()).collect()
    }

    #[test]
    fn feasible_recipes_need_nothing_but_stock_and_staples() {
        let catalog = catalog();
        let found = suggest(
            &catalog,
            &Criteria::default(),
            &stock(&["œufs", "lait", "farine"]),
        );
        // Most of the stock used first.
        assert_eq!(ids(&found.feasible), ["crepes", "omelette"]);
    }

    #[test]
    fn near_recipes_miss_one_or_two_ingredients_and_use_the_stock() {
        let catalog = catalog();
        let found = suggest(
            &catalog,
            &Criteria::default(),
            &stock(&["œufs", "lait", "jambon"]),
        );
        assert_eq!(ids(&found.feasible), ["omelette"]);
        assert_eq!(ids(&found.near), ["crepes", "croque", "quiche"]);
        assert_eq!(found.near[0].missing, ["farine"]);
        assert_eq!(found.near[2].missing, ["farine", "fromage"]);
        // "salade" misses its only ingredient but uses nothing from the stock.
        assert!(!ids(&found.near).contains(&"salade".to_string()));
    }

    #[test]
    fn three_missing_ingredients_is_too_many() {
        let catalog = catalog();
        let found = suggest(&catalog, &Criteria::default(), &stock(&["œufs"]));
        assert!(!ids(&found.near).contains(&"quiche".to_string()));
    }

    #[test]
    fn tag_filters_apply() {
        let catalog = catalog();
        let criteria = Criteria {
            tags: [("viande".to_string(), TagFilter::Required)].into(),
            ..Criteria::default()
        };
        let found = suggest(
            &catalog,
            &criteria,
            &stock(&["œufs", "lait", "farine", "jambon"]),
        );
        assert!(found.feasible.is_empty());
        assert_eq!(ids(&found.near), ["quiche", "croque"]);
    }

    #[test]
    fn spelling_variants_resolve_to_catalog_ingredients() {
        let catalog = catalog();
        assert_eq!(
            catalog.ingredient("Oeufs").map(|i| i.name.as_str()),
            Some("œufs")
        );
        assert_eq!(
            catalog.ingredient("jambon").map(|i| i.name.as_str()),
            Some("jambon")
        );
        assert!(catalog.ingredient("chocolat").is_none());
    }
}
