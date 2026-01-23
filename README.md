# Dry Testing Engine

A high-performance dry testing engine for execution lanes and order state machine. Designed for testing and benchmarking arbitrage execution with low-latency guarantees.

## Architecture

- **Execution Lanes**: Venue-based routing with bounded queues
- **Order State Machine**: Lifecycle management with state transitions
- **Arbitrage Coordinator**: Multi-leg execution with atomicity guarantees
- **Venue Simulator**: Configurable latency and response simulation

## Features

- ✅ Lock-free data structures for high performance
- ✅ Async/await for non-blocking execution
- ✅ Batched database writes for efficiency
- ✅ Venue-agnostic design (extensible to any venue)
- ✅ Recovery from database on startup
- ✅ Comprehensive error handling

## Setup

1. Set environment variables:
   ```bash
   DATABASE_URL=postgresql://user:password@localhost/dbname
   ```

2. Run the engine:
   ```bash
   cargo run
   ```

## Testing

```bash
# Run unit tests
cargo test

# Run benchmarks
cargo bench
```

## Project Structure

```
src/
├── main.rs              # CLI entry point
├── lib.rs               # Library root
├── core/                # Core utilities (sequence generator, etc.)
├── execution/           # Execution lanes and routing
├── state_machine/       # Order state machine
├── coordinator/         # Arbitrage execution coordinator
├── routing/             # Order router for venue events
├── venue/               # Venue adapters and simulators
├── db/                  # Database integration
├── recovery/            # Recovery from database
└── types/               # Core types and errors
```

## Performance Targets

- Enqueue latency: <1μs
- Routing latency: <10μs
- Throughput: >10,000 intents/sec per lane
- Recovery time: <5s

## License

MIT OR Apache-2.0

