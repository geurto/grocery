# grocery

Shopping lists and nutrition estimates from recipes in an Obsidian vault.

Steps 2 and 3 of the plan: a library, a SQLite store for NEVO food
composition data, and a CLI. The web service and PWA (steps 4–5) build on
these crates without changing them.

```
crates/
  grocery-core/   parse → resolve → aggregate → render, and nutrition (pure)
  grocery-store/  SQLite: NEVO import, composition lookup, name search
  grocery-cli/    the `grocery` binary
examples/vault/   a small vault to try it on (also used by the integration test)
```

## Quick start

```sh
cargo test --workspace
cargo run -q -- --vault examples/vault check
cargo run -q -- --vault examples/vault list "Pasta arrabbiata=2" Shakshuka
cargo run -q -- --vault examples/vault list --plain Shakshuka     # for Signal
cargo run -q -- --vault examples/vault list --write Shakshuka     # → Shopping/<date>.md
cargo run -q -- --vault examples/vault show "stamppot boerenkool"
```

Set `GROCERY_VAULT` to skip `--vault`. Recipes can be given as a path or by
name (case-insensitive file name from `Recipes/`), with `=N` for servings.

### Nutrition (needs the NEVO export)

Download the dataset from RIVM (free, after accepting their conditions of
use), unzip it, then:

```sh
cargo run -q -- import-nevo ~/Downloads/NEVO2025          # folder or main CSV
cargo run -q -- --vault examples/vault nutrition Shakshuka
cargo run -q -- --vault examples/vault nutrition Shakshuka --all --with-optional
cargo run -q -- nevo search boerenkool                    # find codes
cargo run -q -- nevo show 959                             # one food per 100 g
cargo run -q -- --vault examples/vault nevo suggest       # codes for unmapped foods
```

The database goes to `$XDG_DATA_HOME/grocery/grocery.sqlite`
(`~/.local/share/...`); override with `--db` or `GROCERY_DB`. Keep it out of
the vault: it's rebuilt from the export and shouldn't sync.

## Vault layout

```
Recipes/          .cook (Cooklang) or .md with `ingredients:` in frontmatter
Food/aliases.yaml ingredient names → foods, aisle, gram conversions, NEVO code
Food/pantry.yaml  staples never put on the list
Shopping/         written by the app; never edited or deleted by it
```

## Recipe formats

Both formats produce the same `model::Recipe`; pick per recipe.

**Cooklang** (`.cook`), parsed by the official `cooklang` crate:

```
---
servings: 4
---
Cook @spaghetti{400%g}. Fry @onion{1} and @garlic{3%cloves} in @olive oil{3%tbsp}.
Add @chili flakes{=1%tsp} (fixed: doesn't scale) and @?parmesan{60%g} (optional).
```

**Markdown** (`.md`) with the list in frontmatter. Notes without `ingredients`
are ignored, so recipes can live next to ordinary notes.

```yaml
---
servings: 2
ingredients:
  - 1 ui                                    # shorthand: qty [unit] name[, note] [(optional)]
  - 2 tenen knoflook
  - 0,5 l halfvolle melk
  - { item: feta, qty: 100, unit: g, optional: true }
  - { item: salt, qty: 1, unit: tsp, scale: false }
  - verse koriander                         # no quantity = to taste
---
```

The shorthand only treats known words as units (g, el, tl, cloves, blik,
bos, ...) so "2 sweet potatoes" stays a name. Use the `{ item, qty, unit }`
form for anything unusual.

## aliases.yaml

See `examples/vault/Food/aliases.yaml`. Each food can have `synonyms`, an
`aisle`, and the data to convert to grams: `piece_g`, `units` (per named unit)
and `density` (g/ml), plus `nevo:` for nutrition. Typos in field names
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
  are listed with the reason: not in aliases, no `nevo` code, code not in the
  dataset, or no gram weight for the unit. "To taste" lines are listed
  separately, as they have no weight. Salt to taste is therefore never in the
  salt figure.
- **Optional** ingredients are left out unless `--with-optional`.
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
  `aliases.yaml` weights and densities are your own additions, not NEVO data.
- No charging end users for the data.

The dataset isn't included in this repository. Tests use synthetic fixtures;
the real export is only read by an ignored test
(`NEVO_DIR=... cargo test -p grocery-store -- --ignored`).

## Known limitations

- Recipe references (`@./sauces/pesto{}`) aren't expanded; a warning tells
  you to add that recipe to the list separately.
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
  "no nevo code" rather than being guessed.
- cooklang 0.19 quirks handled here: its `ADVANCED_UNITS` extension is off
  (it rejects `~{5%minutes}` without bundled units), `@x{100 g}` without `%`
  is recovered from the text value, and a spurious "unnecessary scaling
  lock" warning is filtered. Revisit when upgrading the crate.

## Next steps

- **Step 4:** Axum service watching the vault, writing to `Shopping/`, using
  `grocery-store` for NEVO and a derived recipe index.
- **Step 5:** PWA for checking items off.
