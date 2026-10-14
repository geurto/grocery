# grocery

Shopping lists, recipe exports and nutrition estimates from a library of
[Cooklang](https://cooklang.org) recipes: a CLI, a small web service, and a
phone app (PWA) for shopping together, with or without signal.

```
crates/
  grocery-core/    parse → resolve → aggregate → render, and nutrition (pure)
  grocery-store/   SQLite: NEVO data, recipe index, saved shopping lists
  grocery-server/  Axum service: watches Recipes/, JSON API, live events,
                   md/txt exports, and serves the web app
  grocery-cli/     the `grocery` binary (including `grocery serve`)
web/               the web app: Svelte 5 + TypeScript, built with Vite
examples/library/  a small library to try it on (also used by the tests)
```

## Quick start

`nix develop` gives you Rust, Node and sqlite3 (see `flake.nix`).

```sh
cargo test --workspace
(cd web && npm ci && npm test)
export GROCERY_LIBRARY=examples/library        # or pass --library
cargo run -q -- check
cargo run -q -- list "Pasta arrabbiata=2" Shakshuka            # Markdown
cargo run -q -- list --plain Shakshuka                         # for Signal
cargo run -q -- export "Stamppot boerenkool=2"                 # recipe as Markdown
cargo run -q -- export --plain Shakshuka                       # recipe as text
cargo run -q -- show "stamppot boerenkool"                     # how it parsed
```

Recipes can be given as a path or by name (case-insensitive file name from
`Recipes/`), with `=N` for servings.

## Library layout

```
Recipes/           .cook files, subfolders allowed
Food/aliases.yaml  ingredient names → foods, aisle, gram conversions, NEVO code
Food/pantry.yaml   staples never put on the list
Food/custom.yaml   your own nutrition values, for foods NEVO doesn't have
```

Only `Recipes/` and `Food/` are read; nothing is written there. Back up the
library and the database (below), which holds your saved lists.

## Recipes

Parsed by the official `cooklang` crate:

```
---
title: Pasta arrabbiata
servings: 4
tags: [pasta, weeknight]
---
Cook @spaghetti{400%g}. Fry @onion{1} and @garlic{3%cloves} in @olive oil{3%tbsp}.
Add @chili flakes{=1%tsp} (fixed: doesn't scale) and @?parmesan{60%g} (optional).
Season with @salt. Serve with @basil{a handful}.
```

`servings` is needed to scale; `title` defaults to the file name. Hidden
ingredients (`@-water`) are never put on a list.

## Web service

```sh
cargo run --release -- serve                     # 127.0.0.1:8080
cargo run --release -- serve --listen 0.0.0.0:8080
```

At startup it indexes `Recipes/` into the database, then watches the folder
and re-indexes after every change (only files whose mtime or size changed
are re-parsed). A recipe that fails to parse stays in the index with its
error, so you can see what's wrong instead of it disappearing.

There is **no authentication**. Keep it on localhost or a private network
(Tailscale, WireGuard), or put an authenticating reverse proxy in front.

### API

```
GET    /api/status
GET    /api/events                            server-sent events (below)
POST   /api/reindex                           re-parse every recipe
GET    /api/recipes?q=boerenkool              search title, tags, ingredients
GET    /api/recipes/{path}?format=&servings=  one recipe (JSON), or md/txt export
GET    /api/nutrition/{path}?all=&optional=   per-serving report (text)
GET    /api/lists                             newest first
POST   /api/lists                             {"title"?, "recipes": [{"path", "servings"?}]}
GET    /api/lists/{id}?format=                JSON, md (all items, ticked as [x]),
                                              or txt (only what's left to buy)
PATCH  /api/lists/{id}                        {"title"?, "status"?: "open" | "done"}
DELETE /api/lists/{id}
POST   /api/lists/{id}/items                  {"name": "toilet paper"}
PATCH  /api/lists/{id}/items/{item}           {"checked": true}
DELETE /api/lists/{id}/items/{item}
```

`{path}` is relative to `Recipes/`, as `/api/recipes` lists it (URL-encode
spaces). Only indexed files can be read through it. For example:

```sh
curl -s localhost:8080/api/lists -H 'content-type: application/json' \
  -d '{"recipes":[{"path":"Pasta arrabbiata.cook","servings":2},{"path":"Shakshuka.cook"}]}'
curl -s 'localhost:8080/api/lists/1?format=txt'        # paste into Signal
curl -s 'localhost:8080/api/lists/1?format=md' > list.md
curl -s 'localhost:8080/api/recipes/Shakshuka.cook?format=md&servings=4'
```

A saved list is a snapshot: editing a recipe later doesn't change a list
you're shopping from. Make a new list to pick up the change.

`/api/events` streams `{"type":"list","id":3}` whenever a list is created,
changed or deleted, and `{"type":"recipes"}` when the index changes. Events
carry no data: clients refetch what they show. `{"type":"resync"}` means a
client fell behind and should refetch everything.

### Running it on the server

A systemd unit, for example (`/etc/systemd/system/grocery.service`):

```ini
[Service]
User=grocery
Environment=GROCERY_LIBRARY=/srv/recipes
Environment=GROCERY_DB=/var/lib/grocery/grocery.sqlite
ExecStart=/usr/local/bin/grocery serve --listen 127.0.0.1:8080
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

It stops cleanly on SIGTERM.

## Web app

`grocery serve` also serves the app at `/`. It's built into the binary:

```sh
cd web && npm ci && npm run build      # → web/dist
cargo build --release                  # embeds web/dist
```

A release build without `web/dist` still compiles and runs, but shows a
"not built" page at `/`. Debug builds read `web/dist` from disk, so a new
`npm run build` shows up without recompiling.

What it does:

- **Lists overview**: open lists with progress; done lists folded away.
- **Shopping**: items grouped by aisle in walking order. Tap to tick;
  ticked items sink to the bottom of their aisle (or hide them). Add items
  by hand. Share what's left (share sheet, or clipboard), copy as Markdown,
  keep the screen on, mark done, rename, delete.
- **New list**: search recipes by title, tag or ingredient, set servings,
  and make the list.
- **Recipe**: scaled ingredients, method, and nutrition per serving.

**Shopping together.** Both phones stay connected to `/api/events`, so a
tick on one shows on the other within a second. Ticks set "checked" or
"unchecked" rather than toggling, so when you both tap the same item, the
last tap wins and nothing flips back unexpectedly.

**Without signal.** Every change is saved on the phone first and shown at
once, then sent in order when the server can be reached (on reconnect, when
the app comes back to the foreground, and every 15 s). The header shows
*Live*, *Offline* or *N to sync*. Items added offline get their real id once
sent, and ticks made on them in the meantime follow. Lists, recipes you've
opened and the app itself are cached, so the app also opens offline. Making
a new list, renaming, deleting and nutrition need the server.

**HTTPS is required** for the offline part and for installing to the home
screen: browsers only allow service workers on HTTPS (or `localhost`). Over
plain HTTP everything else works, but the app won't open without a
connection. With Tailscale, `tailscale serve --bg 8080` puts the service on
`https://<machine>.<tailnet>.ts.net` with a valid certificate, reachable
only from your tailnet. Then open that URL on each phone and choose "Add to
Home Screen" (iOS: Share menu; Android: browser menu).

### Developing the app

```sh
cargo run -- --library examples/library serve   # API on :8080
cd web && npm run dev                           # app on :5173, /api proxied
npm test          # vitest: offline sync, formatting, routes
npm run check     # svelte-check (types)
```

The service worker only registers in production builds, so `npm run dev`
always runs fresh code.

## Database

`$XDG_DATA_HOME/grocery/grocery.sqlite` (`~/.local/share/...`) by default;
override with `--db` or `GROCERY_DB`. It holds:

- NEVO data: rebuilt by `import-nevo`.
- The recipe index: rebuilt from the files at every start.
- **Saved shopping lists**: these exist nowhere else. If you care about
  them, include the database in your backup. It runs in WAL mode, so copy it
  with `sqlite3 grocery.sqlite ".backup backup.sqlite"` rather than `cp`
  while the service is running.

A database from step 3 is upgraded in place the first time it's opened.

## Nutrition (needs the NEVO export)

Download the dataset from RIVM (free, after accepting their conditions of
use), unzip it, then:

```sh
cargo run -q -- import-nevo ~/Downloads/NEVO2025          # folder or main CSV
cargo run -q -- nutrition Shakshuka
cargo run -q -- nutrition Shakshuka --all --with-optional
cargo run -q -- nevo search boerenkool                    # find codes
cargo run -q -- nevo show 959                             # one food per 100 g
cargo run -q -- nevo suggest                              # codes for unmapped foods
```

## Your own foods (custom.yaml)

For something NEVO doesn't have (fresh coriander, a branded product), copy
the numbers off the package. They live in `Food/custom.yaml`, apart from the
NEVO data: a re-import replaces NEVO wholesale, RIVM's conditions require
its values to stay unchanged, and reports say which numbers are yours.

```sh
grocery custom add "fresh coriander"     # asks for each line of the label
grocery custom add tahini --kcal 595 --fat 53 --protein 17 --carbs 9 --salt 0.05 \
  --source "jar, Albert Heijn"           # or give everything as flags
grocery custom list                      # what you have, and what uses it
```

or edit the file yourself (the command appends to it and keeps your comments):

```yaml
foods:
  fresh coriander:
    per: 100g                  # or 100ml; default 100g
    source: package, Albert Heijn
    energy_kcal: 23            # energy_kj works too (kcal is derived)
    protein: 2.1
    fat: 0.5
    saturated_fat: 0.03
    carbohydrate: 3.7
    sugars: 0.9
    fibre: 2.8
    salt: 0.1                  # label salt, g; or sodium_mg
    nutrients: { VITC: 27 }    # any NEVO nutrient code, in NEVO's unit
```

Then point a food in `aliases.yaml` at it, instead of a NEVO code:

```yaml
  coriander:
    custom: fresh coriander
```

- Values are per 100 g (or 100 ml), as on a label. Salt is converted to
  sodium the EU way (sodium × 2.5 = salt), so it adds up with NEVO's.
- **Leave out what the label doesn't list.** A missing value is unknown, not
  0: totals that use the food get the `*` and list it under "no value for".
- The report adds "Own values from Food/custom.yaml, not NEVO data: …" with
  your `source`, and marks those lines `(own value)`. The NEVO citation stays.
- Mistakes are errors, not silent zeros: unknown fields, negative numbers,
  both `salt` and `sodium_mg`, both `nevo` and `custom` on one food. A
  nutrient code NEVO doesn't have is called out in the report (typo?).
- `grocery check` lists foods with neither a `nevo` nor a `custom` entry, and
  `custom:` names that aren't in the file.
- For a homemade mix, prefer a recipe over typed-in numbers (they'd go stale
  when the recipe changes). Recipe references aren't expanded yet.

## aliases.yaml

See `examples/library/Food/aliases.yaml`. Each food can have `synonyms`, an
`aisle`, and the data to convert to grams: `piece_g`, `units` (per named unit)
and `density` (g/ml), plus `nevo:` (or `custom:`, see above) for nutrition. Typos in field names
and a synonym listed under two foods are errors, not silent misses.

`grocery check` lists every ingredient name that doesn't resolve and every
food used without a `nevo` code, with the recipes using them. That's the
to-do list for this file. `grocery nevo suggest` proposes codes.

## How aggregation works

1. Scale each recipe to the requested servings (fixed amounts stay fixed).
2. Resolve names through `aliases.yaml` (exact, then naive singular).
3. Normalize units: mass → g, volume → ml, no unit → pieces, anything else
   is a named unit ("cloves", "blik").
4. Sum per food and dimension. A food in several dimensions is merged only
   if every part converts to grams with data from `aliases.yaml`, and is
   then shown in the unit you buy it in (2 onions + 100 g onion → 3).
   Otherwise it stays two lines: no guessing.
5. Drop pantry items, sort by the aisle order in `aliases.yaml`, render.

Counts and named units round up (you can't buy 0.75 onion). Ranges (`2-3`)
use the upper bound.

## How nutrition works

For each ingredient line: resolve the name → take the food's `nevo` code →
convert the amount to grams (ml for the few NEVO foods given per 100 ml)
with the food's `piece_g` / `units` / `density` → add `value × amount / 100`
for every nutrient. Totals are divided by the recipe's `servings`.

- **Unknown is not zero.** NEVO leaves some cells empty (vitamin K for 1030
  foods, iodine for 480, ...). Those stay absent; a total that's missing a
  value for some ingredient is marked `*` and the report lists which foods
  lack what. Trace amounts are 0 in NEVO itself and are kept as 0.
- **Coverage** is counted weight ÷ known weight. Lines that can't be counted
  are listed with the reason: not in aliases, no `nevo` or `custom` entry, code
  not in the dataset (or name not in custom.yaml), or no gram weight for the
  unit. "To taste" lines are listed
  separately, as they have no weight. Salt to taste is therefore never in the
  salt figure.
- **Optional** ingredients are left out unless `--with-optional` (API: `?optional=true`).
- **Salt** = sodium × 2.5, as on EU labels. **%RI** uses the EU reference
  intakes (Regulation 1169/2011, Annex XIII); vitamin A is shown as RAE,
  folate as dietary folate equivalents, niacin as niacin equivalents.
- **Search** rebuilds NEVO's inverted compounds ("Kool boeren-" →
  boerenkool, "Olie olijf-" → olijfolie), searches English names and
  synonyms, and prefers raw products, NEVO averages and ingredients over
  prepared dishes. Hyphenated modifiers ("Koekje roomboter-") rank below
  the food itself.

### NEVO conditions of use

From RIVM's conditions for NEVO-online 2025/9.0, which this code follows:

- Every nutrition output carries "Based on data from NEVO online version
  <version>, RIVM, Bilthoven"; the version comes from the imported file.
- Data must be used unchanged: values are stored exactly as published.
  `aliases.yaml` weights and densities and everything in `custom.yaml` are
  your own additions, kept apart and labelled as such in reports.
- No charging end users for the data.

The dataset isn't included in this repository. Tests use synthetic fixtures;
the real export is only read by an ignored test
(`NEVO_DIR=... cargo test -p grocery-store -- --ignored`).

## Known limitations

- Recipe references (`@./sauces/pesto{}`) aren't expanded; a warning tells
  you to add that recipe to the list separately.
- Recipe exports show amounts as written. They're summed per ingredient only
  when the unit is spelled the same, so `2 cloves` + `1 clove` stays as is.
- Volume without a `density` is never converted to grams, so "200 ml milk +
  100 g milk" stays two lines until you add a density.
- Plural handling for named units is naive (strips one trailing "s"). List
  Dutch plurals explicitly under `units:` (e.g. `teen` and `tenen`).
- NEVO values are per 100 g *edible part*. Recipes weigh as bought, so
  potatoes with peel or eggs with shell are slightly overestimated. Use a
  lower `piece_g` (edible weight) if that matters to you.
- Raw vs cooked matters: map to the "rauw" entry when the recipe gives raw
  weights. Vitamin losses during cooking aren't modelled.
- A few foods aren't in NEVO (fresh coriander, for one); they show up as
  "no nevo or custom entry" rather than being guessed. Add them to
  `custom.yaml` (above).
- cooklang 0.19 quirks handled here: its `ADVANCED_UNITS` extension is off
  (it rejects `~{5%minutes}` without bundled units), `@x{100 g}` without `%`
  is recovered from the text value, and a spurious "unnecessary scaling
  lock" warning is filtered. Revisit when upgrading the crate.

## Ideas

- Reordering aisles from the app (they come from `aliases.yaml` now).
- A "frequently added" suggestion list for hand-added items.
