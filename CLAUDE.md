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

Current state: the binary loads and validates the catalog (at startup and with `--validate`) and
serves the whole main flow (criteria, proposal, rejection, stock check, shopping list) and the
catalog editor. The catalog is reloaded when its files change.

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
  At startup, an invalid catalog stops the server with the errors. Changes are detected by
  polling a fingerprint of the files every second (no file system notifications, so it also
  works on network file systems and ConfigMap volumes); a change is applied once stable for one
  interval, so a burst of writes triggers a single reload.
- The in-app editor writes the YAML files back atomically and refuses to overwrite a file that
  changed since it was read. Written files are normalized: comments and hand formatting are lost.
  If the data directory is read-only, the editor is disabled. Every edit is validated on a
  staging copy of the catalog with the same rules as `--validate` before anything is written.
  Renaming an ingredient or a tag used by recipes rewrites those recipes, after a confirmation
  page listing them. Renaming a recipe keeps its file name, hence its identifier.
- Recipe form: ingredients are picked with a search field whose suggestions come from a native
  `<datalist>` (no JavaScript). Adding, removing or replacing an ingredient submits the form and
  shows it again, the search field focused. A typed name that only differs by spelling from a
  catalog name (normalization) is added as that name; any other unknown name is marked new,
  with a button to replace it by the closest catalog name when there is one, and new
  ingredients are added to the catalog when the recipe is saved, in the same validated edit.
- Edits are POST forms. POST requests from other sites are refused (`Sec-Fetch-Site`, or
  `Origin` against `Host` for browsers without it), since there is no authentication.
- JSON Schemas for the YAML files are generated from the Rust types, for editor autocompletion.
- Command line:
  - `on-mange-quoi --catalog <dir> --port <port>` serves the catalog.
  - `on-mange-quoi --catalog <dir> --validate` validates the catalog without serving it.
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

Always use mise tasks, not `cargo` directly.

- `mise run dev` - serve a copy of `examples/catalog` on http://localhost:8080; the copy
  (`target/dev-catalog`, created on first run) is what the editor modifies
- `mise run fmt` / `mise run fmt:check` - rustfmt fix / check
- `mise run lint` / `mise run lint:check` - clippy fix / check (`-D warnings`, all targets)
- `mise run build` - debug build
- `mise run test` - unit tests and CLI integration tests
- `mise run check` - all checks; run before every commit (CI runs the same)
- `mise run deps:update` - upgrade dependencies (cargo-edit)
- `mise run schemas` - regenerate `schemas/*.json` after changing the catalog file types (a test
  fails while they are out of date)

## Architecture

- `src/lib.rs` - library crate holding every module; `src/main.rs` is the command line (clap):
  either `--validate` or serve, with graceful shutdown on Ctrl-C/SIGTERM.
- `src/catalog/` - domain module, no HTTP/template/i18n dependency:
  - `mod.rs` - `Catalog`, `Problem`/`ProblemKind`, reading the layout, `load()` and `load_with()`
    (extra files from `--validate` added on top or replacing catalog files), `fingerprint()`
    (hash of the files `load()` reads);
  - `files.rs` - serde types of the three file types, parsed with serde-saphyr (`serde_yaml` is
    archived). `Spanned` gives the line and column of every name for error messages. Also file
    kind detection and the schema generation test;
  - `normalize.rs` - name normalization and closest-name suggestion;
  - `validate.rs` - cross-file rules (uniqueness, references, removed names still in use);
  - `tests.rs` - one test per rule, on temporary catalog directories.
- `src/live.rs` - `LiveCatalog`: the served `Snapshot` (catalog plus the problems of the files
  on disk), swapped as a whole under a lock; `spawn_watcher` polls `catalog::fingerprint` in a
  thread and reloads after each settled change.
- `src/editor.rs` - `Editor`: catalog edits (recipes, ingredients, tags) validated on a staging
  copy then written atomically, version tokens, YAML writing (plain scalars when unambiguous,
  JSON-quoted otherwise), recipe identifiers derived from names (`slug`).
- `src/report.rs` - renders problems in French as `path:line:column : message`. Structural errors
  (unknown key, wrong type) embed the parser's English message.
- `src/menu.rs` - domain module for steps 1 to 3 of the main flow: `Criteria` (meal count, tag
  filters), `Menu` (proposed and rejected recipe identifiers) and `Menu::update`, which drops
  recipes that left the catalog, replaces a rejected recipe in place and fills empty slots. The
  random source (`fastrand::Rng`) is a parameter, seeded in tests.
- `src/shopping.rs` - domain module for steps 4 and 5: ingredients to check (union of the menu's
  ingredients, without staples, in catalog order) and the shopping list.
- `src/web/mod.rs` - axum router, embedded stylesheets, template rendering. Each request takes a
  snapshot of the `LiveCatalog` from the router state; every page struct has `catalog_errors`,
  shown as a banner by `base.html`.
- `src/web/flow.rs` - the flow state as carried by query strings, shared by every page:
  `meals`, `tag:<name>=required|excluded`, repeated `proposed`, `rejected` and `in_stock`,
  `with_menu=0|1` (a hidden `0` precedes the checkbox, the last value wins).
- `src/web/menu.rs` - pages `/` (criteria form) and `/menu` (proposal). Actions (`reject=<id>`,
  first proposal, catalog changes) answer with a 303 redirect to the URL of the new state, so a
  reload never changes the menu.
- `src/web/shopping.rs` - pages `/menu/accepted` (accepted menu and stock check),
  `/shopping-list` and `/shopping-list.txt` (plain text, to copy or print).
- `src/web/editor.rs` - editor pages under `/catalog/` (recipes, ingredients, tags), shown from
  the catalog on disk; a successful edit reloads the served catalog and redirects with a
  `notice`. Templates in `templates/editor/`.
- `src/i18n.rs` - `locales/fr.json` is embedded and deserialized into the `Messages` struct
  (`deny_unknown_fields`), so templates reference keys as fields checked at compile time.
  `interpolate()` fills `{name}` placeholders. Tests enforce French typography in the locale.
- `templates/` - askama templates; `base.html` is the layout, each page extends it.
- `locales/fr.json` - the only locale. Every new key must be added to the `Messages` structs.
- `assets/pico.min.css` - vendored Pico CSS v2, embedded in the binary; `assets/main.css` holds
  the app's own styles.
- `examples/catalog/` - example catalog, used by tests; `mise run dev` serves a copy of it.
- `schemas/` - JSON Schemas of the catalog files, generated from the serde types.
- `tests/cli.rs` - command-line integration tests (assert_cmd).

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
- The stock check and the shopping list follow the order of `ingredients.yml`, so grouping
  ingredients by shop aisle in that file groups the list too.
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
- Ingredients can only be reordered (which orders the shopping list) by editing
  `ingredients.yml` by hand.
