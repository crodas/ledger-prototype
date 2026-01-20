Create a project skeleton with crates. Add a ledger crate with methods for deposit, withdrawal and move funds. Don't implement anything, just add todo!()
Add also a main.rs to read a list of transactions using the csv crate

--

See transaction.rs, implement the id() method. Let it be the Sha256(Sha256(Inputs) + Sha256(Outputs))

--

Extend the transaction and add a timestamp. Add a microsecond timestamp. Also add a String reference. Make these two fields part of the ID hash

--

Change the transaction, if None is passed as a timestamp use the system microsecond
