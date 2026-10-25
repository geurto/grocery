//! Saved shopping lists.
//!
//! A list is a snapshot: the aggregated items at the moment it was made.
//! Editing a recipe afterwards doesn't change a list someone is shopping
//! with. Items can be ticked off, and extra items added by hand.

use grocery_core::aggregate::{self, Item, Qty, ShoppingList, Source};
use grocery_core::food::FoodDb;
use grocery_core::units::Dim;
use rusqlite::{params, OptionalExtension};

use crate::{join_lines, split_lines, Error, Result, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStatus {
    Open,
    Done,
}

impl ListStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ListStatus::Open => "open",
            ListStatus::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(ListStatus::Open),
            "done" => Some(ListStatus::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListSummary {
    pub id: i64,
    pub title: String,
    /// As passed to [`Store::create_list`] (the service uses RFC 3339).
    pub created: String,
    pub status: ListStatus,
    pub items: usize,
    pub checked: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredItem {
    pub id: i64,
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredList {
    pub summary: ListSummary,
    pub sources: Vec<Source>,
    pub items: Vec<StoredItem>,
    pub pantry_skipped: Vec<String>,
}

impl StoredList {
    /// For the renderers in `grocery_core::render`.
    pub fn shopping_list(&self) -> ShoppingList {
        ShoppingList {
            sources: self.sources.clone(),
            items: self.items.iter().map(|i| i.item.clone()).collect(),
            pantry_skipped: self.pantry_skipped.clone(),
            unmapped: vec![],
            warnings: vec![],
        }
    }
}

fn dim_to_sql(dim: &Dim) -> String {
    match dim {
        Dim::Mass => "g".into(),
        Dim::Volume => "ml".into(),
        Dim::Count => "pc".into(),
        Dim::Named(unit) => format!("unit:{unit}"),
    }
}

fn dim_from_sql(s: &str) -> Option<Dim> {
    Some(match s {
        "g" => Dim::Mass,
        "ml" => Dim::Volume,
        "pc" => Dim::Count,
        _ => Dim::Named(s.strip_prefix("unit:")?.to_owned()),
    })
}

fn status_from_sql(s: String) -> rusqlite::Result<ListStatus> {
    ListStatus::parse(&s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown list status '{s}'").into(),
        )
    })
}

const SUMMARY: &str = "
    SELECT l.id, l.title, l.created, l.status,
           COUNT(i.id), COALESCE(SUM(i.checked), 0)
    FROM lists l LEFT JOIN list_items i ON i.list = l.id";

fn summary_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ListSummary> {
    Ok(ListSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        created: r.get(2)?,
        status: status_from_sql(r.get(3)?)?,
        items: r.get::<_, i64>(4)? as usize,
        checked: r.get::<_, i64>(5)? as usize,
    })
}

impl Store {
    /// Save `list` and return its id.
    pub fn create_list(&mut self, title: &str, created: &str, list: &ShoppingList) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO lists (title, created, status, pantry) VALUES (?1, ?2, 'open', ?3)",
            params![title, created, join_lines(&list.pantry_skipped)],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut src = tx.prepare(
                "INSERT INTO list_recipes (list, pos, path, title, servings, factor)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (pos, s) in list.sources.iter().enumerate() {
                src.execute(params![
                    id,
                    pos as i64,
                    s.path.to_string_lossy(),
                    s.title,
                    s.servings,
                    s.factor
                ])?;
            }
            for (pos, item) in list.items.iter().enumerate() {
                insert_item(&tx, id, pos as i64, item)?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// All lists, newest first.
    pub fn lists(&self) -> Result<Vec<ListSummary>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SUMMARY} GROUP BY l.id ORDER BY l.id DESC"))?;
        let rows = stmt.query_map([], summary_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn list(&self, id: i64) -> Result<Option<StoredList>> {
        let Some(summary) = self
            .conn
            .query_row(
                &format!("{SUMMARY} WHERE l.id = ?1 GROUP BY l.id"),
                [id],
                summary_from_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let pantry: String =
            self.conn
                .query_row("SELECT pantry FROM lists WHERE id = ?1", [id], |r| r.get(0))?;

        let sources = self
            .conn
            .prepare(
                "SELECT path, title, servings, factor FROM list_recipes WHERE list = ?1 ORDER BY pos",
            )?
            .query_map([id], |r| {
                Ok(Source {
                    path: r.get::<_, String>(0)?.into(),
                    title: r.get(1)?,
                    servings: r.get(2)?,
                    factor: r.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT id, name, aisle, dim, value, label, notes, optional, recipes, known, checked,
                    written
             FROM list_items WHERE list = ?1 ORDER BY pos, id",
        )?;
        let items = stmt
            .query_map([id], |r| {
                let dim: Option<String> = r.get(3)?;
                let quantity = match dim {
                    Some(d) => Some(Qty {
                        dim: dim_from_sql(&d).ok_or_else(|| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                format!("unknown dimension '{d}'").into(),
                            )
                        })?,
                        value: r.get(4)?,
                        label: r.get(5)?,
                    }),
                    None => None,
                };
                Ok(StoredItem {
                    id: r.get(0)?,
                    item: Item {
                        name: r.get(1)?,
                        written: split_lines(&r.get::<_, String>(11)?),
                        aisle: r.get(2)?,
                        quantity,
                        notes: split_lines(&r.get::<_, String>(6)?),
                        optional: r.get(7)?,
                        recipes: split_lines(&r.get::<_, String>(8)?),
                        known: r.get(9)?,
                        checked: r.get(10)?,
                    },
                })
            })?
            .collect::<Result<_, _>>()?;

        Ok(Some(StoredList {
            summary,
            sources,
            items,
            pantry_skipped: split_lines(&pantry),
        }))
    }

    /// Rename and/or change status. Returns `false` if there's no such list.
    pub fn update_list(
        &self,
        id: i64,
        title: Option<&str>,
        status: Option<ListStatus>,
    ) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE lists SET title = COALESCE(?2, title), status = COALESCE(?3, status) WHERE id = ?1",
            params![id, title, status.map(ListStatus::as_str)],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_list(&self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM lists WHERE id = ?1", [id])? > 0)
    }

    /// Add an item by hand ("toilet paper", "melk", "500 g kaas"). A name
    /// `db` knows becomes that food, with its aisle; the list is re-sorted
    /// so it lands in the right aisle. Returns the new item's id, or `None`
    /// if there's no such list.
    pub fn add_item(
        &mut self,
        list: i64,
        name: &str,
        quantity: Option<Qty>,
        db: &FoodDb,
    ) -> Result<Option<i64>> {
        let name = name.trim();
        if name.is_empty() || name.contains('\n') {
            return Err(Error::Invalid(
                "item name must be one non-empty line".into(),
            ));
        }
        let tx = self.conn.transaction()?;
        if !list_exists(&tx, list)? {
            return Ok(None);
        }
        let item = Item {
            quantity,
            ..aggregate::hand_item(name, db)
        };
        let id = insert_item(&tx, list, i64::MAX, &item)?;
        resort(&tx, list, db.aisle_order())?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// Add recipes to an existing list: their items are merged into the
    /// lines already there (see [`aggregate::merge_into`]), the recipes are
    /// added to the list's sources, and the list is re-sorted. Returns
    /// `false` if there's no such list.
    pub fn add_recipes(&mut self, list: i64, built: &ShoppingList, db: &FoodDb) -> Result<bool> {
        let Some(current) = self.list(list)? else {
            return Ok(false);
        };
        let ids: Vec<i64> = current.items.iter().map(|i| i.id).collect();
        let mut items: Vec<Item> = current.items.into_iter().map(|i| i.item).collect();
        let changed = aggregate::merge_into(&mut items, built.items.clone(), db);

        let tx = self.conn.transaction()?;
        for &i in &changed {
            update_item(&tx, ids[i], &items[i])?;
        }
        for item in &items[ids.len()..] {
            insert_item(&tx, list, i64::MAX, item)?;
        }
        // A recipe that's on the list already is cooked more: add up.
        let mut next: i64 = tx.query_row(
            "SELECT COALESCE(MAX(pos), -1) + 1 FROM list_recipes WHERE list = ?1",
            [list],
            |r| r.get(0),
        )?;
        for s in &built.sources {
            let path = s.path.to_string_lossy();
            let merged = tx.execute(
                "UPDATE list_recipes
                 SET factor = factor + ?3,
                     servings = CASE WHEN servings IS NULL OR ?4 IS NULL THEN NULL ELSE servings + ?4 END
                 WHERE list = ?1 AND path = ?2",
                params![list, path, s.factor, s.servings],
            )?;
            if merged == 0 {
                tx.execute(
                    "INSERT INTO list_recipes (list, pos, path, title, servings, factor)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![list, next, path, s.title, s.servings, s.factor],
                )?;
                next += 1;
            }
        }
        let mut pantry = current.pantry_skipped;
        for p in &built.pantry_skipped {
            if !pantry.contains(p) {
                pantry.push(p.clone());
            }
        }
        tx.execute(
            "UPDATE lists SET pantry = ?2 WHERE id = ?1",
            params![list, join_lines(&pantry)],
        )?;
        resort(&tx, list, db.aisle_order())?;
        tx.commit()?;
        Ok(true)
    }

    /// Tick an item on or off. Returns `false` if the item isn't on that list.
    pub fn set_checked(&self, list: i64, item: i64, checked: bool) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE list_items SET checked = ?3 WHERE list = ?1 AND id = ?2",
            params![list, item, checked],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_item(&self, list: i64, item: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "DELETE FROM list_items WHERE list = ?1 AND id = ?2",
            params![list, item],
        )?;
        Ok(changed > 0)
    }
}

fn list_exists(conn: &rusqlite::Connection, list: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM lists WHERE id = ?1)",
        [list],
        |r| r.get(0),
    )?)
}

/// Put a list's items back in walking order (see [`aggregate::sort_key`]).
fn resort(conn: &rusqlite::Connection, list: i64, aisle_order: &[String]) -> Result<()> {
    let mut rows: Vec<(i64, Item)> = conn
        .prepare("SELECT id, name, aisle FROM list_items WHERE list = ?1 ORDER BY pos, id")?
        .query_map([list], |r| {
            Ok((
                r.get(0)?,
                Item {
                    name: r.get(1)?,
                    written: vec![],
                    aisle: r.get(2)?,
                    quantity: None,
                    notes: vec![],
                    optional: false,
                    recipes: vec![],
                    known: false,
                    checked: false,
                },
            ))
        })?
        .collect::<Result<_, _>>()?;
    // Stable: lines of the same food keep their order.
    rows.sort_by_cached_key(|(_, item)| aggregate::sort_key(item, aisle_order));
    let mut set = conn.prepare("UPDATE list_items SET pos = ?2 WHERE id = ?1")?;
    for (pos, (id, _)) in rows.iter().enumerate() {
        set.execute(params![id, pos as i64])?;
    }
    Ok(())
}

fn update_item(conn: &rusqlite::Connection, id: i64, item: &Item) -> Result<()> {
    let q = item.quantity.as_ref();
    conn.execute(
        "UPDATE list_items SET aisle = ?2, dim = ?3, value = ?4, label = ?5, notes = ?6,
                optional = ?7, recipes = ?8, known = ?9, checked = ?10, written = ?11
         WHERE id = ?1",
        params![
            id,
            item.aisle,
            q.map(|q| dim_to_sql(&q.dim)),
            q.map(|q| q.value),
            q.and_then(|q| q.label.clone()),
            join_lines(&item.notes),
            item.optional,
            join_lines(&item.recipes),
            item.known,
            item.checked,
            join_lines(&item.written)
        ],
    )?;
    Ok(())
}

fn insert_item(conn: &rusqlite::Connection, list: i64, pos: i64, item: &Item) -> Result<i64> {
    let q = item.quantity.as_ref();
    conn.execute(
        "INSERT INTO list_items
         (list, pos, name, aisle, dim, value, label, notes, optional, recipes, known, checked,
          written)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            list,
            pos,
            item.name,
            item.aisle,
            q.map(|q| dim_to_sql(&q.dim)),
            q.map(|q| q.value),
            q.and_then(|q| q.label.clone()),
            join_lines(&item.notes),
            item.optional,
            join_lines(&item.recipes),
            item.known,
            item.checked,
            join_lines(&item.written)
        ],
    )?;
    Ok(conn.last_insert_rowid())
}
