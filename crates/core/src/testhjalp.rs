//! Hjälpmedel för tester i andra crates (feature `test-support`).

use crate::Databas;
use crate::aktor::{Aktör, AnvändarId};

/// En ny användare i webben, utan passkey och utan åtkomst till något företag. Inbjudningar
/// kommer i ett senare ticket; tills dess är det här vägen till en andra användare i tester.
pub async fn användare(db: &Databas, epost: &str) -> Aktör {
    let mut handle = [0u8; 16];
    getrandom::fill(&mut handle).expect("slumptal från operativsystemet");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO anvandare (handle, epost, visningsnamn, skapad)
         VALUES (?, ?, ?, '2026-01-01T00:00:00Z') RETURNING id",
    )
    .bind(&handle[..])
    .bind(epost)
    .bind(epost)
    .fetch_one(db.pool())
    .await
    .expect("testanvändare");
    Aktör::webb(AnvändarId(id))
}

/// Kör en SQL-sats direkt mot databasen, förbi `core`, för att visa att schemat avvisar den.
pub async fn kör_sql(db: &Databas, sql: &'static str) -> Result<(), sqlx::Error> {
    sqlx::query(sql).execute(db.pool()).await.map(|_| ())
}
