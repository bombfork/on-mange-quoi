//! Stock check and shopping list (steps 4 and 5 of the main flow).
//!
//! Ingredients are listed in catalog order, so the order of `ingredients.yml` (for example
//! grouped by shop aisle) is the order of the list.

use std::collections::BTreeSet;

use crate::catalog::Catalog;

/// Every ingredient used by the given recipes, once, without pantry staples. Unknown recipe
/// identifiers are skipped.
pub fn ingredients_to_check<'a>(catalog: &'a Catalog, recipe_ids: &[String]) -> Vec<&'a str> {
    let used: BTreeSet<&str> = catalog
        .recipes
        .iter()
        .filter(|recipe| recipe_ids.contains(&recipe.id))
        .flat_map(|recipe| recipe.ingredients.iter().map(String::as_str))
        .collect();
    catalog
        .ingredients
        .iter()
        .filter(|ingredient| !ingredient.staple && used.contains(ingredient.name.as_str()))
        .map(|ingredient| ingredient.name.as_str())
        .collect()
}

/// The ingredients to check that are not in stock.
pub fn shopping_list<'a>(
    catalog: &'a Catalog,
    recipe_ids: &[String],
    in_stock: &[String],
) -> Vec<&'a str> {
    ingredients_to_check(catalog, recipe_ids)
        .into_iter()
        .filter(|name| !in_stock.iter().any(|stocked| stocked == name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Ingredient, Recipe};

    fn catalog() -> Catalog {
        let ingredient = |name: &str, staple| Ingredient {
            name: name.into(),
            staple,
        };
        let recipe = |id: &str, ingredients: &[&str]| Recipe {
            id: id.into(),
            name: id.into(),
            ingredients: ingredients.iter().map(|i| i.to_string()).collect(),
            tags: Vec::new(),
        };
        Catalog {
            ingredients: vec![
                ingredient("lait", false),
                ingredient("œufs", false),
                ingredient("jambon", false),
                ingredient("pain de mie", false),
                ingredient("sel", true),
            ],
            tags: Vec::new(),
            recipes: vec![
                recipe("croque", &["pain de mie", "jambon", "sel"]),
                recipe("omelette", &["œufs", "jambon", "sel"]),
                recipe("flan", &["lait", "œufs"]),
            ],
        }
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn lists_each_ingredient_once_in_catalog_order_without_staples() {
        assert_eq!(
            ingredients_to_check(&catalog(), &ids(&["omelette", "croque"])),
            ["œufs", "jambon", "pain de mie"]
        );
    }

    #[test]
    fn skips_unknown_recipes() {
        assert_eq!(
            ingredients_to_check(&catalog(), &ids(&["flan", "disparue"])),
            ["lait", "œufs"]
        );
        assert!(ingredients_to_check(&catalog(), &[]).is_empty());
    }

    #[test]
    fn shopping_list_removes_what_is_in_stock() {
        let catalog = catalog();
        let list = shopping_list(
            &catalog,
            &ids(&["omelette", "croque"]),
            &ids(&["jambon", "inconnu"]),
        );
        assert_eq!(list, ["œufs", "pain de mie"]);
    }
}
