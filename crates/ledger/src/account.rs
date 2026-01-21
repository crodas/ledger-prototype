pub type Id = u16;

/// Account types
#[derive(Debug, Copy, Hash, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    /// Normal account types
    Main,
    /// Sub-account where all the disputed balances are moved to
    Disputed,
}

impl Type {
    pub fn to_byte(&self) -> u8 {
        match self {
            Type::Main => 0,
            Type::Disputed => 1,
        }
    }
}

/// Account Status
pub enum Status {
    Operational,
    Locked,
}

/// Internal full Account Id
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
    pub fn id(&self) -> Id {
        self.0.0
    }

    pub fn typ(&self) -> Type {
        self.0.1
    }

    pub fn to_bytes(&self) -> [u8; 3] {
        let mut bytes = [0u8; 3];
        bytes[..2].copy_from_slice(&self.0.0.to_le_bytes());
        bytes[2] = self.0.1.to_byte();
        bytes
    }
}

pub struct Account {
    id: Id,
    status: Status,
}
