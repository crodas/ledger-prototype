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

Add more tests. Cover the new functions added (Ledger::dispute, asume the user always have more or enough credit to cover their dispute).

Also test transactions with duplicate references and get_tx_by_reference

Add a tests where user have 3 deposits (a: 10, b: 5), withdraw 11, then deposit (c: 4), then disput b. The test will check the dispute work after the utxos has been shuffled.

--

Update the tests and use get_balance to check the ledger is behaving as expected


--

Write a test for both the in memory implemetnation of get accounts and the ledger version (which does not return sub accounts). Test that the memory returns in order.

Create many operations before writing each tests, the idea is to get multipel accounts. Add a loop or something so the code is short.

--

Migrate the tests in the in memory implementation. Move it to the storage/mod.rs, behind a macro. Make the macro take an instance of the impl Storage, so all the tests are inherited and easily added to any storage implementation.

The usage should be 

```
storage_test!(InMemory::default());
```

--

Fix documentation. Fix any typos. Document why and every external function. Deny by clippy anything missing.

Document also the ledger in a markdown with mermaid. Focus on the UXTO and why it is an easy model to reason about.
