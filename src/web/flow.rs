//! State of the main flow as carried by query strings, shared by every page of the flow.
//!
//! Keys: `meals`, `tag:<name>=required|excluded`, repeated `proposed` and `rejected` recipe
//! identifiers, repeated `in_stock` ingredient names, `with_menu=0|1`; plus the action
//! `reject=<id>` and the notice `removed=<count>`.

use crate::catalog::Catalog;
use crate::menu::{Criteria, MAX_MEALS, Menu, TagFilter};

const TAG_PREFIX: &str = "tag:";

/// What a query string carries: the flow state, the requested action and pending notices.
#[derive(Debug, Default)]
pub(super) struct FlowQuery {
    pub menu: Menu,
    pub reject: Option<String>,
    pub removed: usize,
    pub in_stock: Vec<String>,
    /// `None` until the user has made a choice.
    pub with_menu: Option<bool>,
}

/// Lenient on purpose: invalid values fall back to defaults and unknown tags are ignored.
pub(super) fn parse(pairs: &[(String, String)], catalog: &Catalog) -> FlowQuery {
    let mut query = FlowQuery::default();
    for (key, value) in pairs {
        match key.as_str() {
            "meals" => {
                if let Ok(meals) = value.trim().parse::<usize>() {
                    query.menu.criteria.meals = meals.clamp(1, MAX_MEALS);
                }
            }
            "proposed" => query.menu.proposed.push(value.clone()),
            "rejected" => query.menu.rejected.push(value.clone()),
            "reject" => query.reject = Some(value.clone()),
            "removed" => query.removed = value.parse().unwrap_or(0),
            "in_stock" => query.in_stock.push(value.clone()),
            // A hidden `0` precedes the checkbox `1`, so the last value is the user's choice.
            "with_menu" => query.with_menu = Some(value == "1"),
            _ => {
                let Some(tag) = key.strip_prefix(TAG_PREFIX) else {
                    continue;
                };
                if !catalog.tags.iter().any(|known| known == tag) {
                    continue;
                }
                let tags = &mut query.menu.criteria.tags;
                match value.as_str() {
                    "required" => tags.insert(tag.to_string(), TagFilter::Required),
                    "excluded" => tags.insert(tag.to_string(), TagFilter::Excluded),
                    _ => tags.remove(tag),
                };
            }
        }
    }
    query
}

pub(super) type Fields = Vec<(String, String)>;

pub(super) fn criteria_fields(criteria: &Criteria) -> Fields {
    let mut fields = vec![("meals".to_string(), criteria.meals.to_string())];
    for (tag, filter) in &criteria.tags {
        let value = match filter {
            TagFilter::Required => "required",
            TagFilter::Excluded => "excluded",
        };
        fields.push((format!("{TAG_PREFIX}{tag}"), value.to_string()));
    }
    fields
}

pub(super) fn menu_fields(menu: &Menu) -> Fields {
    let mut fields = criteria_fields(&menu.criteria);
    fields.extend(repeated("proposed", &menu.proposed));
    fields.extend(repeated("rejected", &menu.rejected));
    fields
}

/// Menu fields plus the stock check answers.
pub(super) fn stock_fields(menu: &Menu, in_stock: &[String], with_menu: bool) -> Fields {
    let mut fields = menu_fields(menu);
    fields.extend(repeated("in_stock", in_stock));
    fields.push(("with_menu".to_string(), u8::from(with_menu).to_string()));
    fields
}

fn repeated(key: &str, values: &[String]) -> Fields {
    values
        .iter()
        .map(|value| (key.to_string(), value.clone()))
        .collect()
}

pub(super) fn encode(fields: &[(String, String)]) -> String {
    serde_urlencoded::to_string(fields).expect("string pairs always encode")
}

/// A hidden form input.
pub(super) struct Field {
    pub name: String,
    pub value: String,
}

pub(super) fn hidden(fields: Fields) -> Vec<Field> {
    fields
        .into_iter()
        .map(|(name, value)| Field { name, value })
        .collect()
}

pub(super) struct RecipeView {
    pub id: String,
    pub name: String,
    pub ingredients: String,
}

/// Recipes of the catalog with the given identifiers, in that order; unknown ones are skipped.
pub(super) fn recipe_views(catalog: &Catalog, ids: &[String]) -> Vec<RecipeView> {
    ids.iter()
        .filter_map(|id| catalog.recipes.iter().find(|recipe| &recipe.id == id))
        .map(|recipe| RecipeView {
            id: recipe.id.clone(),
            name: recipe.name.clone(),
            ingredients: recipe.ingredients.join(", "),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            tags: vec!["rapide".into(), "enfants".into(), "végétarien".into()],
            ..Catalog::default()
        }
    }

    fn pairs(query: &str) -> Fields {
        serde_urlencoded::from_str(query).unwrap()
    }

    #[test]
    fn parses_leniently_and_ignores_unknown_tags() {
        let query = parse(
            &pairs(
                "meals=99&tag%3Arapide=required&tag%3Aenfants=ignored&tag%3Ainconnu=required\
                 &proposed=croque&rejected=omelette&reject=croque&in_stock=lait\
                 &with_menu=0&with_menu=1",
            ),
            &catalog(),
        );
        assert_eq!(query.menu.criteria.meals, MAX_MEALS);
        assert_eq!(
            query.menu.criteria.tags.into_iter().collect::<Vec<_>>(),
            [("rapide".to_string(), TagFilter::Required)]
        );
        assert_eq!(query.menu.proposed, ["croque"]);
        assert_eq!(query.menu.rejected, ["omelette"]);
        assert_eq!(query.reject.as_deref(), Some("croque"));
        assert_eq!(query.in_stock, ["lait"]);
        assert_eq!(query.with_menu, Some(true));
        assert_eq!(
            parse(&pairs("with_menu=0"), &catalog()).with_menu,
            Some(false)
        );
        assert_eq!(parse(&pairs("meals=abc"), &catalog()).with_menu, None);
    }

    #[test]
    fn encoded_state_parses_back_to_itself() {
        let menu = Menu {
            criteria: Criteria {
                meals: 2,
                tags: [("végétarien".to_string(), TagFilter::Excluded)].into(),
            },
            proposed: vec!["croque".into()],
            rejected: vec!["omelette".into()],
        };
        let in_stock = vec!["crème fraîche".to_string()];
        let query = parse(
            &pairs(&encode(&stock_fields(&menu, &in_stock, false))),
            &catalog(),
        );
        assert_eq!(query.menu, menu);
        assert_eq!(query.in_stock, in_stock);
        assert_eq!(query.with_menu, Some(false));
    }
}
