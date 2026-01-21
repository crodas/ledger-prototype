//! In memory implementation to show that I know how DB works internally.
use crate::{FullAccount, Reference, transaction::UtxoId};

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
    txs_by_account: HashMap<FullAccount, VecDeque<HashId>>,
    txs_by_reference: HashMap<(FullAccount, Reference), HashId>,
    txs: HashMap<HashId, Transaction>,
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

        // All check passed, now do the persitance
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

        // create the new utox
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
    use crate::AccountId;

    use super::*;

    fn make_account(id: AccountId) -> FullAccount {
        id.into()
    }

    fn make_deposit_tx(
        account: FullAccount,
        amount: Amount,
        reference: &str,
        timestamp: u64,
    ) -> Transaction {
        Transaction::new(
            vec![],
            vec![(account, amount)],
            reference.to_string(),
            Some(timestamp),
        )
        .expect("deposit transaction should be valid")
    }

    fn make_utxo(tx_id: HashId, pos: u8, amount: Amount) -> Utxo {
        Utxo::new((tx_id, pos).into(), amount)
    }

    #[tokio::test]
    async fn test_get_unspent_empty_account() {
        let storage = Memory::default();
        let account = make_account(1);

        let result = storage
            .get_unspent(&account, None)
            .await
            .expect("get_unspent should succeed for empty account");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_store_and_get_unspent() {
        let storage = Memory::default();
        let account = make_account(1);
        let amount: Amount = 100.into();

        let tx = make_deposit_tx(account, amount, "deposit-1", 1000);
        storage
            .store_tx(tx.clone())
            .await
            .expect("store_tx should succeed for valid deposit");

        let unspent = storage
            .get_unspent(&account, None)
            .await
            .expect("get_unspent should succeed after deposit");
        assert_eq!(unspent.len(), 1);
        assert_eq!(unspent[0].amount(), amount);
    }

    #[tokio::test]
    async fn test_duplicate_transaction_rejected() {
        let storage = Memory::default();
        let account = make_account(1);
        let amount: Amount = 100.into();

        let tx = make_deposit_tx(account, amount, "deposit-1", 1000);
        storage
            .store_tx(tx.clone())
            .await
            .expect("first store_tx should succeed");

        let result = storage.store_tx(tx).await;
        assert!(matches!(result, Err(Error::Duplicate)));
    }

    #[tokio::test]
    async fn test_spent_utxo_cannot_be_spent_twice() {
        let storage = Memory::default();
        let account = make_account(1);
        let amount: Amount = 100.into();

        // Create initial deposit
        let deposit_tx = make_deposit_tx(account, amount, "deposit-1", 1000);
        let deposit_id = deposit_tx.id();
        storage
            .store_tx(deposit_tx)
            .await
            .expect("deposit should succeed");

        // Spend the UTXO
        let utxo = make_utxo(deposit_id, 0, amount);
        let spend_tx = Transaction::new(
            vec![utxo],
            vec![(account, amount)],
            "spend-1".to_string(),
            Some(2000),
        )
        .expect("spend transaction should be valid");
        storage
            .store_tx(spend_tx)
            .await
            .expect("first spend should succeed");

        // Try to spend the same UTXO again
        let utxo_again = make_utxo(deposit_id, 0, amount);
        let double_spend_tx = Transaction::new(
            vec![utxo_again],
            vec![(account, amount)],
            "spend-2".to_string(),
            Some(3000),
        )
        .expect("double spend transaction should be valid structurally");
        let result = storage.store_tx(double_spend_tx).await;
        assert!(matches!(result, Err(Error::SpentUtxo(_))));
    }

    #[tokio::test]
    async fn test_missing_utxo_error() {
        let storage = Memory::default();
        let account = make_account(1);
        let amount: Amount = 100.into();

        // Try to spend a UTXO that doesn't exist
        let fake_tx_id = [0u8; 32];
        let utxo = make_utxo(fake_tx_id, 0, amount);
        let tx = Transaction::new(
            vec![utxo],
            vec![(account, amount)],
            "spend-1".to_string(),
            Some(1000),
        )
        .expect("transaction with fake utxo should be valid structurally");

        let result = storage.store_tx(tx).await;
        assert!(matches!(result, Err(Error::MissingUtxo(_))));
    }

    #[tokio::test]
    async fn test_mismatch_amount_error() {
        let storage = Memory::default();
        let account = make_account(1);
        let amount: Amount = 100.into();

        // Create initial deposit
        let deposit_tx = make_deposit_tx(account, amount, "deposit-1", 1000);
        let deposit_id = deposit_tx.id();
        storage
            .store_tx(deposit_tx)
            .await
            .expect("deposit should succeed");

        // Try to spend with wrong amount
        let wrong_amount: Amount = 50.into();
        let utxo = make_utxo(deposit_id, 0, wrong_amount);
        let spend_tx = Transaction::new(
            vec![utxo],
            vec![(account, wrong_amount)],
            "spend-1".to_string(),
            Some(2000),
        )
        .expect("transaction with wrong amount should be valid structurally");

        let result = storage.store_tx(spend_tx).await;
        assert!(matches!(result, Err(Error::MismatchAmount)));
    }

    #[tokio::test]
    async fn test_get_unspent_with_target_amount_exact() {
        let storage = Memory::default();
        let account = make_account(1);

        // Create two deposits
        let tx1 = make_deposit_tx(account, 50.into(), "deposit-1", 1000);
        let tx2 = make_deposit_tx(account, 50.into(), "deposit-2", 2000);
        storage
            .store_tx(tx1)
            .await
            .expect("first deposit should succeed");
        storage
            .store_tx(tx2)
            .await
            .expect("second deposit should succeed");

        // Request exactly 50 - should get one UTXO
        let unspent = storage
            .get_unspent(&account, Some(50.into()))
            .await
            .expect("get_unspent with target should succeed");
        assert_eq!(unspent.len(), 1);
        assert_eq!(*unspent[0].amount(), 50);
    }

    #[tokio::test]
    async fn test_get_unspent_with_target_amount_needs_multiple() {
        let storage = Memory::default();
        let account = make_account(1);

        // Create two deposits
        let tx1 = make_deposit_tx(account, 50.into(), "deposit-1", 1000);
        let tx2 = make_deposit_tx(account, 50.into(), "deposit-2", 2000);
        storage
            .store_tx(tx1)
            .await
            .expect("first deposit should succeed");
        storage
            .store_tx(tx2)
            .await
            .expect("second deposit should succeed");

        // Request 75 - should get two UTXOs
        let unspent = storage
            .get_unspent(&account, Some(75.into()))
            .await
            .expect("get_unspent with target should succeed");
        assert_eq!(unspent.len(), 2);
    }

    #[tokio::test]
    async fn test_get_unspent_without_target_returns_all() {
        let storage = Memory::default();
        let account = make_account(1);

        // Create three deposits
        let tx1 = make_deposit_tx(account, 50.into(), "deposit-1", 1000);
        let tx2 = make_deposit_tx(account, 50.into(), "deposit-2", 2000);
        let tx3 = make_deposit_tx(account, 50.into(), "deposit-3", 3000);
        storage
            .store_tx(tx1)
            .await
            .expect("first deposit should succeed");
        storage
            .store_tx(tx2)
            .await
            .expect("second deposit should succeed");
        storage
            .store_tx(tx3)
            .await
            .expect("third deposit should succeed");

        // Request without target - should get all UTXOs
        let unspent = storage
            .get_unspent(&account, None)
            .await
            .expect("get_unspent without target should succeed");
        assert_eq!(unspent.len(), 3);
    }

    #[tokio::test]
    async fn test_spent_utxos_not_returned() {
        let storage = Memory::default();
        let account = make_account(1);

        // Create initial deposit
        let deposit_tx = make_deposit_tx(account, 100.into(), "deposit-1", 1000);
        let deposit_id = deposit_tx.id();
        storage
            .store_tx(deposit_tx)
            .await
            .expect("deposit should succeed");

        // Spend the UTXO creating a new one
        let utxo = make_utxo(deposit_id, 0, 100.into());
        let spend_tx = Transaction::new(
            vec![utxo],
            vec![(account, 100.into())],
            "spend-1".to_string(),
            Some(2000),
        )
        .expect("spend transaction should be valid");
        storage
            .store_tx(spend_tx.clone())
            .await
            .expect("spend should succeed");

        // Get unspent - should only see the new UTXO
        let unspent = storage
            .get_unspent(&account, None)
            .await
            .expect("get_unspent should succeed after spend");
        assert_eq!(unspent.len(), 1);
        // The new UTXO should be from the spend transaction
        assert_eq!(unspent[0].id(), (spend_tx.id(), 0).into());
    }

    #[tokio::test]
    async fn test_multiple_accounts_isolated() {
        let storage = Memory::default();
        let account1 = make_account(1);
        let account2 = make_account(2);

        // Deposit to both accounts
        let tx1 = make_deposit_tx(account1, 100.into(), "deposit-1", 1000);
        let tx2 = make_deposit_tx(account2, 200.into(), "deposit-2", 2000);
        storage
            .store_tx(tx1)
            .await
            .expect("deposit to account1 should succeed");
        storage
            .store_tx(tx2)
            .await
            .expect("deposit to account2 should succeed");

        // Check each account has its own balance
        let unspent1 = storage
            .get_unspent(&account1, None)
            .await
            .expect("get_unspent for account1 should succeed");
        let unspent2 = storage
            .get_unspent(&account2, None)
            .await
            .expect("get_unspent for account2 should succeed");

        assert_eq!(unspent1.len(), 1);
        assert_eq!(*unspent1[0].amount(), 100);
        assert_eq!(unspent2.len(), 1);
        assert_eq!(*unspent2[0].amount(), 200);
    }

    #[tokio::test]
    async fn test_get_tx_by_reference_returns_transaction() {
        let storage = Memory::default();
        let account = make_account(1);

        let tx = make_deposit_tx(account, 100.into(), "deposit-1", 1000);
        let tx_id = tx.id();
        storage.store_tx(tx).await.expect("store_tx should succeed");

        let result = storage
            .get_tx_by_reference(&account, &"deposit-1".to_string())
            .await
            .expect("get_tx_by_reference should succeed");

        assert!(result.is_some());
        let found_tx = result.unwrap();
        assert_eq!(found_tx.id(), tx_id);
        assert_eq!(found_tx.reference(), "deposit-1");
    }

    #[tokio::test]
    async fn test_get_tx_by_reference_nonexistent_returns_none() {
        let storage = Memory::default();
        let account = make_account(1);

        let tx = make_deposit_tx(account, 100.into(), "deposit-1", 1000);
        storage.store_tx(tx).await.expect("store_tx should succeed");

        let result = storage
            .get_tx_by_reference(&account, &"nonexistent".to_string())
            .await
            .expect("get_tx_by_reference should succeed");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_tx_by_reference_wrong_account_returns_none() {
        let storage = Memory::default();
        let account1 = make_account(1);
        let account2 = make_account(2);

        let tx = make_deposit_tx(account1, 100.into(), "deposit-1", 1000);
        storage.store_tx(tx).await.expect("store_tx should succeed");

        // Try to get the transaction with the right reference but wrong account
        let result = storage
            .get_tx_by_reference(&account2, &"deposit-1".to_string())
            .await
            .expect("get_tx_by_reference should succeed");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_duplicate_reference_same_account_rejected() {
        let storage = Memory::default();
        let account = make_account(1);

        let tx1 = make_deposit_tx(account, 100.into(), "deposit-1", 1000);
        storage
            .store_tx(tx1)
            .await
            .expect("first store_tx should succeed");

        // Different transaction with same reference should fail
        let tx2 = make_deposit_tx(account, 50.into(), "deposit-1", 2000);
        let result = storage.store_tx(tx2).await;

        assert!(matches!(result, Err(Error::Duplicate)));
    }

    #[tokio::test]
    async fn test_same_reference_different_accounts_allowed() {
        let storage = Memory::default();
        let account1 = make_account(1);
        let account2 = make_account(2);

        let tx1 = make_deposit_tx(account1, 100.into(), "deposit-1", 1000);
        storage
            .store_tx(tx1)
            .await
            .expect("first store_tx should succeed");

        // Same reference but different account should succeed
        let tx2 = make_deposit_tx(account2, 50.into(), "deposit-1", 2000);
        storage
            .store_tx(tx2)
            .await
            .expect("second store_tx with same reference different account should succeed");

        // Verify both transactions exist
        let result1 = storage
            .get_tx_by_reference(&account1, &"deposit-1".to_string())
            .await
            .expect("get_tx_by_reference should succeed");
        let result2 = storage
            .get_tx_by_reference(&account2, &"deposit-1".to_string())
            .await
            .expect("get_tx_by_reference should succeed");

        assert!(result1.is_some());
        assert!(result2.is_some());
        // They should be different transactions
        assert_ne!(result1.unwrap().id(), result2.unwrap().id());
    }

    #[tokio::test]
    async fn test_get_tx_by_reference_empty_storage_returns_none() {
        let storage = Memory::default();
        let account = make_account(1);

        let result = storage
            .get_tx_by_reference(&account, &"any-reference".to_string())
            .await
            .expect("get_tx_by_reference should succeed for empty storage");

        assert!(result.is_none());
    }
}
