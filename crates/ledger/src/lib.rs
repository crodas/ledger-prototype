//! This is meant to be a generic simple ledger to record transaction movements

mod account;
mod amount;
mod storage;
mod transaction;

use std::sync::Arc;

use storage::{Memory, Storage};
use transaction::{HashId, Transaction};

pub use self::{
    account::{FullAccount, Id as AccountId, Type as AccountType},
    amount::Amount,
};

pub type Reference = String;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Tx(#[from] transaction::Error),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error("Not enough in account")]
    NotEnough,

    #[error("Overflow or underflow error")]
    Math,
}

/// Very simple UTXO based ledger, a simplified version of my own ledger prototype that someday I
/// will make it open source and will be promoted to my Github
///
/// https://git.cesar.com.py/cesar/ledger-prototype
#[derive(Debug, Clone)]
pub struct Ledger<S>
where
    S: Storage,
{
    storage: Arc<S>, // TODO: implement
}

impl Default for Ledger<Memory> {
    fn default() -> Self {
        Ledger {
            storage: Arc::new(Memory::default()),
        }
    }
}

impl<S> Ledger<S>
where
    S: Storage,
{
    pub fn new(storage: S) -> Self {
        Ledger {
            storage: Arc::new(storage),
        }
    }

    pub async fn deposit(
        &mut self,
        account: AccountId,
        reference: Reference,
        amount: Amount,
    ) -> Result<HashId, Error> {
        let new_tx = Transaction::new(vec![], vec![(account.into(), amount)], reference, None)?;
        let tx_id = new_tx.id();
        self.storage.store_tx(new_tx).await?;
        Ok(tx_id)
    }

    pub async fn withdraw(
        &mut self,
        account: AccountId,
        reference: Reference,
        amount: Amount,
    ) -> Result<HashId, Error> {
        let inputs = self
            .storage
            .get_unspent(&account.into(), Some(amount))
            .await?;

        let total: i128 = inputs.iter().map(|x| *x.amount()).sum();
        let change = if total < *amount {
            return Err(Error::NotEnough);
        } else if total > *amount {
            vec![(
                account.into(),
                (total.checked_sub(*amount).ok_or(Error::Math)?.into()),
            )]
        } else {
            vec![]
        };

        let new_tx = Transaction::new(inputs, change, reference, None)?;
        let id = new_tx.id();

        self.storage.store_tx(new_tx).await?;

        Ok(id)
    }

    pub fn movement(&mut self, from: AccountId, to: AccountId, amount: Amount) {
        todo!()
    }
}
