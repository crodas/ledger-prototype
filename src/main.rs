use std::env;
use std::error::Error;

use csv::Trim;
use futures::StreamExt;
use ledger::{AccountId, Amount, Ledger};
use serde::{Deserialize, Serialize};

pub const AMOUNT_PRECISION: u8 = 4;

#[derive(Deserialize, Clone, Debug)]
enum Action {
    #[serde(rename = "deposit")]
    Deposit,
    #[serde(rename = "withdrawal")]
    Withdrawal,
    #[serde(rename = "dispute")]
    Dispute,
    #[serde(rename = "resolve")]
    Resolve,
    #[serde(rename = "chargeback")]
    Chargeback,
}

#[derive(Serialize, Clone, Debug)]
struct CsvAccount {
    client: AccountId,
    available: f64,
    held: f64,
    total: f64,
    locked: bool,
}

#[derive(Deserialize, Clone, Debug)]
struct CsvEntry {
    #[serde(rename = "type")]
    typ: Action,
    client: AccountId,
    tx: u32,
    #[serde(default)]
    amount: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <transactions.csv>", args[0]);
        std::process::exit(1);
    }

    let mut reader = csv::ReaderBuilder::new()
        .trim(Trim::All) // <-- trims leading & trailing whitespace
        .from_path(&args[1])?;

    let ledger = Ledger::default();

    for (line, result) in reader.deserialize::<CsvEntry>().enumerate() {
        let record = match result {
            Ok(result) => result,
            Err(err) => {
                eprintln!("Failed to parse line {}: {:?}", line, err);
                continue;
            }
        };

        let amount = match record
            .amount
            .map(|x| Amount::from_f64(x, AMOUNT_PRECISION))
            .transpose()
        {
            Ok(amount) => amount,
            Err(err) => {
                eprintln!("Error parsing the amount {err}");
                continue;
            }
        };

        let result = match record.typ {
            Action::Deposit => ledger
                .deposit(
                    record.client,
                    record.tx.to_string(),
                    amount.expect("missing amount"),
                )
                .await
                .map(|_| ()),
            Action::Withdrawal => ledger
                .withdraw(
                    record.client,
                    record.tx.to_string(),
                    amount.expect("missing amount"),
                )
                .await
                .map(|_| ()),
            Action::Dispute => ledger
                .dispute(record.client, record.tx.to_string())
                .await
                .map(|_| ()),
            Action::Resolve => ledger
                .resolve(record.client, record.tx.to_string())
                .await
                .map(|_| ()),
            Action::Chargeback => ledger
                .chargeback(record.client, record.tx.to_string())
                .await
                .map(|_| ()),
        };

        if let Err(err) = result {
            eprintln!("Error processing {:?}  with {}", record, err);
        }
    }

    let mut accounts = ledger.get_accounts().await;

    let mut wtr = csv::Writer::from_writer(std::io::stdout());

    while let Some(account) = accounts.next().await {
        let account = match account {
            Ok(account) => account,
            Err(err) => {
                eprintln!("Error reading account {:?}", err);
                continue;
            }
        };

        let balance = match ledger.get_balances(account).await {
            Ok(balance) => balance,
            Err(err) => {
                eprintln!(
                    "Error reading balance for customer {} with err {:?}",
                    account, err
                );
                continue;
            }
        };

        let record = CsvAccount {
            client: account,
            total: balance.total.to_f64(AMOUNT_PRECISION).expect("valid f64"),
            held: balance
                .disputed
                .to_f64(AMOUNT_PRECISION)
                .expect("valid f64"),
            available: balance
                .available
                .to_f64(AMOUNT_PRECISION)
                .expect("valid f64"),
            locked: (*balance.chargeback) > 0,
        };

        if let Err(err) = wtr.serialize(record) {
            eprintln!("Error serializing {:?}", err);
        }
    }

    Ok(())
}
