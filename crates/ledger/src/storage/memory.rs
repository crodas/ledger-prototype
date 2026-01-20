//! In memory implementation to show that I know how DB works internally.
use crate::{FullAccount, transaction::UtxoId};

use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};

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
        todo!()
    }
}
