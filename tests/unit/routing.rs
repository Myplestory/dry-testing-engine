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
    let mut prev_seq = 0u64;
    for _ in 0..10 {
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

