use serde::{Deserialize, Serialize};

/// A unique identifier for an account.
///
/// Using u16 limits the system to 65,535 accounts, which is sufficient for
/// most use cases while keeping storage compact.
pub type Id = u16;

/// Categorizes sub-accounts to track different states of funds.
///
/// The UTXO model uses sub-accounts to separate funds by their state, avoiding
/// complex state machines and making balance calculations trivial (just sum UTXOs).
#[derive(Debug, Copy, Hash, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Type {
    /// Primary account holding available, spendable funds.
    Main,
    /// Sub-account holding funds under active dispute investigation.
    Disputed,
    /// Sub-account recording funds that have been permanently charged back.
    Chargeback,
}

impl Type {
    /// Serializes the account type to a single byte for storage and hashing.
    ///
    /// Using fixed byte values ensures consistent ordering and compact storage.
    pub fn to_byte(&self) -> u8 {
        match self {
            Type::Main => 0,
            Type::Disputed => 1,
            Type::Chargeback => 2,
        }
    }
}

/// A complete account identifier combining user ID and account type.
///
/// This composite key enables the UTXO model to track funds in different states
/// (Main, Disputed, Chargeback) as separate "accounts" while presenting a unified
/// view to external callers. Ordering is by ID first, then by Type, ensuring
/// all sub-accounts for a user are grouped together.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FullAccount((Id, Type));

impl From<Id> for FullAccount {
    fn from(value: Id) -> Self {
        FullAccount((value, Type::Main))
    }
}

impl From<(Id, Type)> for FullAccount {
    fn from(value: (Id, Type)) -> Self {
        FullAccount(value)
    }
}

impl FullAccount {
    /// Returns the numeric account identifier.
    pub fn id(&self) -> Id {
        self.0.0
    }

    /// Returns the sub-account type (Main, Disputed, or Chargeback).
    pub fn typ(&self) -> Type {
        self.0.1
    }

    /// Serializes to bytes for hashing and storage keys.
    ///
    /// Format: 2 bytes (ID, little-endian) + 1 byte (Type)
    pub fn to_bytes(&self) -> [u8; 3] {
        let mut bytes = [0u8; 3];
        bytes[..2].copy_from_slice(&self.0.0.to_le_bytes());
        bytes[2] = self.0.1.to_byte();
        bytes
    }
}
