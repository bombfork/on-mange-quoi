# On mange quoi ?

A meal planner and shopping list helper for families, self-hosted on the local network. It
suggests meals for a given period, checks with you what is already in the cupboard, and produces
the shopping list.

The user interface is in French. The code and documentation are in English.

> **Status:** early development. Menus can be proposed and adjusted; the stock check and the
> shopping list are not implemented yet.

## How it is meant to work

1. Choose how many meals to plan and filter by tags (for example *rapide*, *enfants*,
   *végétarien*): each tag can be required, excluded or ignored.
2. The app proposes that many distinct recipes. Reject any of them and only the rejected ones
   are replaced.
3. Once the menu is accepted, the app lists every ingredient needed (each one once), skipping
   pantry staples that are always in stock. Tick what you already have.
4. The app returns the shopping list, and the menu if you want it.

Recipes carry no cooking instructions, only a name, a list of ingredients and tags. Ingredients
come from a shared catalog rather than free text, so "lait" and "du lait" can never become two
different items.

## Usage

On mange quoi ? is a single binary that serves a catalog directory of YAML files:

```bash
on-mange-quoi --catalog path/to/catalog --port 8080
```

It listens on all interfaces by default (`--host` to restrict it) and has no authentication:
run it on a trusted local network only.

The catalog layout:

```
catalog/
  ingredients.yml   # ingredients and pantry staples
  tags.yml          # tags available to recipes
  recipes/          # one YAML file per recipe
```

See [`examples/catalog`](examples/catalog) for a complete example. The server refuses to start
when the catalog is invalid, and lists every problem.

To check new or modified files against a catalog without serving it:

```bash
on-mange-quoi --catalog path/to/catalog --validate new-recipe.yml
# A new recipe may use ingredients added in the same command:
on-mange-quoi --catalog path/to/catalog --validate new-recipe.yml --validate ingredients.yml
```

Each file is checked as if it were added to the catalog, or replaced the catalog file of the same
kind (ingredients, tags) or name (recipe). Removing an ingredient or a tag still used by a recipe
is an error.

JSON Schemas for the three file types are in [`schemas/`](schemas), for autocompletion in editors
that support them, for example with the YAML language server:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/bombfork/on-mange-quoi/main/schemas/recipe.schema.json
recipe:
  name: Coquillettes au jambon mouliné
  ingredients:
    - coquillettes
    - jambon
    - beurre
```

## Development

Tooling is managed with [mise](https://mise.jdx.dev).

```bash
mise run dev     # Serve examples/catalog on http://localhost:8080
mise run check   # Format check, clippy, build, tests (CI runs the same)
mise run fmt     # Apply formatting
mise run lint    # Apply clippy fixes
mise run schemas # Regenerate schemas/ after changing the catalog file types
```

## License

[GPL-3.0-or-later](LICENSE). Pico CSS (`assets/pico.min.css`) is MIT-licensed, see
[`assets/PICO_LICENSE`](assets/PICO_LICENSE).
