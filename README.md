# On mange quoi ?

A meal planner and shopping list helper for families, self-hosted on the local network. It
suggests meals for a given period, checks with you what is already in the cupboard, and produces
the shopping list.

The user interface is in French. The code and documentation are in English.

> **Status:** early development. The whole flow works, from criteria to shopping list, and the
> catalog can be edited from the app.

## How it is meant to work

1. Choose how many meals to plan and filter by tags (for example *rapide*, *enfants*,
   *végétarien*): each tag can be required, excluded or ignored.
2. The app proposes that many distinct recipes. Reject any of them and only the rejected ones
   are replaced.
3. Once the menu is accepted, the app lists every ingredient needed (each one once), skipping
   pantry staples that are always in stock. Tick what you already have.
4. The app returns the shopping list, and the menu if you want it, also as plain text to copy
   or print. The list follows the order of the ingredients file, so you can group ingredients
   by shop aisle there.

Recipes carry no cooking instructions, only a name, a list of ingredients and tags. Ingredients
come from a shared catalog rather than free text, so "lait" and "du lait" can never become two
different items.

## Installation

Download the binary for your machine from the
[latest release](https://github.com/bombfork/on-mange-quoi/releases/latest):
`on-mange-quoi-x86_64-unknown-linux-musl` for a PC or most servers,
`on-mange-quoi-aarch64-unknown-linux-musl` for a Raspberry Pi 3, 4 or 5 running a 64-bit system.
The binaries are statically linked and run on any Linux distribution.

```bash
base=https://github.com/bombfork/on-mange-quoi/releases/latest/download
curl -LO "$base/on-mange-quoi-x86_64-unknown-linux-musl" -LO "$base/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
install -m 755 on-mange-quoi-x86_64-unknown-linux-musl ~/.local/bin/on-mange-quoi
```

To start with a catalog, copy [`examples/catalog`](examples/catalog) and adapt it.

## Usage

On mange quoi ? is a single binary that serves a catalog directory of YAML files:

```bash
on-mange-quoi --catalog path/to/catalog --port 8080
```

It listens on all interfaces by default (`--host` to restrict it) and has no authentication:
run it on a trusted local network only. Edit requests coming from other sites are refused.
Behind a reverse proxy, browsers that do not send `Sec-Fetch-Site` need the proxy to forward
the original `Host` header for edits to work.

The catalog layout:

```
catalog/
  ingredients.yml   # ingredients and pantry staples
  tags.yml          # tags available to recipes
  recipes/          # one YAML file per recipe
```

See [`examples/catalog`](examples/catalog) for a complete example. The server refuses to start
when the catalog is invalid, and lists every problem.

Changes to the catalog files are picked up within a couple of seconds, without a restart. If the
modified files are invalid, the previous catalog stays in use and every page shows the errors
until they are fixed.

The catalog can also be edited from the app (the "Catalogue" link): recipes, ingredients and
tags. In the recipe form, ingredients are found with a search field, and a new ingredient can be
typed there: it is added to the catalog when the recipe is saved. Several recipes and
ingredients can also be imported at once by pasting raw YAML documents (the "Importer" page). Edits are checked with the same rules as below before being written; files written by the
app lose their comments and hand formatting. When the catalog directory is read-only, the editor
is disabled.

To check a catalog, or new or modified files against it, without serving it:

```bash
on-mange-quoi --catalog path/to/catalog --validate
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
mise run dev     # Serve a copy of examples/catalog on http://localhost:8080
mise run check   # Format check, clippy, build, tests (CI runs the same)
mise run fmt     # Apply formatting
mise run lint    # Apply clippy fixes
mise run schemas # Regenerate schemas/ after changing the catalog file types
```

## License

[GPL-3.0-or-later](LICENSE). Pico CSS (`assets/pico.min.css`) is MIT-licensed, see
[`assets/PICO_LICENSE`](assets/PICO_LICENSE).
