# UTXO-Based Ledger

A simplified ledger implementation using the Unspent Transaction Output (UTXO) model, inspired by Bitcoin's architecture.

## Why UTXO?

The UTXO model is remarkably easy to reason about because it treats money like physical cash: you have discrete "coins" (UTXOs) that you spend entirely and receive change back.

### Mental Model: Cash in Envelopes

Think of each UTXO as cash in an envelope:
- **Deposit**: Someone gives you a new envelope with money
- **Withdrawal**: You open envelopes, take out exactly what you need, and get a new envelope with change
- **Balance**: Count the money in all your unopened envelopes

```mermaid
graph LR
    subgraph "Your Wallet"
        E1["Envelope 1<br/>$50"]
        E2["Envelope 2<br/>$30"]
        E3["Envelope 3<br/>$20"]
    end

    E1 --> B["Balance = $100"]
    E2 --> B
    E3 --> B
```

## Core Concepts

### 1. UTXOs Are Immutable

Once created, a UTXO never changes. It can only be:
- **Unspent**: Available for use
- **Spent**: Consumed by a transaction (forever)

This eliminates race conditions: two threads cannot spend the same UTXO because the first one to commit wins, and the second fails.

```mermaid
stateDiagram-v2
    [*] --> Unspent: Created by deposit<br/>or as change
    Unspent --> Spent: Consumed by<br/>transaction
    Spent --> [*]: Forever spent
```

### 2. Transactions Are Balanced

Every transaction must balance: inputs = outputs (except for deposits and withdrawals which create/destroy money).

```mermaid
flowchart LR
    subgraph Inputs
        U1["UTXO: $50"]
        U2["UTXO: $30"]
    end

    subgraph Transaction
        TX["Total: $80"]
    end

    subgraph Outputs
        O1["To: Alice $60"]
        O2["Change: $20"]
    end

    U1 --> TX
    U2 --> TX
    TX --> O1
    TX --> O2
```

### 3. Sub-Accounts for State Management

Instead of complex state machines, we use sub-accounts to track fund states:

```mermaid
flowchart TD
    subgraph Account["Account #1"]
        Main["Main<br/>(Available)"]
        Disputed["Disputed<br/>(Frozen)"]
        Chargeback["Chargeback<br/>(Lost)"]
    end

    Main -->|"dispute()"| Disputed
    Disputed -->|"resolve()"| Main
    Disputed -->|"chargeback()"| Chargeback
```

## Operations Explained

### Deposit

Creates money by producing a UTXO with no inputs.

```mermaid
sequenceDiagram
    participant User
    participant Ledger
    participant Storage

    User->>Ledger: deposit(account, "ref-1", $100)
    Ledger->>Ledger: Create TX with no inputs
    Ledger->>Storage: Store TX, create UTXO
    Storage-->>Ledger: Success
    Ledger-->>User: TX Hash

    Note over Storage: UTXO created:<br/>Account Main: $100
```

### Withdrawal (Exact Amount)

When you have exactly the right UTXO, it's consumed directly.

```mermaid
sequenceDiagram
    participant User
    participant Ledger
    participant Storage

    User->>Ledger: withdraw(account, "ref-2", $100)
    Ledger->>Storage: Get UTXOs >= $100
    Storage-->>Ledger: [UTXO: $100]
    Ledger->>Ledger: Create TX: $100 in, no outputs
    Ledger->>Storage: Store TX, mark UTXO spent
    Ledger-->>User: TX Hash

    Note over Storage: UTXO $100: SPENT
```

### Withdrawal (With Change)

When UTXOs don't match exactly, an exchange transaction creates change.

```mermaid
sequenceDiagram
    participant User
    participant Ledger
    participant Storage

    User->>Ledger: withdraw(account, "ref-3", $60)
    Ledger->>Storage: Get UTXOs >= $60
    Storage-->>Ledger: [UTXO: $100]

    Note over Ledger: Need change!<br/>$100 - $60 = $40

    Ledger->>Ledger: Create Exchange TX:<br/>$100 in → $60 + $40 out
    Ledger->>Storage: Store Exchange TX

    Ledger->>Ledger: Create Withdrawal TX:<br/>$60 in → no outputs
    Ledger->>Storage: Store Withdrawal TX

    Ledger-->>User: TX Hash

    Note over Storage: Old UTXO $100: SPENT<br/>New UTXO $40: UNSPENT
```

