//! Execution timing tracker for benchmarking

use std::time::{Duration, Instant};
use uuid::Uuid;

/// Timing data for a single execution intent
#[derive(Debug, Clone)]
pub struct ExecutionTiming {
    /// Intent ID
    pub intent_id: Uuid,

    /// Start time when intent is enqueued
    pub start_time: Instant,

    /// End time when execution completes
    pub end_time: Option<Instant>,

    /// Enqueue phase duration
    pub enqueue_time: Option<Duration>,

    /// Order creation phase duration
    pub order_creation_time: Option<Duration>,

    /// Database write time for Order A
    pub db_write_time_a: Option<Duration>,

    /// Database write time for Order B
    pub db_write_time_b: Option<Duration>,

    /// Venue submission time for Leg A
    pub submission_time_a: Option<Duration>,

    /// Venue submission time for Leg B
    pub submission_time_b: Option<Duration>,

    /// Venue response processing time for Leg A
    pub process_time_a: Option<Duration>,

    /// Venue response processing time for Leg B
    pub process_time_b: Option<Duration>,

    /// State transition time for Leg A
    pub state_transition_time_a: Option<Duration>,

    /// State transition time for Leg B
    pub state_transition_time_b: Option<Duration>,

    /// Venue FILL latency for Leg A (from simulator callback)
    pub fill_latency_a: Option<Duration>,

    /// Venue FILL latency for Leg B (from simulator callback)
    pub fill_latency_b: Option<Duration>,

    /// Coordinator completion phase duration
    pub coordinator_completion_time: Option<Duration>,
}

impl ExecutionTiming {
    /// Create a new timing tracker
    pub fn new(intent_id: Uuid) -> Self {
        Self {
            intent_id,
            start_time: Instant::now(),
            end_time: None,
            enqueue_time: None,
            order_creation_time: None,
            db_write_time_a: None,
            db_write_time_b: None,
            submission_time_a: None,
            submission_time_b: None,
            process_time_a: None,
            process_time_b: None,
            state_transition_time_a: None,
            state_transition_time_b: None,
            fill_latency_a: None,
            fill_latency_b: None,
            coordinator_completion_time: None,
        }
    }

    /// Calculate total execution time
    pub fn total_execution_time(&self) -> Option<Duration> {
        self.end_time.map(|end| end.duration_since(self.start_time))
    }

    /// Format timing summary as a string
    pub fn format_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "📊 Execution Benchmark for Intent: {}",
            self.intent_id
        ));

        if let Some(dur) = self.enqueue_time {
            lines.push(format!("   Enqueue: {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.order_creation_time {
            lines.push(format!("   Order Creation: {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.db_write_time_a {
            lines.push(format!("   DB Write (Order A): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.db_write_time_b {
            lines.push(format!("   DB Write (Order B): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.submission_time_a {
            lines.push(format!("   Venue Submission (Leg A): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.submission_time_b {
            lines.push(format!("   Venue Submission (Leg B): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.process_time_a {
            lines.push(format!("   Venue ACK (Leg A): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.process_time_b {
            lines.push(format!("   Venue ACK (Leg B): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.state_transition_time_a {
            lines.push(format!("   State Transition (Leg A): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.state_transition_time_b {
            lines.push(format!("   State Transition (Leg B): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.fill_latency_a {
            lines.push(format!("   Venue FILL (Leg A): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.fill_latency_b {
            lines.push(format!("   Venue FILL (Leg B): {}ms", dur.as_millis()));
        }

        if let Some(dur) = self.coordinator_completion_time {
            lines.push(format!("   Coordinator Completion: {}ms", dur.as_millis()));
        }

        if let Some(total) = self.total_execution_time() {
            lines.push("   ──────────────────────────────────────".to_string());
            lines.push(format!("   Total Execution Time: {}ms", total.as_millis()));
        }

        lines.join("\n")
    }
}

