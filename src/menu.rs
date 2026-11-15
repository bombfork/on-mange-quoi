//! Menu proposal: picks distinct recipes matching the tag filters and replaces the rejected
//! ones. A recipe rejected once is never proposed again in the same session, the session being
//! the chain of pages whose state travels in the URL.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::{Catalog, Recipe};

pub const DEFAULT_MEALS: usize = 5;
pub const MAX_MEALS: usize = 30;

/// Filter on one tag. A tag absent from [`Criteria::tags`] is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagFilter {
    Required,
    Excluded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Criteria {
    pub meals: usize,
    pub tags: BTreeMap<String, TagFilter>,
}

impl Default for Criteria {
    fn default() -> Self {
        Self {
            meals: DEFAULT_MEALS,
            tags: BTreeMap::new(),
        }
    }
}

impl Criteria {
    pub fn matches(&self, recipe: &Recipe) -> bool {
        self.tags.iter().all(|(tag, filter)| {
            let tagged = recipe.tags.contains(tag);
            match filter {
                TagFilter::Required => tagged,
                TagFilter::Excluded => !tagged,
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Menu {
    pub criteria: Criteria,
    /// Recipe identifiers, in display order.
    pub proposed: Vec<String>,
    /// Every recipe rejected during the session.
    pub rejected: Vec<String>,
}

impl Menu {
    /// Brings the menu in line with the catalog, then applies the rejection of `reject` and
    /// fills the empty slots with random matching recipes. A replacement takes the place of
    /// the recipe it replaces.
    ///
    /// Returns how many proposed recipes were dropped because they left the catalog.
    pub fn update(
        &mut self,
        catalog: &Catalog,
        reject: Option<&str>,
        rng: &mut fastrand::Rng,
    ) -> usize {
        let known: BTreeSet<&str> = catalog.recipes.iter().map(|r| r.id.as_str()).collect();
        let before = self.proposed.len();
        self.proposed.retain(|id| known.contains(id.as_str()));
        let removed = before - self.proposed.len();

        self.rejected.retain(|id| known.contains(id.as_str()));
        dedup(&mut self.rejected);
        dedup(&mut self.proposed);
        let rejected = &self.rejected;
        self.proposed.retain(|id| !rejected.contains(id));
        self.proposed.truncate(self.criteria.meals);

        if let Some(id) = reject
            && let Some(index) = self.proposed.iter().position(|p| p == id)
        {
            self.rejected.push(id.to_string());
            match self.pick(catalog, rng) {
                Some(replacement) => self.proposed[index] = replacement,
                None => {
                    self.proposed.remove(index);
                }
            }
        }

        while self.proposed.len() < self.criteria.meals {
            match self.pick(catalog, rng) {
                Some(recipe) => self.proposed.push(recipe),
                None => break,
            }
        }
        removed
    }

    /// Number of meals without a recipe, because not enough recipes match the criteria.
    pub fn missing(&self) -> usize {
        self.criteria.meals.saturating_sub(self.proposed.len())
    }

    fn pick(&self, catalog: &Catalog, rng: &mut fastrand::Rng) -> Option<String> {
        let candidates: Vec<&Recipe> = catalog
            .recipes
            .iter()
            .filter(|recipe| {
                self.criteria.matches(recipe)
                    && !self.proposed.contains(&recipe.id)
                    && !self.rejected.contains(&recipe.id)
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        Some(candidates[rng.usize(..candidates.len())].id.clone())
    }
}

fn dedup(ids: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    ids.retain(|id| seen.insert(id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(id: &str, tags: &[&str]) -> Recipe {
        Recipe {
            id: id.into(),
            name: id.to_uppercase(),
            ingredients: vec!["x".into()],
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    /// Ten recipes: `r0` to `r9`; even ones are `rapide`, multiples of three `végétarien`.
    fn catalog() -> Catalog {
        let recipes = (0..10)
            .map(|i| {
                let mut tags = Vec::new();
                if i % 2 == 0 {
                    tags.push("rapide");
                }
                if i % 3 == 0 {
                    tags.push("végétarien");
                }
                recipe(&format!("r{i}"), &tags)
            })
            .collect();
        Catalog {
            ingredients: Vec::new(),
            tags: vec!["rapide".into(), "végétarien".into()],
            recipes,
        }
    }

    fn menu(meals: usize, tags: &[(&str, TagFilter)]) -> Menu {
        Menu {
            criteria: Criteria {
                meals,
                tags: tags.iter().map(|(t, f)| (t.to_string(), *f)).collect(),
            },
            ..Menu::default()
        }
    }

    fn rng() -> fastrand::Rng {
        fastrand::Rng::with_seed(7)
    }

    fn distinct(ids: &[String]) -> bool {
        ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
    }

    #[test]
    fn filters_on_required_excluded_and_ignored_tags() {
        let quick = recipe("a", &["rapide"]);
        let quick_veggie = recipe("b", &["rapide", "végétarien"]);
        let criteria = menu(1, &[("rapide", TagFilter::Required)]).criteria;
        assert!(criteria.matches(&quick) && criteria.matches(&quick_veggie));
        let criteria = menu(
            1,
            &[
                ("rapide", TagFilter::Required),
                ("végétarien", TagFilter::Excluded),
            ],
        )
        .criteria;
        assert!(criteria.matches(&quick) && !criteria.matches(&quick_veggie));
        assert!(menu(1, &[]).criteria.matches(&recipe("c", &[])));
    }

    #[test]
    fn proposes_distinct_matching_recipes() {
        let catalog = catalog();
        let mut menu = menu(3, &[("rapide", TagFilter::Required)]);
        assert_eq!(menu.update(&catalog, None, &mut rng()), 0);
        assert_eq!(menu.proposed.len(), 3);
        assert!(distinct(&menu.proposed));
        let quick = ["r0", "r2", "r4", "r6", "r8"];
        assert!(menu.proposed.iter().all(|id| quick.contains(&id.as_str())));
        assert_eq!(menu.missing(), 0);
    }

    #[test]
    fn proposes_what_matches_when_there_are_not_enough_recipes() {
        let catalog = catalog();
        // Quick and vegetarian: r0 and r6 only.
        let mut menu = menu(
            4,
            &[
                ("rapide", TagFilter::Required),
                ("végétarien", TagFilter::Required),
            ],
        );
        menu.update(&catalog, None, &mut rng());
        let mut proposed = menu.proposed.clone();
        proposed.sort();
        assert_eq!(proposed, ["r0", "r6"]);
        assert_eq!(menu.missing(), 2);
    }

    #[test]
    fn replaces_only_the_rejected_recipe_in_place() {
        let catalog = catalog();
        let mut rng = rng();
        let mut menu = menu(4, &[]);
        menu.update(&catalog, None, &mut rng);
        let before = menu.proposed.clone();
        let rejected = before[1].clone();

        menu.update(&catalog, Some(&rejected), &mut rng);

        assert_eq!(menu.proposed.len(), 4);
        assert_eq!(menu.proposed[0], before[0]);
        assert_eq!(menu.proposed[2..], before[2..]);
        assert_ne!(menu.proposed[1], rejected);
        assert!(distinct(&menu.proposed));
        assert_eq!(menu.rejected, [rejected]);
    }

    #[test]
    fn a_rejected_recipe_is_never_proposed_again() {
        let catalog = catalog();
        let mut rng = rng();
        let mut menu = menu(3, &[]);
        menu.update(&catalog, None, &mut rng);
        let mut ever_rejected = BTreeSet::new();
        // Keep rejecting the first recipe until the catalog runs out.
        while let Some(first) = menu.proposed.first().cloned() {
            ever_rejected.insert(first.clone());
            menu.update(&catalog, Some(&first), &mut rng);
            assert!(menu.proposed.iter().all(|id| !ever_rejected.contains(id)));
        }
        assert_eq!(ever_rejected.len(), 10);
        assert_eq!(menu.missing(), 3);
    }

    #[test]
    fn rejecting_without_replacement_shortens_the_menu() {
        let catalog = catalog();
        let mut menu = menu(2, &[("végétarien", TagFilter::Required)]);
        menu.proposed = vec!["r0".into(), "r3".into()];
        menu.rejected = vec!["r6".into(), "r9".into()];
        menu.update(&catalog, Some("r3"), &mut rng());
        assert_eq!(menu.proposed, ["r0"]);
        assert_eq!(menu.missing(), 1);
    }

    #[test]
    fn recipes_that_left_the_catalog_are_dropped_and_replaced() {
        let catalog = catalog();
        let mut menu = menu(2, &[]);
        menu.proposed = vec!["r1".into(), "gone".into()];
        menu.rejected = vec!["gone-too".into()];
        assert_eq!(menu.update(&catalog, None, &mut rng()), 1);
        assert_eq!(menu.proposed.len(), 2);
        assert_eq!(menu.proposed[0], "r1");
        assert!(menu.rejected.is_empty());
    }

    #[test]
    fn an_up_to_date_menu_is_left_unchanged() {
        let catalog = catalog();
        let mut rng = rng();
        let mut menu = menu(3, &[]);
        menu.update(&catalog, None, &mut rng);
        let first = menu.proposed[0].clone();
        menu.update(&catalog, Some(&first), &mut rng);
        let settled = menu.clone();
        assert_eq!(menu.update(&catalog, None, &mut rng), 0);
        assert_eq!(menu, settled);
    }

    #[test]
    fn unknown_rejection_changes_nothing() {
        let catalog = catalog();
        let mut rng = rng();
        let mut menu = menu(3, &[]);
        menu.update(&catalog, None, &mut rng);
        let settled = menu.clone();
        menu.update(&catalog, Some("r-not-proposed"), &mut rng);
        assert_eq!(menu, settled);
    }
}