### Dispute Flow

Disputes move funds to a frozen sub-account.

```mermaid
sequenceDiagram
    participant User
    participant Ledger
    participant MainAccount as Main Account
    participant DisputedAccount as Disputed Account

    Note over MainAccount: UTXO: $100

    User->>Ledger: dispute(account, "deposit-ref")
    Ledger->>Ledger: Find original deposit TX
    Ledger->>Ledger: Create TX:<br/>Main $100 → Disputed $100

    Note over MainAccount: UTXO $100: SPENT
    Note over DisputedAccount: New UTXO: $100

    Ledger-->>User: Success
```

### Resolve vs Chargeback

```mermaid
flowchart TD
    D["Disputed<br/>UTXO: $100"]

    D -->|resolve| R["Main Account<br/>UTXO: $100<br/>(funds returned)"]
    D -->|chargeback| C["Chargeback Account<br/>UTXO: $100<br/>(funds lost)"]
```

## Why UTXO is Easy to Reason About

### 1. No Hidden State

Balance is always: `sum(unspent UTXOs)`. No need to replay history or trust running totals.

```mermaid
flowchart LR
    subgraph "Traditional Account"
        TA["Balance: $500<br/>(trust me)"]
    end

    subgraph "UTXO Account"
        U1["UTXO: $200"]
        U2["UTXO: $150"]
        U3["UTXO: $100"]
        U4["UTXO: $50"]
        SUM["Sum = $500<br/>(verifiable)"]
    end

    U1 --> SUM
    U2 --> SUM
    U3 --> SUM
    U4 --> SUM
```

### 2. Natural Concurrency

No locks needed on account balances. Each UTXO is independent.

```mermaid
flowchart TD
    subgraph "Thread 1"
        T1["Spend UTXO-A"]
    end

    subgraph "Thread 2"
        T2["Spend UTXO-B"]
    end

    subgraph "Storage"
        A["UTXO-A"]
        B["UTXO-B"]
    end

    T1 --> A
    T2 --> B

    Note["No conflict!<br/>Different UTXOs"]
```

### 3. Atomic Multi-Step Operations

Complex operations are naturally atomic because they consume and produce UTXOs in a single transaction.

```mermaid
flowchart LR
    subgraph "Single Atomic Transaction"
        I1["Input: UTXO $100"]
        I2["Input: UTXO $50"]
        TX["Transaction"]
        O1["Output: Alice $80"]
        O2["Output: Bob $50"]
        O3["Output: Change $20"]
    end

    I1 --> TX
    I2 --> TX
    TX --> O1
    TX --> O2
    TX --> O3
```

### 4. Complete Audit Trail

Every UTXO traces back to its origin through transaction hashes.

```mermaid
flowchart BT
    U["Current UTXO<br/>$40"]
    TX1["Withdrawal TX<br/>(change)"]
    TX2["Deposit TX"]

    U -->|"created by"| TX1
    TX1 -->|"spent UTXO from"| TX2
    TX2 -->|"origin"| Origin["External Deposit"]
```

## Architecture Overview

```mermaid
flowchart TB
    subgraph "Public API"
        deposit
        withdraw
        dispute
        resolve
        chargeback
        get_balances
    end

    subgraph "Ledger Core"
        TX["Transaction Builder"]
        UTXO["UTXO Selection"]
    end

    subgraph "Storage Layer"
        Store["store_tx()"]
        Get["get_unspent()"]
        Ref["get_tx_by_reference()"]
    end

    deposit --> TX
    withdraw --> UTXO
    withdraw --> TX
    dispute --> Ref
    dispute --> TX
    resolve --> Ref
    resolve --> TX
    chargeback --> Ref
    chargeback --> TX
    get_balances --> Get

    TX --> Store
    UTXO --> Get
```

## Summary

The UTXO model trades some storage efficiency for:
- **Simplicity**: Balance = sum of UTXOs
- **Safety**: No double-spending by design
- **Concurrency**: No global locks needed
- **Auditability**: Complete traceable history

It's the same model that secures billions of dollars in Bitcoin, simplified for application-level accounting.
