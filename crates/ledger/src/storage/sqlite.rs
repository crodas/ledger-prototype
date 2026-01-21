//! SQLite implementation of the Storage trait.
use crate::transaction::{HashId, Transaction, Utxo, UtxoId};
use crate::{Amount, FullAccount, Reference};

use futures::Stream;
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;
use std::task::Poll;

use super::{Error, Storage};

/// SQLite-backed storage implementation.
///
/// Uses an in-memory SQLite database by default, but can be configured to use a file-based
/// database for persistence.
pub struct Sqlite {
    conn: Arc<Mutex<Connection>>,
}

impl Default for Sqlite {
    fn default() -> Self {
        Self::in_memory().expect("failed to create in-memory SQLite database")
    }
}

impl Sqlite {
    /// Creates a new in-memory SQLite storage.
    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        Self::with_connection(conn)
    }

    /// Creates a new file-backed SQLite storage.
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        Self::with_connection(conn)
    }

    fn with_connection(conn: Connection) -> Result<Self, rusqlite::Error> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS transactions (
                tx_id BLOB PRIMARY KEY,
                tx_data BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS utxos (
                hash_id BLOB NOT NULL,
                pos INTEGER NOT NULL,
                account_id INTEGER NOT NULL,
                account_type INTEGER NOT NULL,
                amount INTEGER NOT NULL,
                spent_at BLOB,
                PRIMARY KEY (hash_id, pos)
            );

            CREATE INDEX IF NOT EXISTS idx_utxos_account
                ON utxos (account_id, account_type);

            CREATE TABLE IF NOT EXISTS tx_references (
                account_id INTEGER NOT NULL,
                account_type INTEGER NOT NULL,
                reference TEXT NOT NULL,
                tx_id BLOB NOT NULL,
                PRIMARY KEY (account_id, account_type, reference)
            );

            CREATE TABLE IF NOT EXISTS accounts (
                account_id INTEGER NOT NULL,
                account_type INTEGER NOT NULL,
                PRIMARY KEY (account_id, account_type)
            );
            ",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn account_type_to_int(typ: crate::account::Type) -> i64 {
        typ.to_byte() as i64
    }

    fn int_to_account_type(val: i64) -> crate::account::Type {
        match val {
            0 => crate::account::Type::Main,
            1 => crate::account::Type::Disputed,
            2 => crate::account::Type::Chargeback,
            _ => crate::account::Type::Main,
        }
    }
}

/// Stream for iterating over accounts in sorted order.
pub struct AccountStream {
    conn: Arc<Mutex<Connection>>,
    offset: usize,
}

impl Stream for AccountStream {
    type Item = Result<FullAccount, Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let conn = this.conn.lock();

