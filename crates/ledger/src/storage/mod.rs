use crate::transaction::{Transaction, Utxo, UtxoId};
use crate::{FullAccount, Reference};

use super::Amount;

mod memory;

pub use memory::Memory;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Missing utxo {0:?}")]
    MissingUtxo(UtxoId),

    #[error("Spent utxo {0:?}")]
    SpentUtxo(UtxoId),

    #[error("Mismatch amount between the stored utxo and the tx utxo")]
    MismatchAmount,

    #[error("Math error")]
    Math,

    #[error("Duplicate")]
    Duplicate,

    #[error("Error internal")]
    Internal,
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

    /// Get transactions by Reference
    async fn get_tx_by_reference(
        &self,
        account: &FullAccount,
        reference: &Reference,
    ) -> Result<Option<Transaction>, Error>;

    /// Stores a transaction
    ///
    /// It is important that correctness is kept at all time. For instance if a input UTXO is
    /// already spent, that this function fails.
    ///
    /// In the same transaction the transaction is stored and the input UTXO are set as spent. The
    /// entire operations succeeds or it is rollback
    ///
    /// References are unique per account as has to be enforced
    async fn store_tx(&self, tx: Transaction) -> Result<(), Error>;
}
