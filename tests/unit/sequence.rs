//! Unit tests for sequence generator

use dry_testing_engine::core::SequenceGenerator;
use std::sync::Arc;
use std::thread;

#[test]
fn test_sequence_monotonicity() {
    let gen = SequenceGenerator::new();
    let mut prev = 0u64;
    
    for _ in 0..1000 {
        let next = gen.next();
        assert!(next > prev, "Sequence must be monotonically increasing");
        prev = next;
    }
}

#[test]
fn test_sequence_thread_safety() {
    let gen = Arc::new(SequenceGenerator::new());
    let mut handles = vec![];

    // Spawn 10 threads, each generating 1000 sequence numbers
    for _ in 0..10 {
        let gen = gen.clone();
        let handle = thread::spawn(move || {
            let mut numbers = Vec::new();
            for _ in 0..1000 {
                numbers.push(gen.next());
            }
            numbers
        });
        handles.push(handle);
    }

    // Collect all sequence numbers
    let mut all_numbers: Vec<u64> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();

    // Verify all numbers are unique
    all_numbers.sort();
    for i in 1..all_numbers.len() {
        assert_ne!(
            all_numbers[i],
            all_numbers[i - 1],
            "All sequence numbers must be unique"
        );
    }
}

#[test]
fn test_sequence_with_initial() {
    let gen = SequenceGenerator::with_initial(100);
    assert_eq!(gen.next(), 101);
    assert_eq!(gen.next(), 102);
}

#[test]
fn test_sequence_reset() {
    let gen = SequenceGenerator::new();
    let first = gen.next();
    let second = gen.next();
    
    assert!(second > first);
    
    gen.reset();
    let after_reset = gen.next();
    assert_eq!(after_reset, 1, "After reset, sequence should start at 1");
}

