use sha2::{Digest, Sha256};

use crate::{Amount, FullAccount};

pub type HashId = [u8; 32];

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
pub struct Utxo {
    tx: HashId,
    pos: u8,
}

impl Utxo {
    fn to_bytes(&self) -> [u8; 33] {
        let mut bytes = [0u8; 33];
        bytes[..32].copy_from_slice(&self.tx);
        bytes[32] = self.pos;
        bytes
    }
}

/// Simplified version
pub struct Transaction {
    from: Vec<Utxo>,
    to: Vec<(FullAccount, Amount)>,
}

impl Transaction {
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

        // SHA256(SHA256(inputs) + SHA256(outputs))
        let mut final_hasher = Sha256::new();
        final_hasher.update(inputs_hash);
        final_hasher.update(outputs_hash);
        final_hasher.finalize().into()
    }
}
