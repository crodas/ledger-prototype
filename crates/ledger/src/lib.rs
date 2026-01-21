//! A generic UTXO-based ledger for recording financial transaction movements.
//!
//! This crate implements a simplified UTXO (Unspent Transaction Output) model inspired by
//! Bitcoin's architecture. The UTXO model provides several key advantages:
//!
//! - **Concurrency Safety**: Each UTXO can only be spent once, eliminating race conditions
//! - **Atomic Operations**: Multi-step transactions are inherently atomic
//! - **Auditability**: Complete transaction history is preserved and verifiable
//! - **Simplicity**: Balance is the sum of unspent outputs, no running totals to reconcile
//!
//! # Architecture
//!
//! The ledger uses sub-accounts to track different states of funds:
//! - `Main`: Normal available balance
//! - `Disputed`: Funds under dispute, frozen from spending
//! - `Chargeback`: Funds that have been charged back
//!
//! # Example
//!
//! ```rust,no_run
//! use ledger::{Ledger, Amount};
//!
//! async fn example() {
//!     let ledger = Ledger::default();
//!
//!     // Deposit funds
//!     let tx_id = ledger.deposit(1, "deposit-001".to_string(), Amount::from(1000)).await.unwrap();
//!
//!     // Check balance
//!     let balance = ledger.get_balances(1).await.unwrap();
//!     assert_eq!(*balance.available, 1000);
//!
//!     // Withdraw funds
//!     ledger.withdraw(1, "withdraw-001".to_string(), Amount::from(500)).await.unwrap();
//! }
//! ```

#![deny(missing_docs)]

mod account;
mod amount;
mod storage;
mod transaction;

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use serde::{Deserialize, Serialize};
use storage::{Memory, Storage};
use transaction::{HashId, Transaction, Utxo};

pub use self::{
    account::{FullAccount, Id as AccountId, Type as AccountType},
    amount::Amount,
};

/// A unique identifier for a transaction within an account's context.
///
/// References allow external systems to idempotently track transactions and enable
/// lookups for dispute resolution. Each reference must be unique per account.
pub type Reference = String;

