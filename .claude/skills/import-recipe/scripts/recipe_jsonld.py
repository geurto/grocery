"""Print the schema.org Recipe embedded in a saved web page, as JSON.

Usage: python3 -I recipe_jsonld.py PAGE.html

Most recipe sites put the recipe in a <script type="application/ld+json">
block, sometimes nested in an @graph or a list. Prints the first Recipe
found, trimmed to the fields an import needs, or exits 1 if there is none.
Standard library only; the page is read as data, never executed.
"""

import json
import sys
from html.parser import HTMLParser

KEEP = [
    "name", "description", "author", "recipeYield", "prepTime", "cookTime",
    "totalTime", "recipeIngredient", "recipeInstructions", "recipeCategory",
    "recipeCuisine", "keywords", "url",
]


class LdJson(HTMLParser):
    def __init__(self):
        super().__init__()
        self.blocks, self._buf = [], None

    def handle_starttag(self, tag, attrs):
        if tag == "script" and dict(attrs).get("type", "").strip() == "application/ld+json":
            self._buf = []

    def handle_data(self, data):
        if self._buf is not None:
            self._buf.append(data)

    def handle_endtag(self, tag):
        if tag == "script" and self._buf is not None:
            self.blocks.append("".join(self._buf))
            self._buf = None


def is_recipe(node):
    t = node.get("@type")
    return t == "Recipe" or (isinstance(t, list) and "Recipe" in t)


def find_recipe(node):
    if isinstance(node, list):
        for item in node:
            if (found := find_recipe(item)) is not None:
                return found
    elif isinstance(node, dict):
        if is_recipe(node):
            return node
        for key in ("@graph", "mainEntity", "itemListElement"):
            if key in node and (found := find_recipe(node[key])) is not None:
                return found
    return None


def steps(instructions):
    """Flatten HowToStep / HowToSection / plain strings to a list of lines."""
    if isinstance(instructions, str):
        return [instructions]
    out = []
    for item in instructions or []:
        if isinstance(item, str):
            out.append(item)
        elif isinstance(item, dict):
            if item.get("@type") == "HowToSection":
                out.append(f"## {item.get('name', '')}".rstrip())
                out.extend(steps(item.get("itemListElement")))
            elif "text" in item:
                out.append(item["text"])
    return out


def main():
    with open(sys.argv[1], encoding="utf-8", errors="replace") as f:
        parser = LdJson()
        parser.feed(f.read())
    for block in parser.blocks:
        try:
            data = json.loads(block)
        except json.JSONDecodeError:
            continue
        if (recipe := find_recipe(data)) is not None:
            trimmed = {k: recipe[k] for k in KEEP if k in recipe}
            trimmed["recipeInstructions"] = steps(recipe.get("recipeInstructions"))
            json.dump(trimmed, sys.stdout, ensure_ascii=False, indent=2)
            print()
            return
    print("no schema.org Recipe in this page", file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
