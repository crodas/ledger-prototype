/// Amount
///
/// The ledger supports negative and positive numbers. By definition the ledger is append only, and
/// all transactions are final. That's why chargebacks are negative deposits.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Amount(i128);

impl From<i128> for Amount {
    fn from(value: i128) -> Self {
        Amount(value)
    }
}

impl Amount {
    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_le_bytes()
    }
}
