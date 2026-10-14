//! Meal plans: per person, which recipes they eat on which day, and how
//! many portions. Nothing here is derived from the library, so like the
//! lists it's what a backup is for.
//!
//! An entry keeps the recipe's title as well as its path, so a plan still
//! reads sensibly after a recipe is renamed or removed.

use chrono::NaiveDate;
use rusqlite::{params, OptionalExtension, Row};

use crate::{Error, Result, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Meal {
    Breakfast,
    Lunch,
    Dinner,
    Snack,
}

impl Meal {
    pub const ALL: [Meal; 4] = [Meal::Breakfast, Meal::Lunch, Meal::Dinner, Meal::Snack];

    pub fn as_str(self) -> &'static str {
        match self {
            Meal::Breakfast => "breakfast",
            Meal::Lunch => "lunch",
            Meal::Dinner => "dinner",
            Meal::Snack => "snack",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    fn sort_key(meal: Option<Meal>) -> i64 {
        meal.map_or(4, |m| m as i64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanEntry {
    pub id: i64,
    pub person: i64,
    pub day: NaiveDate,
    pub meal: Option<Meal>,
    /// Relative to `Recipes/`.
    pub path: String,
    pub title: String,
    /// Portions this person eats (servings as the recipe counts them).
    pub portions: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub person: i64,
    pub day: NaiveDate,
    pub meal: Option<Meal>,
    pub path: String,
    pub title: String,
    pub portions: f64,
}

/// A change to an entry; `None` fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntryChange {
    pub day: Option<NaiveDate>,
    /// `Some(None)` clears the meal.
    pub meal: Option<Option<Meal>>,
    pub portions: Option<f64>,
}

pub fn parse_day(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| Error::Invalid(format!("'{s}' is not a date (YYYY-MM-DD)")))
}

fn check_portions(p: f64) -> Result<()> {
    if p.is_finite() && p > 0.0 && p <= 100.0 {
        Ok(())
    } else {
        Err(Error::Invalid(
            "portions must be more than 0 (and at most 100)".into(),
        ))
    }
}

fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() || name.contains('\n') || name.chars().count() > 40 {
        return Err(Error::Invalid(
            "a name is one line of at most 40 characters".into(),
        ));
    }
    Ok(name)
}

const ENTRY_COLUMNS: &str = "id, person, day, meal, path, title, portions";

fn entry_from_row(r: &Row) -> rusqlite::Result<PlanEntry> {
    let day: String = r.get(2)?;
    let meal: Option<String> = r.get(3)?;
    Ok(PlanEntry {
        id: r.get(0)?,
        person: r.get(1)?,
        // Only valid dates are written.
        day: NaiveDate::parse_from_str(&day, "%Y-%m-%d").unwrap_or_default(),
        meal: meal.as_deref().and_then(Meal::parse),
        path: r.get(4)?,
        title: r.get(5)?,
        portions: r.get(6)?,
    })
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _)
        if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE)
}

