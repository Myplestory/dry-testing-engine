//! Venue types and adapters

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Venue type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VenueType {
    Polymarket,
    Kalshi,
    Manifold,
    Opinion,
    Gemini,
    Other(String),
}

impl VenueType {
    /// Get venue as string
    pub fn as_str(&self) -> &str {
        match self {
            VenueType::Polymarket => "polymarket",
            VenueType::Kalshi => "kalshi",
            VenueType::Manifold => "manifold",
            VenueType::Opinion => "opinion",
            VenueType::Gemini => "gemini",
            VenueType::Other(name) => name.as_str(),
        }
    }
    
    /// Get array index for fast path routing (returns None for Other)
    pub fn array_index(&self) -> Option<usize> {
        match self {
            VenueType::Polymarket => Some(0),
            VenueType::Kalshi => Some(1),
            VenueType::Manifold => Some(2),
            VenueType::Opinion => Some(3),
            VenueType::Gemini => Some(4),
            VenueType::Other(_) => None,
        }
    }
}

impl FromStr for VenueType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "polymarket" => Ok(VenueType::Polymarket),
            "kalshi" => Ok(VenueType::Kalshi),
            "manifold" => Ok(VenueType::Manifold),
            "opinion" => Ok(VenueType::Opinion),
            "gemini" => Ok(VenueType::Gemini),
            other => Ok(VenueType::Other(other.to_string())),
        }
    }
}

impl From<VenueType> for String {
    fn from(venue: VenueType) -> Self {
        venue.as_str().to_string()
    }
}

/// Venue response from order submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VenueResponse {
    /// Order acknowledged
    Ack {
        venue_order_id: String,
    },
    
    /// Order filled (partial or full)
    Fill {
        venue_order_id: String,
        fill_size: (i64, i16),  // (int, scale)
        fill_price: (i64, i16), // (int, scale)
        trade_id: String,
    },
    
    /// Order rejected
    Reject {
        venue_order_id: Option<String>,
        reason: String,
    },
}

/// Venue event (from WebSocket or polling)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueEvent {
    /// Venue type
    pub venue: VenueType,
    
    /// Venue order ID (if available)
    pub venue_order_id: Option<String>,
    
    /// Client order ID (if available, for pre-ACK routing)
    pub client_order_id: Option<String>,
    
    /// Event type
    pub event_type: VenueEventType,
    
    /// Event timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Venue event type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VenueEventType {
    Ack,
    Fill {
        fill_size: (i64, i16),
        fill_price: (i64, i16),
        trade_id: String,
    },
    Reject {
        reason: String,
    },
    Cancel,
}

