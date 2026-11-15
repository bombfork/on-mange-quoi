# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Language conventions

- **Talk to the user in French.** All conversation about this project happens in French.
- **User-facing text is French**, and lives in `locales/fr.json`, never hardcoded in code or
  templates.
- **UI register: infinitive**, neither tutoiement nor vouvoiement (« Choisir le nombre de
  repas », not « Choisis » or « Choisissez »).
- **Code, comments, docs, issues, PRs and commit messages are in English.**
- French typography in locale strings: a narrow no-break space (U+202F) before `?`, `!` and `;`,
  a no-break space (U+00A0) before `:`, and the typographic apostrophe `’`.

## Project overview

On mange quoi ? is a meal planner for families: it proposes meals for a period, checks pantry
stock with the user, and outputs a shopping list. It is self-hosted on the family's local
network.

Current state: the repository still holds the initial vanilla JavaScript skeleton (i18n, landing
page, tooling, CI, GitHub Pages deployment), which the Rust stack below replaces. No product
feature is implemented yet.

## Technical decisions

- A single Rust binary plus a data directory of YAML files. Keeping the architecture simple
  outweighs everything else, including a public demo: there is no GitHub Pages showcase.
- Distributed as a single binary only: no container image and no mobile app for now.
- No authentication for now: the app trusts the local network.
- Pages are rendered on the server from templates, with plain HTML forms and no JavaScript.
  Pico CSS is embedded in the binary.
- The state of the main flow (proposed, accepted and rejected recipes, ticked ingredients)
  travels in forms and URLs: no server-side session, no database.
- The YAML files are the source of truth. The server keeps the last valid catalog in memory and
  reloads it when the files change, without a restart unless the data format itself changes.
  If a change makes the catalog invalid, the previous catalog stays in use and the error is shown.
  At startup, an invalid catalog stops the server with the errors.
- The in-app editor writes the YAML files back atomically and refuses to overwrite a file that
  changed since it was read. Written files are normalized: comments and hand formatting are lost.
  If the data directory is read-only, the editor is disabled.
- JSON Schemas for the YAML files are generated from the Rust types, for editor autocompletion.
- Command line:
  - `on-mange-quoi --catalog <dir> --port <port>` serves the catalog.
  - `on-mange-quoi --catalog <dir> --validate <file> [--validate <file>...]` validates
    ingredients or recipe files without serving, with the same code the server uses. The files
    are checked together against the catalog as if they were added to it (or replaced the
    catalog file of the same name), so references between files are checked too: a new recipe
    may only use ingredients already in the catalog or in an ingredients file given in the same
    command. Non-zero exit code on error.
- An ingredient used by a recipe cannot be removed.
- Domain logic (catalog validation, menu proposal, shopping list) lives in modules independent
  of HTTP and templates, and is unit-tested.

## Development commands

Always use mise tasks, not `npm`/`npx` directly.

- `mise run install` - install npm dependencies (`npm ci`)
- `mise run dev` - serve on http://localhost:8000 with caching disabled
- `mise run lint` - oxlint (`--deny-warnings`) on `js/`, `scripts/`, `tests/`
- `mise run format` / `mise run format-fix` - oxfmt check / fix
- `mise run validate-i18n` - validate `locales/*.json` against `locales/schema.json`
- `mise run test-unit` - `node --test` on `tests/**/*.test.js`
- `mise run validate-all` - all of the above; run before every commit (CI runs the same)

## Architecture

- `index.html` - single page; elements with `data-i18n="key.path"` get their text from the locale.
- `js/main.js` - entry point.
- `js/i18n.js` - browser i18n: `initI18n()`, `setLanguage()`, `t(key, params)`,
  `applyTranslations(root)`. Language priority: `?lang=` URL param, then localStorage, then browser.
- `js/i18n-core.js` - pure helpers (`lookup`, `interpolate`, `pickLanguage`) with no DOM access,
  unit-tested under Node.
- `locales/fr.json` - the only locale for now; `fr` is the default language.
- `locales/schema.json` - strict schema (`additionalProperties: false`): every new key must be
  added to the schema and to every locale.
- `assets/pico.min.css` - vendored Pico CSS v2 (no CDN, so the app works offline and in Capacitor).

Keep domain logic in pure modules without DOM access, so it can be unit-tested with `node --test`.

## Product design

### Main flow

1. The user sets the number of meals and a filter per tag: required, ignored or excluded
   (for example quick = required, kid-friendly = required, vegetarian = ignored).
2. The app proposes that many distinct recipes matching the filters.
3. The user accepts or rejects each one. Only rejected recipes are regenerated, and a recipe
   rejected during a session is not proposed again in that session.
4. Once the menu is accepted, the stock check lists the union of ingredients, each one once,
   minus pantry staples. The user ticks what they already have.
5. Output: the shopping list (unticked ingredients), plus the menu if wanted.

### Data model decisions

- A recipe is a name, a list of ingredients and tags. No cooking instructions.
- No quantities, anywhere.
- Ingredients come from a controlled catalog, never free text. Names are stored without article
  ("lait", not "du lait"). Duplicates are detected by normalizing case, accents, leading articles
  and simple plurals. The same normalization suggests the closest name when a reference is
  unknown; references themselves must match a catalog name exactly.
- Ingredients have no hierarchy: `pâtes` and `coquillettes` are both valid, and a recipe may name
  either.
- Pantry staples (salt, oil, etc.) are always in stock. They are never asked about in the stock
  check.
- No persistent inventory: stock is checked each time a menu is accepted.
- Tags form an extensible catalog. Defaults: `végétarien`, `enfants`, `rapide`. A tag used by a
  recipe cannot be removed.
- The app includes a module to create, edit and delete recipes, ingredients and tags.

### Catalog format

The catalog is a directory with a fixed layout. YAML keys are in English, values in French.

```
catalog/
  ingredients.yml
  tags.yml
  recipes/
    coquillettes-au-jambon-mouline.yml
```

```yaml
# ingredients.yml
ingredients:
  - œufs
  - pâtes
  - coquillettes
  - jambon
  - beurre
staples:
  - sel
  - huile
```

```yaml
# tags.yml
tags:
  - végétarien
  - enfants
  - rapide
```

```yaml
# recipes/coquillettes-au-jambon-mouline.yml
recipe:
  name: Coquillettes au jambon mouliné
  ingredients:
    - coquillettes
    - jambon
    - beurre
  tags:
    - enfants
    - rapide
```

Rules:

- The top-level key (`ingredients`, `tags` or `recipe`) gives the file type. Unknown keys are
  rejected.
- Names are stored with their proper spelling (`pâtes`, `œufs`): they are what the shopping list
  shows.
- `staples` lists pantry staples. A name appears in `ingredients` or in `staples`, not both.
  Recipes may name staples.
- Ingredient names (both lists together), tag names and recipe names are each unique after
  normalization.
- One recipe per file. The file name without `.yml` is the recipe identifier: lowercase ASCII
  letters, digits and hyphens, so it is safe in URLs and does not depend on how the file system
  normalizes Unicode. The editor derives it from the recipe name. Only `.yml` is accepted.
- A recipe has a non-empty name and at least one ingredient, without duplicates. `tags` may be
  absent or empty.

### Open questions

- Using the shopping list away from the local network (in the shop) is not solved yet.
