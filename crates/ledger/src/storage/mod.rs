use crate::FullAccount;
use crate::transaction::{Transaction, Utxo, UtxoId};

use super::Amount;

mod memory;

pub use memory::Memory;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Missing utxo {0:?}")]
    MissingUtxo(UtxoId),

    #[error("Math error")]
    Math,
}

/// Extremely simple storage layer
///
/// All math is not done, and its sole responsabilities are storage, durability and correctness.
#[async_trait::async_trait]
pub trait Storage {
    /// Get unspent UTXO for this given account. Optionally it be capped to cover a target_amount.
    ///
    /// If
    ///
    /// This function is used to request balance or to see how much is spendable. All math is
    /// avoided in the storage layer, it is good to keep it as dumb as possible, with one
    /// responsability, storage and correctness.
    async fn get_unspent(
        &self,
        account: &FullAccount,
        target_amount: Option<Amount>,
    ) -> Result<Vec<Utxo>, Error>;

    /// Stores a transaction
    ///
    /// It is important that correctness is kept at all time. For instance if a input UTXO is
    /// already spent, that this function fails.
    ///
    /// In the same transaction the transaction is stored and the input UTXO are set as spent. The
    /// entire operations succeeds or it is rollback
    async fn store_tx(&self, tx: Transaction) -> Result<(), Error>;
}
