use std::env;
use std::error::Error;
use std::fs::File;

use csv::{Reader, Trim};
use ledger::{AccountId, Ledger, Reference};
use serde::Deserialize;

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

#[derive(Deserialize, Clone, Debug)]
struct CsvEntry {
    #[serde(rename = "type")]
    typ: Action,
    client: AccountId,
    tx: Reference,
    #[serde(default)]
    amount: Option<f64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <transactions.csv>", args[0]);
        std::process::exit(1);
    }

    let mut reader = csv::ReaderBuilder::new()
        .trim(Trim::All) // <-- trims leading & trailing whitespace
        .from_path(&args[1])?;

    let _ledger = Ledger::default();

    for (line, result) in reader.deserialize::<CsvEntry>().enumerate() {
        let record = match result {
            Ok(result) => result,
            Err(err) => {
                println!("Failed to parse line {}: {:?}", line, err);
                continue;
            }
        };

        // TODO: parse record and call ledger methods
        println!("{:?}", record);
    }

    Ok(())
}
