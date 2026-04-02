# Dry Testing Engine

A Rust execution engine for dry-run backtesting of order management system (OMS/EMS) flows against simulated prediction market venues. Built to validate order lifecycle correctness, fill state machine transitions, and multi-leg arbitrage coordination without touching real capital.

Archived as [PolyEdge](https://polyedge.trade) infrastructure exploration. Published as a reference iterative implementation.

## What It Does

- Generates deterministic trade intents from configurable market parameters
- Routes intents through venue-specific execution lanes with bounded queues
- Validates every order state transition through a strict fill lifecycle state machine (`Pending → Submitted → PartialFill → Filled | Cancelled | Rejected`)
- Simulates venue latency and response characteristics (configurable distributions)
- Executes multi-leg arbitrage with coordinated cancellation via `tokio::select!` + task abort
- Writes results to PostgreSQL with batched, non-blocking retries and UUID-based ordering
- Uses scaled-integer (fixed-point) arithmetic throughout — no floating point in financial paths

## Architecture

IntentGenerator (deterministic, seeded)
│
▼
OrderRouter
│
├── VenueExecutor (Polymarket)  ─── bounded queue ─── VenueSimulator
├── VenueExecutor (Kalshi)      ─── bounded queue ─── VenueSimulator
└── VenueExecutor (...)         ─── bounded queue ─── VenueSimulator
│
▼
StateMachine (fill lifecycle validation)
│
▼
BatchWriter (PostgreSQL, non-blocking retries)



### Key Modules

| Module | Description |
|--------|-------------|
| `execution/` | `DryTestingEngine` — orchestrates the full run lifecycle |
| `intent_generator/` | Generates trade intents from `TestConfig` with atomic sequence IDs |
| `state_machine/` | Enforces valid order state transitions, rejects illegal moves |
| `coordinator/` | Multi-leg arbitrage coordinator with `tokio::select!` cancellation |
| `routing/` | Routes intents to venue-specific execution lanes |
| `venue/` | Venue simulators with configurable latency distributions |
| `db/` | Batched PostgreSQL writes with retry logic |
| `recovery/` | Reconstructs engine state from database on startup |
| `core/` | Sequence generator (`AtomicU64`), shared utilities |
| `types/` | Domain types, error hierarchy |

## Setup

```bash
# PostgreSQL with the test schema
psql -f testdbschema.txt

# Set connection string
export DATABASE_URL=postgresql://user:password@localhost/dry_testing

# Run
cargo run

# Tests
cargo test

# Benchmarks
cargo bench
Performance Targets
Metric	Target
Enqueue latency	< 1 us
Routing latency	< 10 us
Throughput	> 10,000 intents/sec per lane
Recovery time	< 5s
Design Decisions
Scaled integers over floats. All prices, sizes, and notional values use rust_decimal / fixed-point representations. No IEEE 754 floating point in financial computation paths.

Direct task spawning over actor model. Each venue executor is a tokio::spawn'd task with a bounded channel. tokio::select! on the coordinator triggers instant abort of all legs when one fails.

Batched writes over per-event persistence. Order events accumulate in memory and flush in configurable batches. Non-blocking retries with UUID ordering guarantee idempotent replay.

DashMap for concurrent state. Lock-free concurrent hashmaps for in-flight order tracking. No Mutex on the hot path.

Tests

tests/
├── end_to_end.rs          # Full pipeline: generate → route → execute → persist
├── state_machine.rs       # Fill lifecycle transition validation
├── intent_generator.rs    # Deterministic generation, batch sequencing
├── routing.rs             # Venue routing, monotonic sequence ordering
├── venue_simulator.rs     # Latency distribution, response simulation
├── fixed_point.rs         # Scaled integer arithmetic correctness
├── sequence.rs            # AtomicU64 sequence generator concurrency
├── types.rs               # Domain type construction and validation
├── connection_tracker.rs  # DB pool connection lifecycle
└── integration/
    └── end_to_end.rs      # Full DB-backed integration test
License
ALL RIGHTS RESERVED
