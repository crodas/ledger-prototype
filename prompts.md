Create a project skeleton with crates. Add a ledger crate with methods for deposit, withdrawal and move funds. Don't implement anything, just add todo!()
Add also a main.rs to read a list of transactions using the csv crate

--

See transaction.rs, implement the id() method. Let it be the Sha256(Sha256(Inputs) + Sha256(Outputs))

--

Extend the transaction and add a timestamp. Add a microsecond timestamp. Also add a String reference. Make these two fields part of the ID hash

--

Change the transaction, if None is passed as a timestamp use the system microsecond

--

Add unit tests for the ledger using the in memory storage. Add also tests for the in memory tests. Focus on the correctness of the storage and that an spent utxo cannot be spent twice. Also check the ifs branches inside the in-memory storage impl.

In the ledger tests focus on deposit and withdrawal. Focus on withdrawal and check that overwithdrawl is not possible.

Don't use unwrap, instead use expect with a meaningful error message

--
