//! Event sources for state machine transitions

use chrono::{DateTime, Utc};

/// Event sources that trigger state transitions
#[derive(Debug, Clone)]
pub enum EventSource {
    /// Venue ACK response (order accepted)
    VenueAck {
        venue_order_id: String,
        timestamp: DateTime<Utc>,
    },

    /// Venue FILL event (partial or full)
    VenueFill {
        venue_order_id: String,
        fill_size: (i64, i16),  // (int, scale)
        fill_price: (i64, i16), // (int, scale)
        trade_id: String,
        timestamp: DateTime<Utc>,
    },

    /// Venue REJECT response
    VenueReject {
        venue_order_id: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// Coordinator timeout (leg not filled in time)
    CoordinatorTimeout {
        order_id: uuid::Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Coordinator cancellation (other leg failed)
    CoordinatorCancel {
        order_id: uuid::Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },
}
