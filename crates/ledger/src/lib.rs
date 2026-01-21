//! This is meant to be a generic simple ledger to record transaction movements

mod account;
mod amount;
mod storage;
mod transaction;

use std::sync::Arc;

use storage::{Memory, Storage};
use transaction::{HashId, Transaction, Utxo};

pub use self::{
    account::{FullAccount, Id as AccountId, Type as AccountType},
    amount::Amount,
};

pub type Reference = String;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Tx(#[from] transaction::Error),

    #[error("Not found")]
    NotFound,

    #[error("Wrong transaction type")]
    WrongType,

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
                    (account.into(), amount), // amount to the withdrawl
                    (
                        account.into(),
                        total.checked_sub(*amount).ok_or(Error::Math)?.into(), // exchange
                    ),
                ],
                format!("Exchange for {}", reference),
                None,
            )?;
            let withdrawl = Transaction::new(
                vec![Utxo::new((exchange_tx.id(), 0u8).into(), amount)],
                vec![],
                reference,
                None,
            )?;
            (withdrawl.id(), vec![exchange_tx, withdrawl])
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

    /// Creates a dispute
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
            .outputs().first()
            .cloned()
            .ok_or(Error::WrongType)?;

        // Happy path, the user still have the amount on hold, otherwise a negative deposit (or a
        // loan) must be created to compesate

        let inputs = self
            .storage
            .get_unspent(&account.into(), Some(disputed_amount))
            .await?;
        let available_amounts: i128 = inputs.iter().map(|f| *f.amount()).sum();

        let target_in_held = ((account, AccountType::Held).into(), disputed_amount);

        let disputed_tx = if available_amounts < *disputed_amount {
            // In this scenario a their main account will go negative, but the 100% positve amount should go to dispute
            todo!()
        } else if available_amounts == *disputed_amount {
            // No change
            Transaction::new(inputs, vec![target_in_held], reference, None)?
        } else {
            // Move the funds to the held account and get the exchagne back to the main account
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
                reference,
                None,
            )?
        };

        self.storage.store_tx(disputed_tx).await?;

        Ok(())
    }

    pub fn movement(&self, _from: AccountId, _to: AccountId, _amount: Amount) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Withdraw exactly 100
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 100.into())
            .await
            .expect("exact withdrawal should succeed");

        assert_ne!(tx_id, [0u8; 32]);
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

        // Withdraw 60 (partial)
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 60.into())
            .await
            .expect("partial withdrawal should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Should be able to withdraw remaining 40
        let tx_id2 = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 40.into())
            .await
            .expect("withdrawing remaining balance should succeed");

        assert_ne!(tx_id2, [0u8; 32]);
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

        // Try to withdraw 150 - should fail
        let result = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 150.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }

    #[tokio::test]
    async fn test_withdraw_from_empty_account() {
        let ledger = Ledger::default();
        let account_id: AccountId = 1;

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

        // Withdraw 120 (needs multiple UTXOs)
        let tx_id = ledger
            .withdraw(account_id, "withdraw-1".to_string(), 120.into())
            .await
            .expect("withdrawal using multiple UTXOs should succeed");

        assert_ne!(tx_id, [0u8; 32]);

        // Should have 30 left
        let tx_id2 = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 30.into())
            .await
            .expect("withdrawing remaining balance should succeed");

        assert_ne!(tx_id2, [0u8; 32]);
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

        // Withdraw 70
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 70.into())
            .await
            .expect("partial withdrawal should succeed");

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

        // Dispute the deposit
        ledger
            .dispute(account_id, "deposit-1".to_string())
            .await
            .expect("dispute should succeed");

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

        // Try to dispute a non-existent reference
        let result = ledger
            .dispute(account_id, "nonexistent-ref".to_string())
            .await;

        assert!(matches!(result, Err(Error::NotFound)));
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

        // Second deposit with same reference should fail
        let result = ledger
            .deposit(account_id, "deposit-1".to_string(), 50.into())
            .await;

        assert!(matches!(result, Err(Error::Storage(storage::Error::Duplicate))));
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

        // Deposit b: 5
        ledger
            .deposit(account_id, "b".to_string(), 5.into())
            .await
            .expect("deposit b should succeed");

        // Withdraw 11 - this consumes both UTXOs and creates exchange (15-11=4 remaining)
        ledger
            .withdraw(account_id, "withdraw-1".to_string(), 11.into())
            .await
            .expect("withdrawal should succeed");

        // Deposit c: 1 (chosen so exchange(4) + c(1) = 5, exactly matching disputed amount)
        ledger
            .deposit(account_id, "c".to_string(), 1.into())
            .await
            .expect("deposit c should succeed");

        // At this point: UTXOs are shuffled - we have exchange(4) + c(1) = 5 total
        // Original deposits a and b UTXOs are spent, but their tx records remain

        // Dispute b (5) - should find original deposit tx by reference and move 5 to held
        ledger
            .dispute(account_id, "b".to_string())
            .await
            .expect("dispute should succeed");

        // After dispute: all 5 moved to held, 0 should remain in main account
        let result = ledger
            .withdraw(account_id, "withdraw-2".to_string(), 1.into())
            .await;

        assert!(matches!(result, Err(Error::NotEnough)));
    }
}
