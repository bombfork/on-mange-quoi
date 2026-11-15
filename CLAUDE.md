# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Language conventions

- **Talk to the user in French.** All conversation about this project happens in French.
- **User-facing text is French**, and lives in `locales/fr.json`, never hardcoded in JS.
- **Code, comments, docs, issues, PRs and commit messages are in English.**
- French typography in locale strings: a narrow no-break space (U+202F) before `?`, `!` and `;`,
  a no-break space (U+00A0) before `:`, and the typographic apostrophe `’`.

## Project overview

On mange quoi ? is a client-side meal planner for families: it proposes meals for a period,
checks pantry stock with the user, and outputs a shopping list. Same technical approach as
`../open-secret-santa`: vanilla JavaScript ES modules, no build step, Pico CSS, deployed to
GitHub Pages. A mobile app (Capacitor) is planned later.

Current state: skeleton only (i18n, landing page, tooling, CI, deployment). No product feature
is implemented yet.

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
  ("lait", not "du lait"). Creating an ingredient checks for duplicates by normalizing case,
  accents, leading articles and simple plurals.
- Pantry staples (salt, oil, etc.) are ingredients flagged as always in stock. They are never
  asked about in the stock check.
- No persistent inventory: stock is checked each time a menu is accepted.
- Tags form an extensible catalog. Defaults: vegetarian, kid-friendly, quick.
- The app includes a module to create, edit and delete recipes, ingredients and tags.

### Open questions

- Where data lives (URL, on-device storage, sync): not decided yet.
- Recipe file format: to be specified by the user.
