# Investigation and Benchmarking Plan

## Part 1: Failure Investigation

### Possibility 1: Error Before Sleep (`.await?` operators)

**Investigation Points:**

1. **`active_pairs.first().ok_or(...)?` (Line 130-132)**
   - **Issue**: If `list_active_pairs(1)` returns empty, this fails
   - **Why it might fail**: Database state differs when test runs
   - **Check**: Add logging before this line to see if `active_pairs` is empty
   - **Fix**: Use same pattern as other tests: `if active_pairs.is_empty() { return Ok(()); }`

2. **`DryTestingEngine::new().await?` (Line 147)**
   - **Issue**: Engine initialization might fail
   - **Why it might fail**: Database connection pool exhaustion, missing env vars
   - **Check**: Add error logging to see exact failure point
   - **Difference**: Other tests create engine BEFORE setup, this creates AFTER

3. **`generator.generate_from_pair_id(...).await?` (Line 152)**
   - **Issue**: Intent generation might fail
   - **Why it might fail**: Database query timeout, missing contract specs, invalid outcome mapping
   - **Check**: Add detailed error logging in `generate_from_pair_id`
   - **Difference**: Other tests handle errors with `match`, this uses `?`

4. **`engine.router().enqueue_intent(intent).await?` (Line 157)**
   - **Issue**: Enqueue might fail
   - **Why it might fail**: Coordinator error, database write failure
   - **Check**: Add error logging in `enqueue_intent` and `coordinator.enqueue_intent`

**Recommended Fixes:**
- Replace `.ok_or(...)?` with graceful empty check
- Add error context logging at each `?` operator
- Wrap critical sections in `match` blocks for better error visibility

### Possibility 2: Test Setup/State Difference

**Key Differences Identified:**

1. **Order of Operations:**
   - `test_complete_execution_flow`: Engine → Generator → Setup → Generate
   - `test_single_intent_execution`: Setup → Engine → Generator → Generate
   - `test_batch_intent_execution`: Engine → Generator → Setup → Generate

2. **Error Handling:**
   - `test_complete_execution_flow`: Uses `match` for generation errors, continues on failure
   - `test_single_intent_execution`: Uses `?` operator, fails immediately
   - `test_batch_intent_execution`: Uses `match` for batch errors, continues on failure

3. **Empty Pair Handling:**
   - `test_complete_execution_flow`: Checks `if active_pairs.is_empty()` and returns `Ok(())`
   - `test_single_intent_execution`: Uses `.ok_or()` which returns error
   - `test_batch_intent_execution`: Checks `if active_pairs.is_empty()` and returns `Ok(())`

**Investigation Steps:**
1. Check if database state is different when `test_single_intent_execution` runs
2. Verify if parallel test execution causes race conditions
3. Add database state logging before each test

### Possibility 3: Missing Error Output

**Issue**: Terminal output doesn't show actual error message for `test_single_intent_execution`

**Investigation Steps:**
1. Run test with `--nocapture` flag: `cargo test --test end_to_end test_single_intent_execution -- --nocapture`
2. Check if error is being swallowed by `?` operator
3. Add explicit error logging before each `?` operator
4. Use `eprintln!` for errors instead of relying on test framework

**Recommended Fix:**
- Add explicit error handling with logging:
```rust
let result = generator.generate_from_pair_id(pair_id, customer_id, strategy_id).await;
match result {
    Ok(intent) => { /* ... */ }
    Err(e) => {
        eprintln!("❌ Failed to generate intent: {:?}", e);
        return Err(e.into());
    }
}
```

## Part 2: Benchmarking Implementation Plan

### Phase 1: Add Timing Instrumentation

**1. Create Timing Module** (`src/core/timing.rs`):
```rust
use std::time::{Instant, Duration};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ExecutionTiming {
    pub intent_enqueue_time: Duration,
    pub order_creation_time: Duration,
    pub db_write_time: Duration,
    pub venue_submission_time: Duration,
    pub venue_ack_time: Duration,
    pub venue_fill_time: Duration,
    pub state_transition_times: Vec<Duration>,
    pub coordinator_update_times: Vec<Duration>,
    pub total_execution_time: Duration,
}

pub struct TimingCollector {
    timings: Arc<Mutex<HashMap<uuid::Uuid, ExecutionTiming>>>,
}
```