        let result: Result<Option<(i64, i64)>, rusqlite::Error> = conn
            .query_row(
                "SELECT account_id, account_type FROM accounts
                 ORDER BY account_id, account_type
                 LIMIT 1 OFFSET ?",
                params![this.offset as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional();

        match result {
            Ok(Some((account_id, account_type))) => {
                this.offset += 1;
                let account: FullAccount =
                    (account_id as u16, Sqlite::int_to_account_type(account_type)).into();
                Poll::Ready(Some(Ok(account)))
            }
            Ok(None) => Poll::Ready(None),
            Err(_) => Poll::Ready(Some(Err(Error::Internal))),
        }
    }
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[async_trait::async_trait]
impl Storage for Sqlite {
    async fn get_accounts(&self) -> AccountStream {
        AccountStream {
            conn: self.conn.clone(),
            offset: 0,
        }
    }

    async fn get_unspent(
        &self,
        account: &FullAccount,
        target_amount: Option<Amount>,
    ) -> Result<Vec<Utxo>, Error> {
        let conn = self.conn.lock();

        let mut stmt = conn
            .prepare(
                "SELECT hash_id, pos, amount FROM utxos
                 WHERE account_id = ? AND account_type = ? AND spent_at IS NULL
                 ORDER BY rowid",
            )
            .map_err(|_| Error::Internal)?;

        let account_id = account.id() as i64;
        let account_type = Self::account_type_to_int(account.typ());

        let rows = stmt
            .query_map(params![account_id, account_type], |row| {
                let hash_id: Vec<u8> = row.get(0)?;
                let pos: i64 = row.get(1)?;
                let amount: i64 = row.get(2)?;
                Ok((hash_id, pos, amount))
            })
            .map_err(|_| Error::Internal)?;

        let mut result = Vec::new();
        let mut total: i128 = 0;

        for row in rows {
            let (hash_id, pos, amount) = row.map_err(|_| Error::Internal)?;
            let hash_id: HashId = hash_id
                .try_into()
                .map_err(|_| Error::Internal)?;
            let utxo_id: UtxoId = (hash_id, pos as u8).into();
            let amount = Amount::from(amount as i128);

            result.push(Utxo::new(utxo_id, amount));

            if let Some(target) = target_amount {
                total = total.checked_add(*amount).ok_or(Error::Math)?;
                if *target <= total {
                    break;
                }
            }
        }

        Ok(result)
    }

    async fn get_tx_by_reference(
        &self,
        account: &FullAccount,
        reference: &Reference,
    ) -> Result<Option<Transaction>, Error> {
        let conn = self.conn.lock();

        let account_id = account.id() as i64;
        let account_type = Self::account_type_to_int(account.typ());

        let tx_id: Option<Vec<u8>> = conn
            .query_row(
                "SELECT tx_id FROM tx_references
                 WHERE account_id = ? AND account_type = ? AND reference = ?",
                params![account_id, account_type, reference],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| Error::Internal)?;

        let tx_id = match tx_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let tx_data: Vec<u8> = conn
            .query_row(
                "SELECT tx_data FROM transactions WHERE tx_id = ?",
                params![tx_id],
                |row| row.get(0),
            )
            .map_err(|_| Error::Internal)?;

        let tx: Transaction = serde_json::from_slice(&tx_data).map_err(|_| Error::Internal)?;
        Ok(Some(tx))
    }

    async fn store_tx(&self, tx: Transaction) -> Result<(), Error> {
        let mut conn = self.conn.lock();

        let tx_id = tx.id();
        let tx_id_bytes = tx_id.as_slice();

        // Check for duplicate transaction
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM transactions WHERE tx_id = ?",
                params![tx_id_bytes],
                |_| Ok(true),
            )
            .optional()
            .map_err(|_| Error::Internal)?
            .unwrap_or(false);

        if exists {
            return Err(Error::Duplicate);
        }

        // Check for duplicate references
        for (account, _) in tx.outputs().iter() {
            let account_id = account.id() as i64;
            let account_type = Self::account_type_to_int(account.typ());

            let ref_exists: bool = conn
                .query_row(
                    "SELECT 1 FROM tx_references
                     WHERE account_id = ? AND account_type = ? AND reference = ?",
                    params![account_id, account_type, tx.reference()],
                    |_| Ok(true),
                )
                .optional()
                .map_err(|_| Error::Internal)?
                .unwrap_or(false);

            if ref_exists {
                return Err(Error::Duplicate);
            }
        }

        // Verify all input UTXOs exist and are unspent
        for input in tx.inputs() {
            let utxo_id = input.id();
            let (hash_id, pos) = (utxo_id.hash_id(), utxo_id.pos());

            let utxo_info: Option<(i64, Option<Vec<u8>>)> = conn
                .query_row(
                    "SELECT amount, spent_at FROM utxos WHERE hash_id = ? AND pos = ?",
                    params![hash_id.as_slice(), pos as i64],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|_| Error::Internal)?;

            match utxo_info {
                None => return Err(Error::MissingUtxo(utxo_id)),
                Some((_, Some(_))) => return Err(Error::SpentUtxo(utxo_id)),
                Some((stored_amount, None)) => {
                    if stored_amount != *input.amount() as i64 {
                        return Err(Error::MismatchAmount);
                    }
                }
            }
        }

        // All checks passed, begin transaction
        let sql_tx = conn.transaction().map_err(|_| Error::Internal)?;

        // Store the transaction
        let tx_data = serde_json::to_vec(&tx).map_err(|_| Error::Internal)?;
        sql_tx
            .execute(
                "INSERT INTO transactions (tx_id, tx_data) VALUES (?, ?)",
                params![tx_id_bytes, tx_data],
            )
            .map_err(|_| Error::Internal)?;

        // Mark input UTXOs as spent
        for input in tx.inputs() {
            let utxo_id = input.id();
            let (hash_id, pos) = (utxo_id.hash_id(), utxo_id.pos());

            sql_tx
                .execute(
                    "UPDATE utxos SET spent_at = ? WHERE hash_id = ? AND pos = ?",
                    params![tx_id_bytes, hash_id.as_slice(), pos as i64],
                )
                .map_err(|_| Error::Internal)?;
        }

        // Create new UTXOs and update references
        for (pos, (account, amount)) in tx.outputs().iter().enumerate() {
            let account_id = account.id() as i64;
            let account_type = Self::account_type_to_int(account.typ());
            let pos = pos as i64;

            // Insert new UTXO
            sql_tx
                .execute(
                    "INSERT INTO utxos (hash_id, pos, account_id, account_type, amount, spent_at)
                     VALUES (?, ?, ?, ?, ?, NULL)",
                    params![tx_id_bytes, pos, account_id, account_type, **amount as i64],
                )
                .map_err(|_| Error::Internal)?;

            // Insert reference
            sql_tx
                .execute(
                    "INSERT INTO tx_references (account_id, account_type, reference, tx_id)
                     VALUES (?, ?, ?, ?)",
                    params![account_id, account_type, tx.reference(), tx_id_bytes],
                )
                .map_err(|_| Error::Internal)?;

            // Track account
            sql_tx
                .execute(
                    "INSERT OR IGNORE INTO accounts (account_id, account_type) VALUES (?, ?)",
                    params![account_id, account_type],
                )
                .map_err(|_| Error::Internal)?;
        }

        sql_tx.commit().map_err(|_| Error::Internal)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::storage_test!(Sqlite::default());
}
