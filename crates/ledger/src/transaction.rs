use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::{Amount, FullAccount, Reference};

pub type HashId = [u8; 32];

#[derive(Debug, Hash, Eq, PartialEq, PartialOrd, Ord, Clone, Copy)]
pub struct UtxoId {
    id: HashId,
    pos: u8,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid From in Tx")]
    InvalidFrom,
    #[error("Invalid To in Tx")]
    InvalidTo,
    #[error("Imbanced transaction")]
    Imbalanced,
}

/// Unspent transaction Output
///
/// This is the core unit of the ledger. It is composed by a transaction ID and a position that is
/// being spent to create this new transaction.
///
/// This simple model, inspired in Bitcoin core arquitecture, takes care of concurrency, as
/// committing this transcation would flag this UTXO as spent, which could only happen once,
/// guaranteed by our storage layer.
///
/// This also enable atomic multi-step movement of assets in a single transaction.
#[derive(Debug, Clone, Copy)]
pub struct Utxo {
    id: UtxoId,
    amount: Amount,
}

impl From<(HashId, u8)> for UtxoId {
    fn from(value: (HashId, u8)) -> Self {
        UtxoId {
            id: value.0,
            pos: value.1,
        }
    }
}

impl Utxo {
    pub fn new(id: UtxoId, amount: Amount) -> Self {
        Self { id, amount }
    }

    fn to_bytes(&self) -> [u8; 33] {
        let mut bytes = [0u8; 33];
        bytes[..32].copy_from_slice(&self.id.id);
        bytes[32] = self.id.pos;
        bytes
    }

    pub fn id(&self) -> UtxoId {
        self.id
    }

    pub fn amount(&self) -> Amount {
        self.amount
    }
}

/// Simplified version of an transaction, lot of details are left out due to time constraints
///
/// By design all transactions are final, to mimic statuses and the lifecycle of transactions it
/// would be achieved in another level with multiple accounts type (user.pending, user.available,
/// user.hold, etc)
#[derive(Debug, Clone)]
pub struct Transaction {
    from: Vec<Utxo>,
    to: Vec<(FullAccount, Amount)>,
    reference: Reference,
    timestamp: u64,
}

impl Transaction {
    pub fn new(
        from: Vec<Utxo>,
        to: Vec<(FullAccount, Amount)>,
        reference: Reference,
        timestamp: Option<u64>,
    ) -> Result<Self, Error> {
        if from.is_empty() && to.is_empty() {
            return Err(Error::InvalidFrom);
        }

        if !from.is_empty() && !to.is_empty() {
            let spending: i128 = from.iter().map(|input| *input.amount).sum();
            let receiving = to.iter().map(|(_, amount)| **amount).sum();

            if spending != receiving {
                return Err(Error::Imbalanced);
            }

            if spending <= 0 {
                return Err(Error::InvalidFrom);
            }
        }

        let timestamp = timestamp.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_micros() as u64
        });

        Ok(Self {
            from,
            to,
            timestamp,
            reference,
        })
    }

    pub fn inputs(&self) -> &[Utxo] {
        &self.from
    }

    pub fn outputs(&self) -> &[(FullAccount, Amount)] {
        &self.to
    }

    pub fn id(&self) -> HashId {
        // SHA256(inputs)
        let mut inputs_hasher = Sha256::new();
        for utxo in &self.from {
            inputs_hasher.update(utxo.to_bytes());
        }
        let inputs_hash = inputs_hasher.finalize();

        // SHA256(outputs)
        let mut outputs_hasher = Sha256::new();
        for (account, amount) in &self.to {
            outputs_hasher.update(account.to_bytes());
            outputs_hasher.update(amount.to_bytes());
        }
        let outputs_hash = outputs_hasher.finalize();

        // SHA256(SHA256(inputs) + SHA256(outputs) + timestamp + reference)
        let mut final_hasher = Sha256::new();
        final_hasher.update(inputs_hash);
        final_hasher.update(outputs_hash);
        final_hasher.update(self.timestamp.to_le_bytes());
        final_hasher.update(self.reference.as_bytes());
        final_hasher.finalize().into()
    }
}
