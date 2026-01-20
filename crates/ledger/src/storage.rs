use crate::AccountType;
use crate::FullAccount;

use super::Amount;

#[derive(Debug, thiserror::Error)]
pub enum Error {}

#[async_trait::async_trait]
pub trait Storage {
    async fn get_balance(
        &self,
        account: &FullAccount,
        typ: AccountType,
    ) -> Result<Vec<Amount>, Error>;
}
