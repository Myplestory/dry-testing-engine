//! Order router for venue event routing

use crate::types::order::Order;
use crate::types::venue::{VenueEvent, VenueType};
use dashmap::DashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Routes venue events to correct order_id
pub struct OrderRouter {
    /// Primary: venue_order_id → order_id (after ACK)
    venue_order_map: Arc<DashMap<(VenueType, String), Uuid>>,
    
    /// Fallback: client_order_id → order_id (pre-ACK race condition)
    client_order_map: Arc<DashMap<(VenueType, String), Uuid>>,
}

impl OrderRouter {
    /// Create a new order router
    pub fn new() -> Self {
        Self {
            venue_order_map: Arc::new(DashMap::new()),
            client_order_map: Arc::new(DashMap::new()),
        }
    }
    
    /// Register order for routing
    pub fn register_order(&self, order: &Order) {
        // Register by client_order_id (always available)
        self.client_order_map.insert(
            (order.venue.clone(), order.client_order_id.clone()),
            order.id,
        );
        
        // Register by venue_order_id (if available from ACK)
        if let Some(ref venue_order_id) = order.venue_order_id {
            self.venue_order_map.insert(
                (order.venue.clone(), venue_order_id.clone()),
                order.id,
            );
        }
    }
    
    /// Update venue_order_id mapping (called on ACK)
    pub fn update_venue_order_id(
        &self,
        venue: VenueType,
        client_order_id: &str,
        venue_order_id: &str,
    ) -> Option<Uuid> {
        // Get order_id from client_order_id
        let order_id = self.client_order_map.get(&(venue.clone(), client_order_id.to_string()))?;
        let order_id = *order_id.value();
        
        // Register venue_order_id mapping
        self.venue_order_map.insert(
            (venue, venue_order_id.to_string()),
            order_id,
        );
        
        Some(order_id)
    }
    
    /// Route venue event to order_id
    pub fn route_event(&self, event: &VenueEvent) -> Option<Uuid> {
        // Try venue_order_id first (preferred, after ACK)
        if let Some(ref venue_order_id) = event.venue_order_id {
            if let Some(order_id) = self.venue_order_map.get(&(event.venue.clone(), venue_order_id.clone())) {
                return Some(*order_id.value());
            }
        }
        
        // Fallback to client_order_id (pre-ACK race condition)
        if let Some(ref client_order_id) = event.client_order_id {
            if let Some(order_id) = self.client_order_map.get(&(event.venue.clone(), client_order_id.clone())) {
                return Some(*order_id.value());
            }
        }
        
        None
    }
    
    /// Remove order from routing
    pub fn unregister_order(&self, order: &Order) {
        if let Some(ref venue_order_id) = order.venue_order_id {
            self.venue_order_map.remove(&(order.venue.clone(), venue_order_id.clone()));
        }
        self.client_order_map.remove(&(order.venue.clone(), order.client_order_id.clone()));
    }
}

impl Default for OrderRouter {
    fn default() -> Self {
        Self::new()
    }
}

