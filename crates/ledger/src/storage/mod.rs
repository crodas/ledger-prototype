use crate::transaction::{Transaction, Utxo, UtxoId};
use crate::{FullAccount, Reference};

use super::Amount;

mod memory;

use futures::Stream;
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
/// All math is not done, and its sole responsibilities are storage, durability and correctness.
#[async_trait::async_trait]
pub trait Storage {
    /// Get unspent UTXO for this given account. Optionally it be capped to cover a target_amount.
    ///
    /// If
    ///
    /// This function is used to request balance or to see how much is spendable. All math is
    /// avoided in the storage layer, it is good to keep it as dumb as possible, with one
    /// responsibility, storage and correctness.
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

    /// Returns an iterator with a list of account. An iterator is used to avoid loading the whole
    /// list (which its size is unknown)
    ///
    /// It is expected the accounts are sorted naturally for the stream filtering to work with
    /// subaccounts
    async fn get_accounts(
        &self,
    ) -> impl Stream<Item = Result<FullAccount, Error>> + Send + Sync + 'static + Unpin;

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

#[cfg(test)]
/// Generates a comprehensive test suite for any `Storage` implementation.
///
/// This macro provides reusable tests that verify correct behavior of the
/// storage contract: UTXO tracking, double-spend prevention, reference
/// uniqueness, and account isolation.
///
/// # Usage
/// ```ignore
/// crate::storage_test!(MyStorage::new());
/// ```
#[macro_export]
macro_rules! storage_test {
    ($storage_expr:expr) => {
        use $crate::storage::Error;
        use $crate::transaction::{HashId, Transaction, Utxo};
        use $crate::{AccountId, AccountType, Amount, FullAccount};

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
            let storage = $storage_expr;
            let account = make_account(1);

            let result = storage
                .get_unspent(&account, None)
                .await
                .expect("get_unspent should succeed for empty account");
            assert!(result.is_empty());
        }

        #[tokio::test]
        async fn test_store_and_get_unspent() {
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
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
            let storage = $storage_expr;
            let account = make_account(1);

            let result = storage
                .get_tx_by_reference(&account, &"any-reference".to_string())
                .await
                .expect("get_tx_by_reference should succeed for empty storage");

            assert!(result.is_none());
        }

        #[tokio::test]
        async fn test_get_accounts_returns_in_order() {
            use futures::StreamExt;

            let storage = $storage_expr;

            // Create accounts in non-sequential order to verify sorting
            let account_ids: Vec<AccountId> = vec![5, 2, 8, 1, 9, 3, 7, 4, 6, 10];
            for (i, &id) in account_ids.iter().enumerate() {
                let account = make_account(id);
                let tx = make_deposit_tx(
                    account,
                    100.into(),
                    &format!("deposit-{}", i),
                    (i * 1000) as u64,
                );
                storage.store_tx(tx).await.expect("deposit should succeed");
            }

            // Also create some disputed sub-accounts for a few accounts
            for &id in &[2, 5, 8] {
                let disputed_account: FullAccount = (id, AccountType::Disputed).into();
                let tx = make_deposit_tx(
                    disputed_account,
                    50.into(),
                    &format!("disputed-{}", id),
                    100000,
                );
                storage
                    .store_tx(tx)
                    .await
                    .expect("disputed deposit should succeed");
            }

            // Collect all accounts from the stream
            let mut stream = storage.get_accounts().await;
            let mut accounts: Vec<FullAccount> = Vec::new();
            while let Some(result) = stream.next().await {
                accounts.push(result.expect("stream should not error"));
            }

            // Verify accounts are returned in sorted order (by id, then by type)
            let mut sorted = accounts.clone();
            sorted.sort();
            assert_eq!(
                accounts, sorted,
                "accounts should be returned in sorted order"
            );

            // Verify we got all accounts (10 main + 3 disputed = 13)
            assert_eq!(accounts.len(), 13);

            // Verify the first few are in expected order: (1, Main), (2, Main), (2, Disputed), ...
            assert_eq!(accounts[0].id(), 1);
            assert_eq!(accounts[0].typ(), AccountType::Main);
            assert_eq!(accounts[1].id(), 2);
            assert_eq!(accounts[1].typ(), AccountType::Main);
            assert_eq!(accounts[2].id(), 2);
            assert_eq!(accounts[2].typ(), AccountType::Disputed);
        }
    };
}
