# On mange quoi ?

A meal planner and shopping list helper for families. It suggests meals for a given period,
checks with you what is already in the cupboard, and produces the shopping list.

The user interface is in French first, with an i18n layer ready for other languages. The code
and documentation are in English.

> **Status:** early bootstrap. Only the project skeleton exists so far.

## How it is meant to work

1. Choose how many meals to plan and filter by tags (for example *quick*, *kid-friendly*,
   *vegetarian*): each tag can be required, excluded or ignored.
2. The app proposes that many distinct recipes. Reject any of them and only the rejected ones
   are replaced.
3. Once the menu is accepted, the app lists every ingredient needed (each one once), skipping
   pantry staples that are always in stock. Tick what you already have.
4. The app returns the shopping list, and the menu if you want it.

Recipes carry no cooking instructions, only a name, a list of ingredients and tags. Ingredients
come from a shared catalog rather than free text, so "lait" and "du lait" can never become two
different items.

## Development

Tooling is managed with [mise](https://mise.jdx.dev). There is no build step: the repository
root is the site, served as static files.

```bash
mise run install       # Install npm dependencies
mise run dev           # Serve on http://localhost:8000
mise run validate-all  # Lint, format check, locale validation, unit tests
mise run format-fix    # Apply formatting
```

Every push to `main` deploys to GitHub Pages.

## License

[GPL-3.0-or-later](LICENSE). Pico CSS (`assets/pico.min.css`) is MIT-licensed, see
[`assets/PICO_LICENSE`](assets/PICO_LICENSE).
