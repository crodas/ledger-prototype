//! This is meant to be a generic simple ledger to record transaction movements

mod account;
mod amount;
mod storage;
mod transaction;

pub use self::{
    account::{FullAccount, Id as AccountId, Type as AccountType},
    amount::Amount,
};

pub type Reference = String;

/// Very simple UTXO based ledger, a simplified version of my own ledger prototype that someday I
/// will make it open source and will be promoted to my Github
///
/// https://git.cesar.com.py/cesar/ledger-prototype
#[derive(Debug, Clone)]
pub struct Ledger {
    // TODO: implement
}

impl Ledger {
    pub fn new() -> Self {
        todo!()
    }

    pub fn deposit(&mut self, account: AccountId, reference: Reference, amount: Amount) {
        todo!()
    }

    pub fn withdraw(&mut self, account: AccountId, amount: Amount) {
        todo!()
    }

    pub fn movement(&mut self, from: AccountId, to: AccountId, amount: Amount) {
        todo!()
    }
}
