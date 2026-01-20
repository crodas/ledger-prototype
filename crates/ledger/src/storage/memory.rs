//! In memory implementation to show that I know how DB works internally.
use crate::{FullAccount, transaction::UtxoId};

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};

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
    txs: HashMap<FullAccount, VecDeque<Transaction>>,
    all_txs: HashSet<HashId>,
}

#[derive(Debug, Default)]
pub struct Memory {
    inner: RwLock<InMemoryStorage>,
}

#[async_trait::async_trait]
impl Storage for Memory {
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
                // We already reached the end, as the store_tx will put the stored utox to the end
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

    async fn store_tx(&self, tx: Transaction) -> Result<(), Error> {
        let mut inner = self.inner.write();

        let tx_id = tx.id();

        // Is it a duplicate tx?
        if inner.all_txs.contains(&tx_id) {
            return Err(Error::Duplicate);
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

        // All check passed, now do the persitance
        inner.all_txs.insert(tx_id);

        // mark the input utxo as spent by this transaction
        for input in tx.inputs() {
            let in_memory_utxo = if let Some(utxo) = inner.utxo.get_mut(&input.id()) {
                utxo
            } else {
                unreachable!();
            };
            in_memory_utxo.spent_at = Some(tx_id);
        }

        // create the new utox
        for (pos, (account, amount)) in tx.outputs().iter().enumerate() {
            inner
                .txs
                .entry(*account)
                .or_default()
                .push_front(tx.clone());
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
