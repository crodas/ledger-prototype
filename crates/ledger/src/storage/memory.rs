//! In memory implementation to show that I know how DB works internally.
use crate::{FullAccount, Reference, transaction::UtxoId};

use futures::Stream;
use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    task::Poll,
};

use crate::{
    Amount,
    transaction::{HashId, Transaction, Utxo},
};

use super::{Error, Storage};

#[derive(Debug, Clone, Copy)]
struct UtxoInMemory {
    amount: Amount,
    spent_at: Option<HashId>,
}

#[derive(Debug, Default)]
struct InMemoryStorage {
    utxo: HashMap<UtxoId, UtxoInMemory>,
    utxo_by_account: HashMap<FullAccount, VecDeque<UtxoId>>,
    txs_by_account: BTreeMap<FullAccount, VecDeque<HashId>>,
    txs_by_reference: HashMap<(FullAccount, Reference), HashId>,
    txs: HashMap<HashId, Transaction>,
}

#[derive(Debug, Default)]
pub struct Memory {
    inner: Arc<RwLock<InMemoryStorage>>,
}

pub struct AccountStream {
    inner: Arc<RwLock<InMemoryStorage>>,
    latest: Option<FullAccount>,
}

impl Stream for AccountStream {
    type Item = Result<FullAccount, Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let inner = this.inner.read();

        if let Some(latest) = this.latest.take() {
            for (next, _) in inner.txs_by_account.range(latest..) {
                if *next != latest {
                    this.latest = Some(*next);
                    return Poll::Ready(Some(Ok(*next)));
                }
            }
        } else if let Some((next, _)) = inner.txs_by_account.iter().next() {
            this.latest = Some(*next);
            return Poll::Ready(Some(Ok(*next)));
        }

        Poll::Ready(None)
    }
}

#[async_trait::async_trait]
impl Storage for Memory {
    async fn get_accounts(&self) -> AccountStream {
        AccountStream {
            inner: self.inner.clone(),
            latest: None,
        }
    }

    async fn get_unspent(
        &self,
        account: &FullAccount,
        target_amount: Option<Amount>,
    ) -> Result<Vec<Utxo>, Error> {
        let inner = self.inner.read();

        let utxos_for_account = if let Some(utxos) = inner.utxo_by_account.get(account) {
            utxos
        } else {
            return Ok(Vec::new());
        };

        let mut result = Vec::new();
        let mut total = 0i128;

        for utxo_id in utxos_for_account {
            let info = inner
                .utxo
                .get(utxo_id)
                .ok_or(Error::MissingUtxo(*utxo_id))?;

            if info.spent_at.is_some() {
                // We already reached the end, as the store_tx will put the stored utxo to the end
                break;
            }

            result.push(Utxo::new(*utxo_id, info.amount));
            if let Some(target_amount) = target_amount {
                // We already have enough UTXO to fullfill the request
                total = total.checked_add(*info.amount).ok_or(Error::Math)?;
                if *target_amount <= total {
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
        let inner = self.inner.read();

        let tx_id = if let Some(tx_id) = inner.txs_by_reference.get(&(*account, reference.clone()))
        {
            tx_id
        } else {
            return Ok(None);
        };

        Ok(Some(inner.txs.get(tx_id).ok_or(Error::Internal)?.clone()))
    }

    async fn store_tx(&self, tx: Transaction) -> Result<(), Error> {
        let mut inner = self.inner.write();

        let tx_id = tx.id();

        // Is it a duplicate tx?
        if inner.txs.contains_key(&tx_id) {
            return Err(Error::Duplicate);
        }

        for (account, _) in tx.outputs().iter() {
            if inner
                .txs_by_reference
                .contains_key(&(*account, tx.reference()))
            {
                return Err(Error::Duplicate);
            }
        }

        // check all the utxo are indeed unspent
        for input in tx.inputs() {
            let in_memory_utxo = if let Some(utxo) = inner.utxo.get(&input.id()) {
                utxo
            } else {
                return Err(Error::MissingUtxo(input.id()));
            };

            if in_memory_utxo.spent_at.is_some() {
                return Err(Error::SpentUtxo(input.id()));
            }

            if in_memory_utxo.amount != input.amount() {
                return Err(Error::MismatchAmount);
            }
        }

        // All check passed, now do the persistence
        inner.txs.insert(tx_id, tx.clone());

        // mark the input utxo as spent by this transaction
        for input in tx.inputs() {
            let in_memory_utxo = if let Some(utxo) = inner.utxo.get_mut(&input.id()) {
                utxo
            } else {
                unreachable!();
            };
            in_memory_utxo.spent_at = Some(tx_id);
        }

        // create the new utxo
        for (pos, (account, amount)) in tx.outputs().iter().enumerate() {
            inner
                .txs_by_account
                .entry(*account)
                .or_default()
                .push_front(tx_id);

            inner
                .txs_by_reference
                .insert((*account, tx.reference()), tx_id);

            let pos = pos.try_into().map_err(|_| Error::Math)?;
            let utxo_id = (tx_id, pos).into();

            // store the new utxo
            inner.utxo.insert(
                utxo_id,
                UtxoInMemory {
                    amount: *amount,
                    spent_at: None,
                },
            );
            // add the utxo to the account
            inner
                .utxo_by_account
                .entry(*account)
                .or_default()
                .push_front(utxo_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::storage_test!(Memory::default());
}
