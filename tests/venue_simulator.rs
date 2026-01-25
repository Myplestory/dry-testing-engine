//! Unit tests for venue simulator

use dry_testing_engine::types::order::{Order, OrderSide};
use dry_testing_engine::types::venue::VenueType;
use dry_testing_engine::venue::simulator::VenueSimulator;
use dry_testing_engine::venue::adapter::VenueAdapter;
use std::time::Duration;
use uuid::Uuid;
use chrono::Utc;

#[tokio::test]
async fn test_simulator_ack() {
    let simulator = VenueSimulator::new(
        VenueType::Polymarket,
        10, // 10ms ACK latency
        100, // 100ms fill latency
        0.0, // No rejections
        0.0, // No fills
    );
    
    let order = Order {
        id: Uuid::new_v4(),
        customer_id: Uuid::new_v4(),
        strategy_id: Uuid::new_v4(),
        pair_id: Uuid::new_v4(),
        leg: dry_testing_engine::types::order::OrderLeg::A,
        venue: VenueType::Polymarket,
        venue_market_id: "test_market".to_string(),
        venue_outcome_id: "test_outcome".to_string(),
        client_order_id: "test_client_id".to_string(),
        venue_order_id: None,
        side: OrderSide::Buy,
        limit_price_int: 100,
        price_scale: 2,
        size_int: 1000,
        size_scale: 2,
        status: dry_testing_engine::types::order::OrderStatus::Pending,
        filled_size_int: 0,
        filled_size_scale: 2,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    let response = simulator.submit_order(order, Duration::from_secs(1)).await.unwrap();
    
    match response {
        dry_testing_engine::venue::adapter::VenueResponse::Ack { venue_order_id } => {
            assert!(venue_order_id.starts_with("sim_"));
        }
        _ => panic!("Expected ACK response"),
    }
}

#[tokio::test]
async fn test_simulator_rejection() {
    let simulator = VenueSimulator::new(
        VenueType::Polymarket,
        10,
        100,
        1.0, // 100% rejection rate
        0.0,
    );
    
    let order = Order {
        id: Uuid::new_v4(),
        customer_id: Uuid::new_v4(),
        strategy_id: Uuid::new_v4(),
        pair_id: Uuid::new_v4(),
        leg: dry_testing_engine::types::order::OrderLeg::A,
        venue: VenueType::Polymarket,
        venue_market_id: "test_market".to_string(),
        venue_outcome_id: "test_outcome".to_string(),
        client_order_id: "test_client_id".to_string(),
        venue_order_id: None,
        side: OrderSide::Buy,
        limit_price_int: 100,
        price_scale: 2,
        size_int: 1000,
        size_scale: 2,
        status: dry_testing_engine::types::order::OrderStatus::Pending,
        filled_size_int: 0,
        filled_size_scale: 2,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    let response = simulator.submit_order(order, Duration::from_secs(1)).await.unwrap();
    
    match response {
        dry_testing_engine::venue::adapter::VenueResponse::Reject { .. } => {
            // Expected
        }
        _ => panic!("Expected REJECT response"),
    }
}

#[tokio::test]
async fn test_simulator_timeout() {
    let simulator = VenueSimulator::new(
        VenueType::Polymarket,
        1000, // 1 second ACK latency
        2000,
        0.0,
        0.0,
    );
    
    let order = Order {
        id: Uuid::new_v4(),
        customer_id: Uuid::new_v4(),
        strategy_id: Uuid::new_v4(),
        pair_id: Uuid::new_v4(),
        leg: dry_testing_engine::types::order::OrderLeg::A,
        venue: VenueType::Polymarket,
        venue_market_id: "test_market".to_string(),
        venue_outcome_id: "test_outcome".to_string(),
        client_order_id: "test_client_id".to_string(),
        venue_order_id: None,
        side: OrderSide::Buy,
        limit_price_int: 100,
        price_scale: 2,
        size_int: 1000,
        size_scale: 2,
        status: dry_testing_engine::types::order::OrderStatus::Pending,
        filled_size_int: 0,
        filled_size_scale: 2,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Timeout after 100ms (less than 1s ACK latency)
    let result = simulator.submit_order(order, Duration::from_millis(100)).await;
    assert!(result.is_err());
}

