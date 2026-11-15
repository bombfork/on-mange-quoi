# On mange quoi ?

A meal planner and shopping list helper for families, self-hosted on the local network. It
suggests meals for a given period, checks with you what is already in the cupboard, and produces
the shopping list.

The user interface is in French. The code and documentation are in English.

> **Status:** early bootstrap. The binary serves a landing page; no product feature exists yet.

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

See [`examples/catalog`](examples/catalog) for a complete example.

## Development

Tooling is managed with [mise](https://mise.jdx.dev).

```bash
mise run dev     # Serve examples/catalog on http://localhost:8080
mise run check   # Format check, clippy, build, tests (CI runs the same)
mise run fmt     # Apply formatting
mise run lint    # Apply clippy fixes
```

## License

[GPL-3.0-or-later](LICENSE). Pico CSS (`assets/pico.min.css`) is MIT-licensed, see
[`assets/PICO_LICENSE`](assets/PICO_LICENSE).
