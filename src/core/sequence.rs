//! Sequence number generator for ordering guarantees

use std::sync::atomic::{AtomicU64, Ordering};

/// Thread-safe sequence number generator
#[derive(Debug)]
pub struct SequenceGenerator {
    counter: AtomicU64,
}

impl SequenceGenerator {
    /// Create a new sequence generator starting at 0
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
    
    /// Create a new sequence generator starting at a specific value
    pub fn with_initial(initial: u64) -> Self {
        Self {
            counter: AtomicU64::new(initial),
        }
    }
    
    /// Get the next sequence number (monotonically increasing)
    pub fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
    
    /// Get the current sequence number without incrementing
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }
}

impl Default for SequenceGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    
    #[test]
    fn test_sequence_monotonicity() {
        let gen = SequenceGenerator::new();
        let mut prev = gen.next();
        
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
            assert_ne!(all_numbers[i], all_numbers[i - 1], "All sequence numbers must be unique");
        }
    }
}