/// Errors that can occur during ledger operations.
///
/// These errors represent the various failure modes when interacting with the ledger,
/// from invalid transactions to storage failures.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Transaction validation failed (imbalanced, invalid inputs/outputs).
    #[error(transparent)]
    Tx(#[from] transaction::Error),

    /// The requested resource (transaction, reference) was not found.
    #[error("Not found")]
    NotFound,

    /// Operation attempted on wrong transaction type (e.g., disputing a withdrawal).
    #[error("Wrong transaction type")]
    WrongType,

    /// Storage layer error (duplicate, missing UTXO, etc.).
    #[error(transparent)]
    Storage(#[from] storage::Error),

    /// Insufficient funds in account for the requested operation.
    #[error("Not enough in account")]
    NotEnough,

    /// Arithmetic overflow or underflow during calculation.
    #[error("Overflow or underflow error")]
    Math,

    /// Internal invariant violation that should never occur.
    #[error("Invalid internal state")]
    Internal,
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

/// A snapshot of an account's balance breakdown across different states.
///
/// The UTXO model naturally separates funds by their state, making balance
/// reconciliation straightforward: each category is simply the sum of its UTXOs.
#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
pub struct Balances {
    /// Funds available for withdrawal or transfer.
    pub available: Amount,
    /// Funds currently under dispute, frozen from spending.
    pub disputed: Amount,
    /// Funds that have been charged back and are no longer accessible.
    pub chargeback: Amount,
    /// Sum of available and disputed funds (excludes chargebacks).
    pub total: Amount,
}

/// A stream that yields unique account IDs, filtering out sub-accounts.
///
/// This is a thin wrapper over the storage layer's account stream that deduplicates
/// accounts by their ID, hiding the internal sub-account structure (Main, Disputed,
/// Chargeback) from callers who only need to enumerate distinct accounts.
pub struct AccountIterator {
    inner: Box<dyn Stream<Item = Result<FullAccount, storage::Error>> + 'static + Unpin>,
    latest: Option<AccountId>,
}

impl Stream for AccountIterator {
    type Item = Result<AccountId, Error>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(account))) => {
                    let account = account.id();

                    if Some(account) != this.latest {
                        this.latest = Some(account);
                        return Poll::Ready(Some(Ok(account)));
                    }
                    // Skip duplicate account IDs (sub-accounts) and continue polling
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(Error::from(e)))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> Ledger<S>
where
    S: Storage,
{
    /// Creates a new ledger with the specified storage backend.
    ///
    /// This allows using custom storage implementations (e.g., database-backed)
    /// instead of the default in-memory storage.
    pub fn new(storage: S) -> Self {
        Ledger {
            storage: Arc::new(storage),
        }
    }

    /// Deposits funds into an account, creating new UTXOs.
    ///
    /// Deposits are transactions with no inputs and one output, effectively creating
    /// new money in the system. The reference must be unique per account to ensure
    /// idempotency and enable dispute lookups.
    ///
    /// # Arguments
    /// * `account` - The account to credit
    /// * `reference` - Unique identifier for this deposit (e.g., external transaction ID)
    /// * `amount` - The amount to deposit in the lowest denomination
    ///
    /// # Returns
    /// The transaction hash ID on success
    pub async fn deposit(
        &self,
        account: AccountId,
        reference: Reference,
        amount: Amount,
    ) -> Result<HashId, Error> {
        let new_tx = Transaction::new(vec![], vec![(account.into(), amount)], reference, None)?;
        let tx_id = new_tx.id();
        self.storage.store_tx(new_tx).await?;
        Ok(tx_id)
    }

    /// Returns a stream of all unique account IDs in the ledger.
    ///
    /// Sub-accounts (Disputed, Chargeback) are filtered out, returning only
    /// distinct account identifiers. Useful for reporting and batch operations.
    pub async fn get_accounts(&self) -> impl Stream<Item = Result<AccountId, Error>> {
        AccountIterator {
            inner: Box::new(self.storage.get_accounts().await),
            latest: None,
        }
    }

    /// Retrieves the balance breakdown for an account.
    ///
    /// The UTXO model makes balance calculation straightforward: simply sum all
    /// unspent outputs for each sub-account type. This naturally provides an
    /// audit trail and prevents double-counting.
    pub async fn get_balances(&self, account: AccountId) -> Result<Balances, Error> {
        let main = self
            .storage
            .get_unspent(&(account, AccountType::Main).into(), None)
            .await?
            .into_iter()
            .map(|u| *u.amount())
            .sum::<i128>();
        let disputed = self
            .storage
            .get_unspent(&(account, AccountType::Disputed).into(), None)
            .await?
            .into_iter()
            .map(|u| *u.amount())
            .sum::<i128>();
        let chargeback = self
            .storage
            .get_unspent(&(account, AccountType::Chargeback).into(), None)
            .await?
            .into_iter()
            .map(|u| *u.amount())
            .sum::<i128>();

        Ok(Balances {
            available: main.into(),
            disputed: disputed.into(),
            chargeback: chargeback.into(),
            total: main.checked_add(disputed).ok_or(Error::Math)?.into(),
        })
    }

    /// Withdraws funds from an account, consuming UTXOs.
    ///
    /// Withdrawals are transactions with inputs and no outputs, effectively removing
    /// money from the system. The UTXO model handles coin selection automatically:
    /// if selected UTXOs exceed the withdrawal amount, an intermediate "exchange"
    /// transaction creates change back to the account.
    ///
    /// # Arguments
    /// * `account` - The account to debit
    /// * `reference` - Unique identifier for this withdrawal
    /// * `amount` - The amount to withdraw in the lowest denomination
    ///
    /// # Errors
    /// Returns `Error::NotEnough` if the account has insufficient available funds.
    pub async fn withdraw(
        &self,
        account: AccountId,
        reference: Reference,
        amount: Amount,
    ) -> Result<HashId, Error> {
        let inputs = self
            .storage
            .get_unspent(&account.into(), Some(amount))
            .await?;

        let total: i128 = inputs.iter().map(|x| *x.amount()).sum();
        let (id, transactions) = if total < *amount {
            return Err(Error::NotEnough);
        } else if total > *amount {
            // The selected inputs are more than the requested amount to withdraw, so an
            // intermediate tx is needed, since the design of ledger does not allow imbalanced
            // transactions (except for deposit and withdrawal, but for that to happen one side if
            // empty)
            let exchange_tx = Transaction::new(
                inputs,
                vec![
                    (account.into(), amount), // amount to the withdrawal
                    (
                        account.into(),
                        total.checked_sub(*amount).ok_or(Error::Math)?.into(), // exchange
                    ),
                ],
                format!("Exchange for {}", reference),
                None,
            )?;
            let withdrawal = Transaction::new(
                vec![Utxo::new((exchange_tx.id(), 0u8).into(), amount)],
                vec![],
                reference,
                None,
            )?;
            (withdrawal.id(), vec![exchange_tx, withdrawal])
        } else {
            // a single transaction
            let withdrawal = Transaction::new(inputs, vec![], reference, None)?;
            (withdrawal.id(), vec![withdrawal])
        };

        for tx in transactions {
            self.storage.store_tx(tx).await?;
        }

        Ok(id)
    }

    /// Initiates a dispute on a deposit, freezing the disputed amount.
    ///
    /// Only deposits (transactions with no inputs) can be disputed. The disputed
    /// amount is moved from the Main sub-account to the Disputed sub-account,
    /// preventing it from being spent while the dispute is being investigated.
    ///
    /// # Arguments
    /// * `account` - The account containing the disputed deposit
    /// * `reference` - The reference of the original deposit to dispute
    ///
    /// # Errors
    /// - `Error::NotFound` if no deposit exists with the given reference
    /// - `Error::WrongType` if the referenced transaction is not a deposit
    pub async fn dispute(&self, account: AccountId, reference: Reference) -> Result<(), Error> {
        let tx_to_dispute = self
            .storage
            .get_tx_by_reference(&account.into(), &reference)
            .await?
            .ok_or(Error::NotFound)?;

        if !tx_to_dispute.inputs().is_empty() || tx_to_dispute.outputs().len() != 1 {
            // Only deposits can be disputed. Deposits have no input and 1 output.
            return Err(Error::WrongType);
        }

        let (_, disputed_amount) = tx_to_dispute
            .outputs()
            .first()
            .cloned()
            .ok_or(Error::WrongType)?;

        // Happy path, the user still have the amount on hold, otherwise a negative deposit (or a
        // loan) must be created to compensate

        let inputs = self
            .storage
            .get_unspent(&account.into(), Some(disputed_amount))
            .await?;
        let available_amounts: i128 = inputs.iter().map(|f| *f.amount()).sum();

        let target_in_held = ((account, AccountType::Disputed).into(), disputed_amount);
        let disputed_ref = format!("dispute:{}", reference);

        let disputed_tx = if available_amounts < *disputed_amount {
            // In this scenario their main account will go negative, but the 100% positive amount should go to dispute
            todo!()
        } else if available_amounts == *disputed_amount {
            // No change
            Transaction::new(inputs, vec![target_in_held], disputed_ref, None)?
        } else {
            // Move the funds to the held account and get the exchange back to the main account
            Transaction::new(
                inputs,
                vec![
                    target_in_held,
                    (
                        // Exchange
                        account.into(),
                        available_amounts
                            .checked_sub(*disputed_amount)
                            .ok_or(Error::Math)?
                            .into(),
                    ),
                ],
                disputed_ref,
                None,
            )?
        };

        self.storage.store_tx(disputed_tx).await?;

        Ok(())
    }

    /// Resolves a dispute in favor of the account holder, releasing frozen funds.
    ///
    /// Moves funds from the Disputed sub-account back to the Main sub-account,
    /// making them available for spending again. This should be called when an
    /// investigation determines the original deposit was legitimate.
    ///
    /// # Arguments
    /// * `account` - The account with the disputed funds
    /// * `reference` - The reference of the original disputed deposit
    ///
    /// # Errors
    /// - `Error::NotFound` if no dispute exists for the given reference
    /// - `Error::Internal` if disputed funds are missing (should never happen)
    pub async fn resolve(&self, account: AccountId, reference: Reference) -> Result<(), Error> {
        let disputed_ref = format!("dispute:{}", reference);
        let resolved_ref = format!("resolved:{}", reference);
        let disputed_account = (account, AccountType::Disputed).into();
        let disputed_tx = self
            .storage
            .get_tx_by_reference(&disputed_account, &disputed_ref)
            .await?
            .ok_or(Error::NotFound)?;

        let amount_to_restore = disputed_tx
            .outputs()
            .iter()
            .filter_map(|(account, total)| {
                if *account == disputed_account {
                    Some(**total)
                } else {
                    None
                }
            })
            .sum::<i128>();

        let inputs = self
            .storage
            .get_unspent(&disputed_account, Some(amount_to_restore.into()))
            .await?;

        let available_amounts: i128 = inputs.iter().map(|f| *f.amount()).sum();
        let restore_tx = (account.into(), amount_to_restore.into());

        let disputed_tx = if available_amounts < amount_to_restore {
            // This cannot happen, as this account should not let money be moved, other than move it
            // back to the main when the dispute has been resolved or to locked if it was a
            // chargeback
            return Err(Error::Internal);
        } else if available_amounts == amount_to_restore {
            // No change
            Transaction::new(inputs, vec![restore_tx], resolved_ref, None)?
        } else {
            // Move the funds to the held account and get the exchange back to the main account
            Transaction::new(
                inputs,
                vec![
                    restore_tx,
                    (
                        // Exchange
                        disputed_account,
                        available_amounts
                            .checked_sub(amount_to_restore)
                            .ok_or(Error::Math)?
                            .into(),
                    ),
                ],
                resolved_ref,
                None,
            )?
        };

        self.storage.store_tx(disputed_tx).await?;

        Ok(())
    }

    /// Processes a chargeback, permanently removing funds from the account.
    ///
    /// Moves funds from the Disputed sub-account to the Chargeback sub-account,
    /// recording that the funds have been reversed. Chargebacked funds are tracked
    /// separately for auditing purposes but are no longer accessible to the account.
    ///
    /// # Arguments
    /// * `account` - The account with the disputed funds
    /// * `reference` - The reference of the original disputed deposit
    ///
    /// # Errors
    /// - `Error::NotFound` if no dispute exists for the given reference
    /// - `Error::Internal` if disputed funds are missing (should never happen)
    pub async fn chargeback(&self, account: AccountId, reference: Reference) -> Result<(), Error> {
        let disputed_ref = format!("dispute:{}", reference);
        let chargeback_ref = format!("chargeback:{}", reference);
        let disputed_account = (account, AccountType::Disputed).into();
        let disputed_tx = self
            .storage
            .get_tx_by_reference(&disputed_account, &disputed_ref)
            .await?
            .ok_or(Error::NotFound)?;

        let amount_to_chargeback = disputed_tx
            .outputs()
            .iter()
            .filter_map(|(account, total)| {
                if *account == disputed_account {
                    Some(**total)
                } else {
                    None
                }
            })
            .sum::<i128>();

        let inputs = self
            .storage
            .get_unspent(&disputed_account, Some(amount_to_chargeback.into()))
            .await?;

        let available_amounts: i128 = inputs.iter().map(|f| *f.amount()).sum();
        let chargeback_tx = (
            (account, AccountType::Chargeback).into(),
            amount_to_chargeback.into(),
        );

        let chargeback_tx = if available_amounts < amount_to_chargeback {
            // This cannot happen, as this account should not let money be moved, other than move it
            // back to the main when the dispute has been resolved or to locked if it was a
            // chargeback
            return Err(Error::Internal);
        } else if available_amounts == amount_to_chargeback {
            // No change
            Transaction::new(inputs, vec![chargeback_tx], chargeback_ref, None)?
        } else {
            // Move the funds to the held account and get the exchange back to the main account
            Transaction::new(
                inputs,
                vec![
                    chargeback_tx,
                    (
                        // Exchange
                        disputed_account,
                        available_amounts
                            .checked_sub(amount_to_chargeback)
                            .ok_or(Error::Math)?
                            .into(),
                    ),
                ],
                chargeback_ref,
                None,
            )?
        };

        self.storage.store_tx(chargeback_tx).await?;

        Ok(())
    }

    /// Transfers funds between accounts (not yet implemented).
    ///
    /// This will enable peer-to-peer transfers by consuming UTXOs from the source
    /// account and creating new UTXOs in the destination account within a single
    /// atomic transaction.
    ///
    /// # Panics
    /// Currently unimplemented and will panic if called.
    pub fn movement(&self, _from: AccountId, _to: AccountId, _amount: Amount) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn assert_balance(
        ledger: &Ledger<Memory>,
        account: AccountId,
        main: i128,
        disputed: i128,
    ) {
        let balances = ledger
            .get_balances(account)
            .await
            .expect("get_balances should succeed");
        assert_eq!(*balances.available, main, "main balance mismatch");
        assert_eq!(*balances.disputed, disputed, "disputed balance mismatch");
        assert_eq!(*balances.total, main + disputed, "total balance mismatch");
    }

    #[tokio::test]
    async fn test_deposit_creates_balance() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        let tx_id = ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify the transaction was created (non-zero hash)
        assert_ne!(tx_id, [0u8; 32]);

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;
    }

    #[tokio::test]
    async fn test_deposit_and_withdraw_exact_amount() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Withdraw exactly 100
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 100.into())
            .await
            .expect("exact withdrawal should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Verify balance after withdrawal
        assert_balance(&ledger, account_id, 0, 0).await;
    }

    #[tokio::test]
    async fn test_withdraw_partial_amount() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Withdraw 60 (partial)
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 60.into())
            .await
            .expect("partial withdrawal should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Verify balance after first withdrawal
        assert_balance(&ledger, account_id, 40, 0).await;

        // Should be able to withdraw remaining 40
        let tx_id2 = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 40.into())
            .await
            .expect("withdrawing remaining balance should succeed");

        assert_ne!(tx_id2, [0u8; 32]);

        // Verify balance after second withdrawal
        assert_balance(&ledger, account_id, 0, 0).await;
    }

    #[tokio::test]
    async fn test_over_withdrawal_not_possible() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Try to withdraw 150 - should fail
        let result = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 150.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));

        // Verify balance unchanged after failed withdrawal
        assert_balance(&ledger, account_id, 100, 0).await;
    }

    #[tokio::test]
    async fn test_withdraw_from_empty_account() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Verify balance is 0 before any operation
        assert_balance(&ledger, account_id, 0, 0).await;

        // Try to withdraw without any deposit
        let result = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 50.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_multiple_deposits_accumulate() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 50 three times
        ledger
            .deposit(account_id, "deposit-1".to_string(), 50.into())
            .await
            .expect("first deposit should succeed");
        ledger
            .deposit(account_id, "deposit-2".to_string(), 50.into())
            .await
            .expect("second deposit should succeed");
        ledger
            .deposit(account_id, "deposit-3".to_string(), 50.into())
            .await
            .expect("third deposit should succeed");

        // Verify balance after 3 deposits
        assert_balance(&ledger, account_id, 150, 0).await;

        // Withdraw 120 (needs multiple UTXOs)
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 120.into())
            .await
            .expect("withdrawal using multiple UTXOs should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Verify balance after first withdrawal
        assert_balance(&ledger, account_id, 30, 0).await;

        // Should have 30 left
        let tx_id2 = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 30.into())
            .await
            .expect("withdrawing remaining balance should succeed");

        assert_ne!(tx_id2, [0u8; 32]);

        // Verify balance after second withdrawal
        assert_balance(&ledger, account_id, 0, 0).await;
    }

    #[tokio::test]
    async fn test_cannot_withdraw_more_than_remaining_after_partial() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Withdraw 70
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 70.into())
            .await
            .expect("partial withdrawal should succeed");

        // Verify balance after first withdrawal
        assert_balance(&ledger, account_id, 30, 0).await;

        // Try to withdraw 50 (only 30 remaining) - should fail
        let result = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 50.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_different_accounts_isolated() {
        let ledger = Ledger::default();
        let account1: AccountId = 1;
        let account2: AccountId = 2;

        // Deposit to account1
        ledger
            .deposit(account1, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit to account1 should succeed");

        // Verify balances: account1 has 100, account2 has 0
        assert_balance(&ledger, account1, 100, 0).await;
        assert_balance(&ledger, account2, 0, 0).await;

        // Try to withdraw from account2 - should fail (no balance)
        let result = ledger
            .withdraw(account2, "withdraw-1".to_string(), 50.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));

        // Account1 should still be able to withdraw
        let tx_id = ledger
            .withdraw(account1, "withdraw-2".to_string(), 100.into())
            .await
            .expect("withdrawal from account1 should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Verify balance after withdrawal: account1 has 0
        assert_balance(&ledger, account1, 0, 0).await;
    }

    #[tokio::test]
    async fn test_withdraw_exact_balance_leaves_nothing() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Withdraw exactly 100
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 100.into())
            .await
            .expect("exact withdrawal should succeed");

        // Verify balance after withdrawal
        assert_balance(&ledger, account_id, 0, 0).await;

        // Try to withdraw even 1 - should fail
        let result = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 1.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_dispute_moves_funds_to_held_exact_amount() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Dispute the deposit
        ledger
            .dispute(account_id, "deposit-1".to_string())
            .await
            .expect("dispute should succeed");

        // After dispute: main=0, disputed=100, total=-100
        assert_balance(&ledger, account_id, 0, 100).await;

        // After dispute, main account should have no funds
        let result = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 1.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_dispute_nonexistent_reference_fails() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Verify balance after deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Try to dispute a non-existent reference
        let result = ledger
            .dispute(account_id, "nonexistent-ref".to_string())
            .await;

        assert!(matches!(result, Err(Error::NotFound)));

        // Verify balance unchanged after failed dispute
        assert_balance(&ledger, account_id, 100, 0).await;
    }

    #[tokio::test]
    async fn test_dispute_transfer_fails_wrong_type() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit 100
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit should succeed");

        // Partial withdraw creates an exchange transaction which has both inputs and outputs
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 50.into())
            .await
            .expect("withdrawal should succeed");

        // Verify balance after withdrawal
        assert_balance(&ledger, account_id, 50, 0).await;

        // Try to dispute the exchange transaction (has inputs, so it's not a deposit)
        // The exchange tx has reference "Exchange for withdraw-1"
        let result = ledger
            .dispute(account_id, "Exchange for withdraw-1".to_string())
            .await;

        assert!(matches!(result, Err(Error::WrongType)));
    }

    #[tokio::test]
    async fn test_duplicate_deposit_reference_fails() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // First deposit
        ledger
            .deposit(account_id, "deposit-1".to_string(), 100.into())
            .await
            .expect("first deposit should succeed");

        // Verify balance after first deposit
        assert_balance(&ledger, account_id, 100, 0).await;

        // Second deposit with same reference should fail
        let result = ledger
            .deposit(account_id, "deposit-1".to_string(), 50.into())
            .await;

        assert!(matches!(
            result,
            Err(Error::Storage(storage::Error::Duplicate))
        ));

        // Verify balance unchanged (still 100) after failed duplicate deposit
        assert_balance(&ledger, account_id, 100, 0).await;
    }

    #[tokio::test]
    async fn test_same_reference_different_accounts_succeeds() {
        let ledger = Ledger::default();
        let account1: AccountId = 1;
        let account2: AccountId = 2;

        // Deposit to account1
        ledger
            .deposit(account1, "deposit-1".to_string(), 100.into())
            .await
            .expect("deposit to account1 should succeed");

        // Deposit to account2 with same reference should succeed (different accounts)
        ledger
            .deposit(account2, "deposit-1".to_string(), 50.into())
            .await
            .expect("deposit to account2 with same reference should succeed");

        // Verify each account has correct balance
        assert_balance(&ledger, account1, 100, 0).await;
        assert_balance(&ledger, account2, 50, 0).await;
    }

    #[tokio::test]
    async fn test_dispute_after_utxo_shuffle() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

        // Deposit a: 10
        ledger
            .deposit(account_id, "a".to_string(), 10.into())
            .await
            .expect("deposit a should succeed");

        // Verify balance after deposit a
        assert_balance(&ledger, account_id, 10, 0).await;

        // Deposit b: 5
        ledger
            .deposit(account_id, "b".to_string(), 5.into())
            .await
            .expect("deposit b should succeed");

        // Verify balance after deposit b
        assert_balance(&ledger, account_id, 15, 0).await;

        // Withdraw 11 - this consumes both UTXOs and creates exchange (15-11=4 remaining)
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 11.into())
            .await
            .expect("withdrawal should succeed");

        // Verify balance after withdrawal
        assert_balance(&ledger, account_id, 4, 0).await;

        // Deposit c: 1 (chosen so exchange(4) + c(1) = 5, exactly matching disputed amount)
        ledger
            .deposit(account_id, "c".to_string(), 1.into())
            .await
            .expect("deposit c should succeed");

        // Verify balance after deposit c
        assert_balance(&ledger, account_id, 5, 0).await;

        // At this point: UTXOs are shuffled - we have exchange(4) + c(1) = 5 total
        // Original deposits a and b UTXOs are spent, but their tx records remain

        // Dispute b (5) - should find original deposit tx by reference and move 5 to held
        ledger
            .dispute(account_id, "b".to_string())
            .await
            .expect("dispute should succeed");

        // After dispute: main=0, disputed=5, total=-5
        assert_balance(&ledger, account_id, 0, 5).await;

        // After dispute: all 5 moved to held, 0 should remain in main account
        let result = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 1.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_get_accounts_returns_unique_ids_no_sub_accounts() {
        use futures::StreamExt;

        let ledger = Ledger::default();

        // Create multiple accounts in non-sequential order using a loop
        let account_ids: Vec<AccountId> = vec![5, 2, 8, 1, 9, 3, 7, 4, 6, 10];
        for (i, &id) in account_ids.iter().enumerate() {
            ledger
                .deposit(id, format!("deposit-{}", i), 100.into())
                .await
                .expect("deposit should succeed");
        }

        // Create disputes for some accounts (this creates sub-accounts internally)
        for &id in &[2, 5, 8] {
            ledger
                .dispute(
                    id,
                    format!(
                        "deposit-{}",
                        account_ids.iter().position(|&x| x == id).unwrap()
                    ),
                )
                .await
                .expect("dispute should succeed");
        }

        // Collect all accounts from the ledger's get_accounts
        let mut stream = ledger.get_accounts().await;
        let mut accounts: Vec<AccountId> = Vec::new();
        while let Some(result) = stream.next().await {
            accounts.push(result.expect("stream should not error"));
        }

        // Verify we got exactly 10 unique account IDs (no sub-accounts)
        assert_eq!(accounts.len(), 10);

        // Verify all expected accounts are present
        let mut sorted_expected = account_ids.clone();
        sorted_expected.sort();
        let mut sorted_actual = accounts.clone();
        sorted_actual.sort();
        assert_eq!(sorted_actual, sorted_expected);
    }
}