**2. Instrument Key Points:**

- **Intent Enqueue** (`execution/router.rs`):
  ```rust
  let start = Instant::now();
  self.coordinator.enqueue_intent(intent).await?;
  let enqueue_time = start.elapsed();
  ```

- **Order Creation** (`coordinator/coordinator.rs`):
  ```rust
  let start = Instant::now();
  // ... order creation ...
  let order_creation_time = start.elapsed();
  ```

- **Database Write** (`coordinator/coordinator.rs`):
  ```rust
  let start = Instant::now();
  let order_a_id = self.db_writer.write_order(&order_a).await?;
  let db_write_time = start.elapsed();
  ```

- **Venue Submission** (`execution/executor.rs`):
  ```rust
  let start = Instant::now();
  let response = self.venue_adapter.submit_order(...).await?;
  let submission_time = start.elapsed();
  ```

- **State Transitions** (`state_machine/machine.rs`):
  ```rust
  let start = Instant::now();
  // ... transition logic ...
  let transition_time = start.elapsed();
  ```

- **Venue Latency** (`venue/simulator.rs`):
  ```rust
  let ack_start = Instant::now();
  sleep(Duration::from_millis(self.ack_latency_ms)).await;
  let ack_latency = ack_start.elapsed();
  ```

### Phase 2: Add Benchmark Reporting

**1. Test-Level Benchmarking** (`tests/integration/end_to_end.rs`):
```rust
#[tokio::test]
async fn test_single_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    let test_start = Instant::now();
    
    // ... test setup ...
    
    let setup_time = setup_start.elapsed();
    println!("⏱️  Setup time: {:?}", setup_time);
    
    // ... intent generation ...
    
    let generation_time = generation_start.elapsed();
    println!("⏱️  Intent generation time: {:?}", generation_time);
    
    // ... enqueue ...
    
    let enqueue_time = enqueue_start.elapsed();
    println!("⏱️  Enqueue time: {:?}", enqueue_time);
    
    // Wait for execution
    let execution_start = Instant::now();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let execution_time = execution_start.elapsed();
    
    println!("\n📊 Execution Benchmark:");
    println!("   Setup: {:?}", setup_time);
    println!("   Generation: {:?}", generation_time);
    println!("   Enqueue: {:?}", enqueue_time);
    println!("   Execution: {:?}", execution_time);
    println!("   Total: {:?}", test_start.elapsed());
    
    Ok(())
}
```

**2. Component-Level Benchmarking** (via logging):
- Add `tracing::instrument` to key functions
- Use `tracing` spans to measure async operations
- Log timing at each phase

**3. Per-Event Benchmarking**:
- Log timing for each state transition
- Log timing for each venue response
- Log timing for each database write
- Aggregate at end of test

### Phase 3: Benchmark Output Format

**Example Output:**
```
📊 Execution Benchmark for Intent: a90b76de-d9ba-4554-a6ab-f22743de698c
   Intent Enqueue:        0.123ms
   Order Creation:        0.045ms
   DB Write (Order A):    2.456ms
   DB Write (Order B):    2.389ms
   Venue Submission (A):  0.234ms
   Venue Submission (B):  0.198ms
   Venue ACK (A):         10.123ms (simulated)
   Venue ACK (B):         10.089ms (simulated)
   State Transition (A):  0.012ms
   State Transition (B):  0.011ms
   Venue Fill (A):        50.234ms (simulated)
   Venue Fill (B):        50.198ms (simulated)
   Coordinator Update:    0.034ms
   Total Execution:       65.234ms
```

## Implementation Priority

1. **Immediate**: Fix error handling in `test_single_intent_execution` (use same pattern as other tests)
2. **High**: Add error logging to identify actual failure point
3. **Medium**: Add basic timing instrumentation to tests
4. **Low**: Add comprehensive component-level benchmarking

