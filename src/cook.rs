//! "Que cuisiner ?": the recipes that can be cooked with the ingredients in stock, and those
//! that miss only a few. Pantry staples always count as in stock.
//!
//! Candidates are the recipes matching the tag filters and, once an ingredient tag is chosen
//! ("affiner"), containing at least one ingredient with that tag.

use std::collections::{HashMap, HashSet};

use crate::catalog::normalize::fold;
use crate::catalog::{Catalog, IngredientTag, Recipe};
use crate::menu::Criteria;

/// A recipe missing at most this many ingredients is still suggested, apart.
pub const MAX_MISSING: usize = 2;

/// Number of one-click stock additions offered without a chosen ingredient tag.
pub const QUICK_ADDITIONS: usize = 12;

fn candidates<'a>(
    catalog: &'a Catalog,
    criteria: &'a Criteria,
    refine: Option<&'a IngredientTag>,
) -> impl Iterator<Item = &'a Recipe> {
    catalog.recipes.iter().filter(move |recipe| {
        criteria.matches(recipe)
            && refine.is_none_or(|tag| {
                recipe
                    .ingredients
                    .iter()
                    .any(|ingredient| tag.ingredients.contains(ingredient))
            })
    })
}

fn staples(catalog: &Catalog) -> HashSet<&str> {
    catalog
        .ingredients
        .iter()
        .filter(|ingredient| ingredient.staple)
        .map(|ingredient| ingredient.name.as_str())
        .collect()
}

/// Ingredient tags worth offering to narrow the candidates: those with at least one ingredient
/// used by a recipe matching the tag filters. In catalog order.
pub fn refinements<'a>(catalog: &'a Catalog, criteria: &Criteria) -> Vec<&'a IngredientTag> {
    let used: HashSet<&str> = candidates(catalog, criteria, None)
        .flat_map(|recipe| recipe.ingredients.iter().map(String::as_str))
        .collect();
    catalog
        .ingredient_tags
        .iter()
        .filter(|tag| tag.ingredients.iter().any(|i| used.contains(i.as_str())))
        .collect()
}

/// Ingredients offered as one-click stock additions, never staples nor what is in stock: the
/// ingredients of the chosen ingredient tag used by the candidates, or else the ingredients
/// most used by the candidates.
pub fn quick_additions<'a>(
    catalog: &'a Catalog,
    criteria: &Criteria,
    refine: Option<&'a IngredientTag>,
    stock: &[String],
) -> Vec<&'a str> {
    let staples = staples(catalog);
    let mut uses: HashMap<&str, usize> = HashMap::new();
    for recipe in candidates(catalog, criteria, refine) {
        for ingredient in &recipe.ingredients {
            *uses.entry(ingredient.as_str()).or_default() += 1;
        }
    }
    let wanted = |name: &&str| {
        uses.contains_key(name) && !staples.contains(name) && !stock.iter().any(|s| s == name)
    };
    match refine {
        Some(tag) => tag
            .ingredients
            .iter()
            .map(String::as_str)
            .filter(wanted)
            .collect(),
        None => {
            let mut frequent: Vec<&str> = catalog
                .ingredients
                .iter()
                .map(|ingredient| ingredient.name.as_str())
                .filter(wanted)
                .collect();
            // Stable sort: equal counts keep the catalog order.
            frequent.sort_by(|a, b| uses[b].cmp(&uses[a]));
            frequent.truncate(QUICK_ADDITIONS);
            frequent
        }
    }
}

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

pub fn suggest<'a>(
    catalog: &'a Catalog,
    criteria: &'a Criteria,
    refine: Option<&'a IngredientTag>,
    stock: &[String],
) -> Suggestions<'a> {
    let staples = staples(catalog);
    let mut suggestions = Suggestions::default();
    for recipe in candidates(catalog, criteria, refine) {
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
            ingredient_tags: Vec::new(),
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
        let criteria = Criteria::default();
        let found = suggest(
            &catalog,
            &criteria,
            None,
            &stock(&["œufs", "lait", "farine"]),
        );
        // Most of the stock used first.
        assert_eq!(ids(&found.feasible), ["crepes", "omelette"]);
    }

    #[test]
    fn near_recipes_miss_one_or_two_ingredients_and_use_the_stock() {
        let catalog = catalog();
        let criteria = Criteria::default();
        let found = suggest(
            &catalog,
            &criteria,
            None,
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
        let criteria = Criteria::default();
        let found = suggest(&catalog, &criteria, None, &stock(&["œufs"]));
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
            None,
            &stock(&["œufs", "lait", "farine", "jambon"]),
        );
        assert!(found.feasible.is_empty());
        assert_eq!(ids(&found.near), ["quiche", "croque"]);
    }

    fn with_ingredient_tags(mut catalog: Catalog) -> Catalog {
        catalog.ingredient_tags = vec![
            IngredientTag {
                name: "charcuterie".into(),
                ingredients: vec!["jambon".into()],
            },
            IngredientTag {
                name: "laitages".into(),
                ingredients: vec!["lait".into(), "fromage".into()],
            },
            IngredientTag {
                name: "poisson".into(),
                ingredients: vec!["cabillaud".into()],
            },
        ];
        catalog
    }

    #[test]
    fn refinements_are_the_ingredient_tags_of_the_candidates() {
        let catalog = with_ingredient_tags(catalog());
        let names = |criteria: &Criteria| -> Vec<String> {
            refinements(&catalog, criteria)
                .into_iter()
                .map(|tag| tag.name.clone())
                .collect()
        };
        assert_eq!(names(&Criteria::default()), ["charcuterie", "laitages"]);
        let vegetarian = Criteria {
            tags: [("végétarien".to_string(), TagFilter::Required)].into(),
            ..Criteria::default()
        };
        assert_eq!(names(&vegetarian), ["laitages"]);
    }

    #[test]
    fn a_chosen_ingredient_tag_keeps_the_recipes_using_it() {
        let catalog = with_ingredient_tags(catalog());
        let charcuterie = &catalog.ingredient_tags[0];
        let criteria = Criteria::default();
        let found = suggest(
            &catalog,
            &criteria,
            Some(charcuterie),
            &stock(&["jambon", "fromage", "œufs", "lait", "farine"]),
        );
        assert_eq!(ids(&found.feasible), ["quiche", "croque"]);
    }

    #[test]
    fn quick_additions_offer_the_chosen_tag_or_the_most_used_ingredients() {
        let catalog = with_ingredient_tags(catalog());
        let laitages = &catalog.ingredient_tags[1];
        let criteria = Criteria::default();
        assert_eq!(
            quick_additions(&catalog, &criteria, Some(laitages), &stock(&["lait"])),
            ["fromage"]
        );
        // Without a tag: by number of recipes, then catalog order; no staples, no stock.
        assert_eq!(
            quick_additions(&catalog, &criteria, None, &stock(&["jambon"])),
            ["œufs", "lait", "farine", "fromage", "tomates"]
        );
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
