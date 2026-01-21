use std::env;
use std::error::Error;
use std::fs::File;

use csv::Reader;
use ledger::Ledger;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <transactions.csv>", args[0]);
        std::process::exit(1);
    }

    let file = File::open(&args[1])?;
    let mut reader = Reader::from_reader(file);

    let _ledger = Ledger::default();

    for result in reader.records() {
        let record = result?;
        // TODO: parse record and call ledger methods
        println!("{:?}", record);
    }

    Ok(())
}
