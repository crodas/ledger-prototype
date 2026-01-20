pub type Id = u32;

/// Account types
#[derive(Debug, Copy, Hash, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    /// Normal account types
    Main,
    /// Sub-account where all the held balance is moved to
    Held,
}

impl Type {
    pub fn to_byte(&self) -> u8 {
        match self {
            Type::Main => 0,
            Type::Held => 1,
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

impl FullAccount {
    pub fn to_bytes(&self) -> [u8; 5] {
        let mut bytes = [0u8; 5];
        bytes[..4].copy_from_slice(&self.0.0.to_le_bytes());
        bytes[4] = self.0.1.to_byte();
        bytes
    }
}

pub struct Account {
    id: Id,
    status: Status,
}
