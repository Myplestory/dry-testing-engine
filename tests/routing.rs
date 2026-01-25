//! Unit tests for routing components

use dry_testing_engine::core::SequenceGenerator;
use dry_testing_engine::routing::OrderRouter;
use dry_testing_engine::types::order::Order;
use dry_testing_engine::types::venue::VenueType;
use uuid::Uuid;
use chrono::Utc;


#[tokio::test]
async fn test_router_sequence_monotonicity() {
    // Test sequence generator directly (router uses it internally)
    let gen = SequenceGenerator::new();
    let mut prev_seq = gen.next(); // Get first value before loop
    
    for _ in 0..9 {
        let seq = gen.next();
        assert!(seq > prev_seq, "Sequence numbers must be monotonically increasing");
        prev_seq = seq;
    }
}

#[tokio::test]
async fn test_order_router_register_and_lookup() {
    let router = OrderRouter::new();
    let order = create_test_order();
    
    // Register order
    router.register_order(&order);
    
    // Lookup by client order ID
    let found = router.lookup_by_client_order_id(&VenueType::Polymarket, &order.client_order_id);
    assert!(found.is_some());
    assert_eq!(found.unwrap(), order.id);
}

#[tokio::test]
async fn test_order_router_venue_order_id_update() {
    let router = OrderRouter::new();
    let order = create_test_order();
    
    // Register order (initially without venue_order_id)
    router.register_order(&order);
    
    // Update with venue_order_id (simulating ACK)
    let venue_order_id = "venue_456".to_string();
    let order_id = router.update_venue_order_id(
        VenueType::Polymarket,
        &order.client_order_id,
        &venue_order_id,
    );
    
    assert_eq!(order_id, Some(order.id));
    
    // Now lookup by venue_order_id should work via route_event
    // (We can't directly test lookup_by_venue_order_id as it's private, but route_event uses it)
    // The update_venue_order_id already verified the mapping was created
}

#[tokio::test]
async fn test_order_router_lookup_not_found() {
    let router = OrderRouter::new();
    let client_order_id = "nonexistent".to_string();
    
    let found = router.lookup_by_client_order_id(&VenueType::Polymarket, &client_order_id);
    assert!(found.is_none(), "Non-existent order should return None");
}

#[tokio::test]
async fn test_order_router_route_event_by_venue_order_id() {
    let router = OrderRouter::new();
    let order = create_test_order();
    
    // Register order
    router.register_order(&order);
    
    // Update with venue_order_id
    let venue_order_id = "venue_789".to_string();
    router.update_venue_order_id(
        VenueType::Polymarket,
        &order.client_order_id,
        &venue_order_id,
    );
    
    // Create venue event with venue_order_id
    let event = dry_testing_engine::types::venue::VenueEvent {
        venue: VenueType::Polymarket,
        venue_order_id: Some(venue_order_id.clone()),
        client_order_id: None,
        event_type: dry_testing_engine::types::venue::VenueEventType::Ack,
        timestamp: Utc::now(),
    };
    
    // Route event should find order by venue_order_id
    let routed_order_id = router.route_event(&event);
    assert_eq!(routed_order_id, Some(order.id));
}

#[tokio::test]
async fn test_order_router_route_event_by_client_order_id() {
    let router = OrderRouter::new();
    let order = create_test_order();
    
    // Register order (without venue_order_id yet)
    router.register_order(&order);
    
    // Create venue event with only client_order_id (pre-ACK scenario)
    let event = dry_testing_engine::types::venue::VenueEvent {
        venue: VenueType::Polymarket,
        venue_order_id: None,
        client_order_id: Some(order.client_order_id.clone()),
        event_type: dry_testing_engine::types::venue::VenueEventType::Ack,
        timestamp: Utc::now(),
    };
    
    // Route event should find order by client_order_id (fallback)
    let routed_order_id = router.route_event(&event);
    assert_eq!(routed_order_id, Some(order.id));
}

#[tokio::test]
async fn test_order_router_route_event_prefers_venue_order_id() {
    let router = OrderRouter::new();
    let order = create_test_order();
    
    // Register order
    router.register_order(&order);
    
    // Update with venue_order_id
    let venue_order_id = "venue_999".to_string();
    router.update_venue_order_id(
        VenueType::Polymarket,
        &order.client_order_id,
        &venue_order_id,
    );
    
    // Create event with both IDs (should prefer venue_order_id)
    let event = dry_testing_engine::types::venue::VenueEvent {
        venue: VenueType::Polymarket,
        venue_order_id: Some(venue_order_id.clone()),
        client_order_id: Some(order.client_order_id.clone()),
        event_type: dry_testing_engine::types::venue::VenueEventType::Fill {
            fill_size: (1000000, 6),
            fill_price: (5000, 2),
            trade_id: "trade_123".to_string(),
        },
        timestamp: Utc::now(),
    };
    
    // Route event should find order by venue_order_id (preferred)
    let routed_order_id = router.route_event(&event);
    assert_eq!(routed_order_id, Some(order.id));
}

#[tokio::test]
async fn test_order_router_route_event_not_found() {
    let router = OrderRouter::new();
    
    // Create event for non-existent order
    let event = dry_testing_engine::types::venue::VenueEvent {
        venue: VenueType::Polymarket,
        venue_order_id: Some("nonexistent_venue_id".to_string()),
        client_order_id: Some("nonexistent_client_id".to_string()),
        event_type: dry_testing_engine::types::venue::VenueEventType::Ack,
        timestamp: Utc::now(),
    };
    
    // Route event should return None
    let routed_order_id = router.route_event(&event);
    assert_eq!(routed_order_id, None);
}

#[tokio::test]
async fn test_order_router_unregister_order() {
    let router = OrderRouter::new();
    let mut order = create_test_order();
    
    // Register order
    router.register_order(&order);
    
    // Update with venue_order_id
    let venue_order_id = "venue_unregister".to_string();
    router.update_venue_order_id(
        VenueType::Polymarket,
        &order.client_order_id,
        &venue_order_id,
    );
    
    // Verify order is registered
    let found = router.lookup_by_client_order_id(&VenueType::Polymarket, &order.client_order_id);
    assert!(found.is_some());
    
    // Unregister order
    order.venue_order_id = Some(venue_order_id);
    router.unregister_order(&order);
    
    // Verify order is no longer found
    let found_after = router.lookup_by_client_order_id(&VenueType::Polymarket, &order.client_order_id);
    assert!(found_after.is_none(), "Order should be unregistered");
}

fn create_test_order() -> Order {
    Order {
        id: Uuid::new_v4(),
        customer_id: Uuid::new_v4(),
        strategy_id: Uuid::new_v4(),
        pair_id: Uuid::new_v4(),
        leg: dry_testing_engine::types::order::OrderLeg::A,
        venue: VenueType::Polymarket,
        venue_market_id: "market_123".to_string(),
        venue_outcome_id: "outcome_123".to_string(),
        client_order_id: "client_123".to_string(),
        venue_order_id: None,
        side: dry_testing_engine::types::order::OrderSide::Buy,
        limit_price_int: 5000,
        price_scale: 2,
        size_int: 1000000,
        size_scale: 6,
        status: dry_testing_engine::types::order::OrderStatus::Pending,
        filled_size_int: 0,
        filled_size_scale: 6,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

