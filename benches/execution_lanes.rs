use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dry_testing_engine::execution::lane::ExecutionLane;
use dry_testing_engine::types::execution::{ArbExecutionIntent, ExecutionPriority};
use dry_testing_engine::types::venue::VenueType;
use uuid::Uuid;
use chrono::Utc;

fn bench_enqueue(c: &mut Criterion) {
    let lane = ExecutionLane::new(VenueType::Polymarket, 1024);
    let intent = ArbExecutionIntent {
        intent_id: Uuid::new_v4(),
        sequence_number: 0,
        customer_id: Uuid::new_v4(),
        strategy_id: Uuid::new_v4(),
        pair_id: Uuid::new_v4(),
        leg_a: Default::default(), // TODO: Implement Default
        leg_b: Default::default(),
        expected_profit_int: 1000,
        expected_profit_scale: 6,
        expected_roi_bps: 50,
        outcome_mapping: Default::default(),
        priority: ExecutionPriority::Opportunity,
        detected_at: Utc::now(),
        enqueued_at: None,
    };
    
    c.bench_function("enqueue", |b| {
        b.iter(|| {
            let _ = lane.enqueue(black_box(intent.clone()));
        });
    });
}

criterion_group!(benches, bench_enqueue);
criterion_main!(benches);