impl Store {
    pub fn people(&self) -> Result<Vec<Person>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM people ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Person {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_person(&self, name: &str) -> Result<Person> {
        let name = check_name(name)?;
        match self
            .conn
            .execute("INSERT INTO people (name) VALUES (?1)", [name])
        {
            Ok(_) => Ok(Person {
                id: self.conn.last_insert_rowid(),
                name: name.to_owned(),
            }),
            Err(e) if is_unique_violation(&e) => Err(Error::Invalid(format!(
                "there's already someone called '{name}'"
            ))),
            Err(e) => Err(e.into()),
        }
    }

    /// `false` if there's no such person.
    pub fn rename_person(&self, id: i64, name: &str) -> Result<bool> {
        let name = check_name(name)?;
        match self.conn.execute(
            "UPDATE people SET name = ?2 WHERE id = ?1",
            params![id, name],
        ) {
            Ok(n) => Ok(n > 0),
            Err(e) if is_unique_violation(&e) => Err(Error::Invalid(format!(
                "there's already someone called '{name}'"
            ))),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes their plan too.
    pub fn delete_person(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM people WHERE id = ?1", [id])?
            > 0)
    }

    /// Entries from `from` to `to` (inclusive), by day and meal.
    pub fn plan(&self, person: i64, from: NaiveDate, to: NaiveDate) -> Result<Vec<PlanEntry>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS} FROM plan_entries
             WHERE person = ?1 AND day BETWEEN ?2 AND ?3
             ORDER BY day, meal_order, id"
        ))?;
        let rows = stmt.query_map(
            params![person, from.to_string(), to.to_string()],
            entry_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn plan_entry(&self, id: i64) -> Result<Option<PlanEntry>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ENTRY_COLUMNS} FROM plan_entries WHERE id = ?1"),
                [id],
                entry_from_row,
            )
            .optional()?)
    }

    /// `None` if there's no such person.
    pub fn add_plan_entry(&self, entry: &NewEntry) -> Result<Option<PlanEntry>> {
        check_portions(entry.portions)?;
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM people WHERE id = ?1)",
            [entry.person],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        self.conn.execute(
            "INSERT INTO plan_entries (person, day, meal, meal_order, path, title, portions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.person,
                entry.day.to_string(),
                entry.meal.map(Meal::as_str),
                Meal::sort_key(entry.meal),
                entry.path,
                entry.title,
                entry.portions
            ],
        )?;
        self.plan_entry(self.conn.last_insert_rowid())
    }

    /// The changed entry, or `None` if there's no such entry.
    pub fn update_plan_entry(&self, id: i64, change: &EntryChange) -> Result<Option<PlanEntry>> {
        let Some(mut entry) = self.plan_entry(id)? else {
            return Ok(None);
        };
        if let Some(p) = change.portions {
            check_portions(p)?;
            entry.portions = p;
        }
        if let Some(day) = change.day {
            entry.day = day;
        }
        if let Some(meal) = change.meal {
            entry.meal = meal;
        }
        self.conn.execute(
            "UPDATE plan_entries SET day = ?2, meal = ?3, meal_order = ?4, portions = ?5 WHERE id = ?1",
            params![
                id,
                entry.day.to_string(),
                entry.meal.map(Meal::as_str),
                Meal::sort_key(entry.meal),
                entry.portions
            ],
        )?;
        Ok(Some(entry))
    }

    pub fn delete_plan_entry(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM plan_entries WHERE id = ?1", [id])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        parse_day(s).unwrap()
    }

    fn entry(person: i64, d: &str, meal: Option<Meal>, title: &str) -> NewEntry {
        NewEntry {
            person,
            day: day(d),
            meal,
            path: format!("{title}.cook"),
            title: title.into(),
            portions: 1.0,
        }
    }

    #[test]
    fn people_and_their_plans() {
        let store = Store::open_in_memory().unwrap();
        let me = store.add_person(" Peter ").unwrap();
        assert_eq!(me.name, "Peter");
        let them = store.add_person("Sam").unwrap();
        assert!(store.add_person("peter").is_err(), "names are unique");
        assert!(store.add_person("").is_err());

        let dinner = store
            .add_plan_entry(&entry(me.id, "2026-10-14", Some(Meal::Dinner), "Curry"))
            .unwrap()
            .unwrap();
        store
            .add_plan_entry(&entry(me.id, "2026-10-14", Some(Meal::Breakfast), "Kwark"))
            .unwrap();
        store
            .add_plan_entry(&entry(me.id, "2026-10-15", None, "Paella"))
            .unwrap();
        store
            .add_plan_entry(&entry(them.id, "2026-10-14", None, "Paella"))
            .unwrap();
        assert!(store
            .add_plan_entry(&entry(99, "2026-10-14", None, "x"))
            .unwrap()
            .is_none());

        let mine = store
            .plan(me.id, day("2026-10-14"), day("2026-10-14"))
            .unwrap();
        let titles: Vec<_> = mine.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Kwark", "Curry"], "by meal within a day");
        assert_eq!(
            store
                .plan(me.id, day("2026-10-12"), day("2026-10-18"))
                .unwrap()
                .len(),
            3
        );

        let changed = store
            .update_plan_entry(
                dinner.id,
                &EntryChange {
                    portions: Some(1.5),
                    meal: Some(None),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!((changed.portions, changed.meal), (1.5, None));
        assert!(store
            .update_plan_entry(
                dinner.id,
                &EntryChange {
                    portions: Some(0.0),
                    ..Default::default()
                }
            )
            .is_err());

        assert!(store.delete_plan_entry(dinner.id).unwrap());
        assert!(!store.delete_plan_entry(dinner.id).unwrap());

        // Deleting a person deletes their plan.
        assert!(store.delete_person(them.id).unwrap());
        assert!(store
            .plan(them.id, day("2026-01-01"), day("2026-12-31"))
            .unwrap()
            .is_empty());
        assert!(store.rename_person(me.id, "P").unwrap());
        assert_eq!(
            store.people().unwrap(),
            [Person {
                id: me.id,
                name: "P".into()
            }]
        );
    }

    #[test]
    fn days_must_be_dates() {
        assert!(parse_day("2026-02-30").is_err());
        assert!(parse_day("14-10-2026").is_err());
        assert_eq!(parse_day("2026-10-14").unwrap().to_string(), "2026-10-14");
    }
}
