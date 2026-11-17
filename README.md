# grocery

Shopping lists, recipe exports, a cooking mode, meal plans and nutrition
estimates from a library of [Cooklang](https://cooklang.org) recipes: a CLI,
a small web service, and a phone app (PWA) for shopping together, with or
without signal.

```
crates/
  grocery-core/    parse → resolve → aggregate → render, and nutrition (pure)
  grocery-store/   SQLite: NEVO data, recipe index, shopping lists, meal plans
  grocery-server/  Axum service: watches Recipes/, JSON API, live events,
                   md/txt exports, and serves the web app
  grocery-cli/     the `grocery` binary (including `grocery serve`)
web/               the web app: Svelte 5 + TypeScript, built with Vite
library/           your recipes and food data
tests/fixtures/library/  the fixed library the tests run against
```

## Quick start

`nix develop` gives you Rust, Node and sqlite3 (see `flake.nix`).

```sh
cargo test --workspace
(cd web && npm ci && npm test)
export GROCERY_LIBRARY=library                 # or pass --library
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

`Recipes/Uitproberen/` holds recipes you're trying out, tagged `proberen`:
move one to its proper folder (and drop the tag) if it's a keeper, or delete
it. In Claude Code, `/import-recipe <url, photo or pasted text>` writes one
there from a web page or cookbook page, in the library's style and food names,
and makes a shopping list from it (`.claude/skills/import-recipe/`).

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

**Time.** Timers (`~{20%minutes}`, `~{2-3%minuten}`, `~{1%uur}`) are shown
in the steps, and tapping one in cooking mode starts it. For the whole
recipe, add `time: 40 min` (or `prep time:` / `cook time:`) to the
metadata; `1 uur 15 min`, `1h15m` and a bare `75` (minutes) also work, and
`tijd`, `bereidingstijd`, `voorbereidingstijd` and `kooktijd` are read too.
Nothing is estimated: without `time:` the app shows the sum of the timers,
labelled as such, since chopping and "fry until golden" have no timer. A
`time:` it can't read is a warning, not a guess.

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
POST   /api/lists                             {"title"?, "recipes"?: [{"path", "servings"?}],
                                               "items"?: [{"name", "amount"?, "unit"?}]}
                                              (nothing: an empty list to fill as you go)
GET    /api/lists/{id}?format=                JSON, md (all items, ticked as [x]),
                                              or txt (only what's left to buy)
PATCH  /api/lists/{id}                        {"title"?, "status"?: "open" | "done"}
DELETE /api/lists/{id}
POST   /api/lists/{id}/items                  {"name": "limoen", "amount"?: 2, "unit"?}; a known
                                              food gets its aisle ("melk" → milk), and an item
                                              joins the line of the same food → {"id"} of the line
POST   /api/lists/{id}/recipes                {"recipes": [{"path", "servings"?}]}: merged in
DELETE /api/lists/{id}/recipes/{path}         its share comes off every line it's on
PATCH  /api/lists/{id}/items/{item}           {"checked": true}
DELETE /api/lists/{id}/items/{item}
GET    /api/people
POST   /api/people                            {"name": "Sam"}
PATCH  /api/people/{id}                       {"name": "Sam"}
DELETE /api/people/{id}                       and their plan
GET    /api/plan?person=&from=&to=            entries; days as YYYY-MM-DD, inclusive
POST   /api/plan                              {"person", "day", "recipe", "meal"?, "portions"?}
                                              or a food: {"person", "day", "food": "banaan",
                                               "amount"?: 1, "unit"?: "g", "meal"?}
PATCH  /api/plan/{id}                         {"day"?, "meal"?: "dinner" | null, "portions"?}
                                              (a food: {"amount", "unit"?} instead of portions)
DELETE /api/plan/{id}
GET    /api/plan/nutrition?person=&from=&to=  per-day totals (JSON, with the NEVO citation)
```

Writing to the library (what the app's editor uses):

```
POST   /api/recipe-check                      {"source"} → parse result, ingredients vs aliases.yaml
GET    /api/recipe-folders                    folders under Recipes/
GET    /api/recipe-files/{path}               {"source", "version"}: a recipe as editor fields
POST   /api/recipe-files                      {"folder"?, "source"} → 201 {"path"}; 409 if it exists
PUT    /api/recipe-files/{path}               {"source", "version"}; 409 if the file changed meanwhile
GET    /api/foods                             what aliases.yaml knows
GET    /api/foods/{name}/nutrition            per 100 g (or ml), with its source and citation
POST   /api/foods                             {"name", "aisle"?, "synonyms"?, "nevo"? | "custom"? |
                                               "own"? | "negligible"?, "piece_g"?, "density"?, "units"?}
GET    /api/aisles
GET    /api/nevo/search?q=&limit=             NEVO foods by name (409 until imported)
```

`source` is `{title, servings, time, tags, extra, body}`: the recipe's
front matter as fields (`extra` is any other front matter, as YAML) and the
method as Cooklang. Every write is built as text, parsed again, and only then
saved: a new recipe never overwrites a file, an edit is refused (409) if the
file was changed since you opened it, a food is appended to `aliases.yaml`
(and its own values to `custom.yaml`) without touching the rest of the file,
and a mistake changes nothing. **The service can now write to your library**,
and it has no authentication: keep it on localhost or a private network.

A recipe's JSON has a `display` part for showing it: scaled ingredient
lines, the stated time and the sum of its timers, and the method as steps
whose `items` keep ingredients (with this step's amount, scaled), cookware
and timers (with `seconds`) apart from the text.

`{path}` is relative to `Recipes/`, as `/api/recipes` lists it (URL-encode
spaces). Only indexed files can be read through it. For example:

```sh
curl -s localhost:8080/api/lists -H 'content-type: application/json' \
  -d '{"recipes":[{"path":"Diner/Chana masala.cook","servings":2},{"path":"Diner/Vega paella.cook"}]}'
curl -s 'localhost:8080/api/lists/1?format=txt'        # paste into Signal
curl -s 'localhost:8080/api/lists/1?format=md' > list.md
curl -s 'localhost:8080/api/recipes/Diner/Chana%20masala.cook?format=md&servings=2'
```

A saved list is a snapshot: editing a recipe later doesn't change a list
you're shopping from. Make a new list to pick up the change.

`/api/events` streams `{"type":"list","id":3}` whenever a list is created,
changed or deleted, `{"type":"plan","person":1}` when someone's plan
changes, `{"type":"people"}`, and `{"type":"recipes"}` when the index
changes. Events
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

### Docker

The image holds the binary (with the web app embedded) and the `library/`
folder (recipes and food data) as it was on `main`. The NEVO data is not baked
in: the database lives in a volume. To use a local library folder instead,
uncomment the `/library` mount in `docker-compose.yml`. Without that mount,
recipes created or edited in the app's editor live only in the container and
are lost when it's recreated, so commit new recipes to the repo.

Every push to `main` (code or recipes) runs `.github/workflows/docker.yml`,
which builds a `linux/amd64` image and pushes it to Docker Hub as
`<user>/grocery:latest` and `:sha-<commit>`. It needs two repository secrets:
`DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` (a Docker Hub access token with
read/write). It can also be started by hand from the Actions tab. To have
the server pick up new images on its own, run something like
[Watchtower](https://containrrr.dev/watchtower/) next to it, or a cron job
doing `docker compose pull && docker compose up -d`.

```sh
# set GROCERY_IMAGE in .env to <dockerhub-user>/grocery, then:
docker login
docker compose build && docker compose push
# or for an arm64 server (e.g. a Pi) as well:
docker buildx build --platform linux/amd64,linux/arm64 -t <user>/grocery:latest --push .

# on the server: copy docker-compose.yml and .env (and your library/ folder
# if you mount it: sudo chown -R 1000:1000 library, the container runs as uid 1000)
docker compose up -d
```

Settings are in `.env` (image, port, bind address, library folder, `TZ`).
The port is published on `127.0.0.1` by default because there is no login;
see "Web app" for HTTPS via `tailscale serve`. Import NEVO into the volume
once. To back up the database (it holds your lists and plans), stop the
service so the WAL is flushed, then copy the file out:

```sh
docker compose run --rm --no-deps -v ~/Downloads/NEVO2025:/nevo:ro grocery import-nevo /nevo
docker compose stop && docker compose cp grocery:/data/grocery.sqlite . && docker compose start
```

## Web app

`grocery serve` also serves the app at `/`. It's built into the binary:

```sh
cd web && npm ci && npm run build      # → web/dist
cargo build --release                  # embeds web/dist
```

A release build without `web/dist` still compiles and runs, but shows a
"not built" page at `/`. Debug builds read `web/dist` from disk, so a new
`npm run build` shows up without recompiling.

Three tabs: **Lists**, **Recipes** and **Plan**.

- **Lists overview**: open lists with progress; done lists folded away.
- **New list** makes an empty list; fill it as you go.
- **Shopping**: items grouped by aisle in walking order. Tap to tick;
  ticked items sink to the bottom of their aisle (or hide them). Share
  what's left (share sheet, or clipboard), copy as Markdown, keep the screen
  on, mark done, rename, delete.
- **Adding items**: type and pick from your foods as you type (name or any
  synonym: "ui" offers onion); a known food lands in its aisle under its own
  name, also offline. ⓘ next to a suggestion shows its nutrition per 100 g.
  Anything else ("toilet paper") goes under Other. An amount can go before
  or after the name ("500 g kaas", "2 bananen", "kaas 1 kg"); it stays when
  you pick a suggestion. A unit is only read as one when it's a known one
  (g, kg, ml, l, el, tl, stuks, blik, bos, zak, pak, …), so "2 bananen" is
  two bananas. An item joins the line of the same food when the amounts add
  up (2 limoen, then 12 limoen, on a list whose curry needs 6: one line of
  20), and the line says who needs what: "Curry 6 · added by hand 14".
- **+ Recipes** on a list (or **Add to a list…** on a recipe): their
  ingredients are merged into what's there. The same food adds up when the
  units allow it (2 onions + 220 g of onion becomes 440 g when the onion has
  a piece weight); otherwise it gets its own line. A ticked line that now
  needs more is unticked, so it isn't missed. Adding a recipe that's already
  on the list counts it twice as many servings.
- **Removing items**: **Edit** puts a remove button on every item (items
  added by hand always have one), with a few seconds to undo.
- **Removing recipes**: **+ Recipes** lists the recipes on the list, each
  with a remove button (and a few seconds to undo). What only that recipe
  needed goes; lines others need too shrink by its share; ticked lines stay
  ticked. A recipe added twice comes off as a whole. Lines saved before the
  app kept track of who needs what (before schema v6) subtract what the
  recipe says now, so an edit to the recipe since can leave a little behind.
- **Recipes**: by folder (`Diner/`, `Ontbijt/`), searchable.
- **New recipe / Edit**: fields for title, folder, servings, time and tags,
  and the method in Cooklang. Buttons insert `@ingredient{amount%unit}`,
  `~{10%minutes}` and `#cookware{}` from a small form, so you don't type the
  braces on a phone. While you type, the recipe is checked against your food
  list: ingredients that aren't there yet get an **Add food** button right
  there. A recipe that doesn't parse can't be saved, and the message says
  where. What you typed survives a reload (a draft kept on the phone), and
  saving is refused if the file changed on disk meanwhile. It writes a plain
  `.cook` file in `Recipes/`, so the usual `git commit` applies. A recipe's
  other front matter (author, source, …) is kept as it was.
- **Foods**: tap a food for its nutrition per 100 g (NEVO or your own
  values, with what's unknown shown as unknown).
- **Add food** (from the editor, or the *Foods* screen): name and other
  names, aisle, and where nutrition comes from: **NEVO** (search by Dutch or
  English name, with kcal to tell raw from cooked), **your own values** (the
  numbers on the package, stored in `custom.yaml`), **negligible** (a pinch of
  dried herbs), or later. Optionally a piece weight, a density (so el, tl and
  ml become grams) and other units. It's appended to `Food/aliases.yaml` with
  a comment naming the NEVO food, leaving the rest of the file as it was.
- **Recipe**: scaled ingredients; the method with each step's amounts in
  place and timers as chips; time; nutrition per serving.
- **Cooking mode** ("Start cooking"): big steps, one highlighted at a time;
  tap the number to mark a step done. A "Set out" checklist for the
  ingredients. The screen stays on. Tap a timer to start it (a range rings at
  the short end): several can run at once, with +1 min, and they keep going
  across screens (a pill shows the next one) and reloads, because a timer is
  stored as the time it ends. When one goes off: a banner, vibration, a beep,
  and a notification if the app is in the background and you allowed them.
  A timer can't wake a phone whose browser has fully closed the app.
  Progress is kept on that device for two days, so you can leave and come
  back ("Continue cooking").
- **Plan**: one plan per person, so each sees their own calories. Per day:
  recipes by meal, with portions in halves (a portion is one of the
  recipe's servings), and single foods from your food list with an amount
  ("a banana", "25 g cashews": search for the food under **+ Add**). A food
  counts only when its amount can be weighed (grams, or pieces of a food
  with a piece weight); otherwise it says why. Below that, the day's energy, macros, salt, and
  (folded) vitamins and minerals as amounts and % of the EU reference
  intakes, with the same honesty as the recipe report: `*` marks a lower
  bound and says which foods have no value; recipes without `servings`
  aren't counted and say so; low ingredient coverage is flagged. The week
  strip shows each day's kcal. **Shopping list for this week** sums
  everyone's portions per recipe for the days you pick, so two people eating
  a curry, or one person eating the leftovers tomorrow, both buy for two
  servings. Planned single foods go on the list too, merged with what the
  recipes need.

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
a new list, renaming, deleting, nutrition and changing a plan need the
server (plans you've looked at can be read offline).

**HTTPS is required** for the offline part and for installing to the home
screen: browsers only allow service workers on HTTPS (or `localhost`). Over
plain HTTP everything else works, but the app won't open without a
connection. With Tailscale, `tailscale serve --bg 8080` puts the service on
`https://<machine>.<tailnet>.ts.net` with a valid certificate, reachable
only from your tailnet. Then open that URL on each phone and choose "Add to
Home Screen" (iOS: Share menu; Android: browser menu).

### Developing the app

```sh
cargo run -- --library library serve   # API on :8080
cd web && npm run dev                           # app on :5173, /api proxied
npm test          # vitest: offline sync, formatting, routes, timers, plans
npm run check     # svelte-check (types)
```

The service worker only registers in production builds, so `npm run dev`
always runs fresh code.

## Database

`$XDG_DATA_HOME/grocery/grocery.sqlite` (`~/.local/share/...`) by default;
override with `--db` or `GROCERY_DB`. It holds:

- NEVO data: rebuilt by `import-nevo`.
- The recipe index: rebuilt from the files at every start.
- **Saved shopping lists and meal plans**: these exist nowhere else. If you
  care about them, include the database in your backup. It runs in WAL mode, so copy it
  with `sqlite3 grocery.sqlite ".backup backup.sqlite"` rather than `cp`
  while the service is running.

An older database is upgraded in place the first time it's opened (v6 records who needs how much of a list line, v5 adds single foods to plans, v3 adds
the plan tables). Downgrading isn't supported: an older build refuses a
newer database rather than guess.

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
- **Negligible foods.** Dried herbs and spices used by the pinch aren't worth
  looking up. Mark them `negligible: true` in `aliases.yaml` (instead of
  `nevo:`/`custom:`): they're left out of nutrition, don't count towards
  coverage, don't put a `*` on totals, and the report just lists them under
  "Negligible, left out". `check` and `nevo suggest` stop asking about them.
  If a food does matter in larger amounts (say, a whole tablespoon of paprika),
  give it a `custom:` entry instead.
- For a homemade mix, prefer a recipe over typed-in numbers (they'd go stale
  when the recipe changes). Recipe references aren't expanded yet.

## aliases.yaml

See `library/Food/aliases.yaml`. Each food can have `synonyms`, an
`aisle`, and the data to convert to grams: `piece_g`, `units` (per named unit)
and `density` (g/ml), plus `nevo:` (or `custom:`, see above) for nutrition.
Typos in field names and a synonym listed under two foods are errors, not
silent misses.

Synonyms decide which food a recipe line is (for grams, nutrition and
merging amounts), not what the shopping list says: a list line shows the
word the recipe uses, so "krieltjes" stays "krieltjes" even though it counts
as `kruimige aardappelen`. If two recipes use different words for the same
food they share one line, "winterpeen / wortel — 3". Hand-added items show
what you typed, and the suggestions list reads "krieltjes (kruimige
aardappelen)" so you can see which food it counts as. Lists saved before
this show the food name.

**Spoons and millilitres.** `el`, `tl`, `ml`, `dl`, `cup` … are fixed volumes
(15 ml, 5 ml, …), so the only thing a food needs to turn them into grams is
its `density`. `density: 1.15` for soy sauce means 1 el = 17 g. There is
deliberately no default density: oil is 0.92, honey 1.4 and ground spices
0.4 to 0.5, so a guess of 1 would be 10 to 60% off. `units:` is for things
that aren't a fixed size ("clove", "blik", "bos"); putting `tl` or `el`
there is an error, because it could never be used.

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

- Renaming or deleting a recipe, and changing or removing a food, from the
  app (editing a recipe's content works; those are still done in the files).
- Single foods in a plan ("a banana", "150 g yoghurt") next to recipes.
- Moving a plan entry to another day (the API can; the app can't yet).
- Recipe times in the recipe list (needs the time in the index).

- Reordering aisles from the app (they come from `aliases.yaml` now).
- A "frequently added" suggestion list for hand-added items.
