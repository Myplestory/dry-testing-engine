//! Unit tests for order state machine

use dry_testing_engine::db::DatabaseWriter;
use dry_testing_engine::routing::OrderRouter;
use dry_testing_engine::state_machine::events::EventSource;
use dry_testing_engine::state_machine::OrderStateMachine;
use dry_testing_engine::types::order::{OrderLeg, OrderStatus};
use dry_testing_engine::types::venue::VenueType;
use std::sync::Arc;
use std::env;
use uuid::Uuid;
use chrono::Utc;

// Helper to create a test state machine (without database)
async fn create_test_state_machine() -> OrderStateMachine {
    // Load .env file from dry_testing_engine directory
    let _ = dotenv::dotenv();
    
    // Get DATABASE_URL from environment
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    let order_router = Arc::new(OrderRouter::new());
    // Create a minimal database writer (will fail on actual writes, but that's ok for unit tests)
    let db = Arc::new(
        sqlx::PgPool::connect(&database_url)
            .await
            .unwrap_or_else(|e| {
                panic!("Test database connection failed: {}. Make sure DATABASE_URL in .env is correct and database is accessible.", e);
            })
    );
    let db_writer = Arc::new(DatabaseWriter::new(db.clone()).await.unwrap());
    OrderStateMachine::new(order_router, db_writer).await.unwrap()
}

#[tokio::test]
async fn test_state_transition_pending_to_submitted() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    // Create initial order state
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: None,
        status: OrderStatus::Pending,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 0,
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Process ACK event
    let event = EventSource::VenueAck {
        venue_order_id: "venue_123".to_string(),
        timestamp: Utc::now(),
    };
    
    let transition = state_machine.process_event(order_id, event).await.unwrap();
    assert_eq!(transition.from, OrderStatus::Pending);
    assert_eq!(transition.to, OrderStatus::Submitted);
    
    let order = state_machine.get_order(order_id).unwrap();
    assert_eq!(order.status, OrderStatus::Submitted);
    assert_eq!(order.venue_order_id, Some("venue_123".to_string()));
}

#[tokio::test]
async fn test_state_transition_submitted_to_filled() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: Some("venue_123".to_string()),
        status: OrderStatus::Submitted,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 0,
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Process FILL event
    let event = EventSource::VenueFill {
        venue_order_id: "venue_123".to_string(),
        fill_size: (1000000, 6), // Full fill
        fill_price: (5000, 2),
        trade_id: "trade_123".to_string(),
        timestamp: Utc::now(),
    };
    
    let transition = state_machine.process_event(order_id, event).await.unwrap();
    assert_eq!(transition.from, OrderStatus::Submitted);
    assert_eq!(transition.to, OrderStatus::Filled);
    
    let order = state_machine.get_order(order_id).unwrap();
    assert_eq!(order.status, OrderStatus::Filled);
    assert_eq!(order.filled_size_int, 1000000);
}

#[tokio::test]
async fn test_state_transition_submitted_to_partial() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: Some("venue_123".to_string()),
        status: OrderStatus::Submitted,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 0,
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Process partial FILL event
    let event = EventSource::VenueFill {
        venue_order_id: "venue_123".to_string(),
        fill_size: (500000, 6), // Half fill
        fill_price: (5000, 2),
        trade_id: "trade_123".to_string(),
        timestamp: Utc::now(),
    };
    
    let transition = state_machine.process_event(order_id, event).await.unwrap();
    assert_eq!(transition.from, OrderStatus::Submitted);
    assert_eq!(transition.to, OrderStatus::Partial);
    
    let order = state_machine.get_order(order_id).unwrap();
    assert_eq!(order.status, OrderStatus::Partial);
    assert_eq!(order.filled_size_int, 500000);
}

#[tokio::test]
async fn test_state_transition_to_rejected() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: Some("venue_123".to_string()),
        status: OrderStatus::Submitted,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 0,
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Process REJECT event
    let event = EventSource::VenueReject {
        venue_order_id: "venue_123".to_string(),
        reason: "Insufficient funds".to_string(),
        timestamp: Utc::now(),
    };
    
    let transition = state_machine.process_event(order_id, event).await.unwrap();
    assert_eq!(transition.from, OrderStatus::Submitted);
    assert_eq!(transition.to, OrderStatus::Rejected);
    
    let order = state_machine.get_order(order_id).unwrap();
    assert_eq!(order.status, OrderStatus::Rejected);
}

#[tokio::test]
async fn test_state_transition_invalid_from_pending() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: None,
        status: OrderStatus::Pending,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 0,
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Try to process FILL event from PENDING (invalid)
    let event = EventSource::VenueFill {
        venue_order_id: "venue_123".to_string(),
        fill_size: (1000000, 6),
        fill_price: (5000, 2),
        trade_id: "trade_123".to_string(),
        timestamp: Utc::now(),
    };
    
    let result = state_machine.process_event(order_id, event).await;
    assert!(result.is_err(), "FILL from PENDING should be invalid");
}

#[tokio::test]
async fn test_state_transition_partial_to_filled() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let initial_state = dry_testing_engine::types::order::OrderState {
        order_id,
        intent_id: Uuid::new_v4(),
        leg: OrderLeg::A,
        venue: VenueType::Polymarket,
        client_order_id: "test_client_id".to_string(),
        venue_order_id: Some("venue_123".to_string()),
        status: OrderStatus::Partial,
        sequence_number: 1,
        total_size_int: 1000000,
        total_size_scale: 6,
        filled_size_int: 500000, // Already half filled
        filled_size_scale: 6,
        transitions: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Use restore_order to add the order to the state machine
    state_machine.restore_order(initial_state).await.unwrap();
    
    // Process second FILL event to complete
    let event = EventSource::VenueFill {
        venue_order_id: "venue_123".to_string(),
        fill_size: (500000, 6), // Remaining half
        fill_price: (5000, 2),
        trade_id: "trade_124".to_string(),
        timestamp: Utc::now(),
    };
    
    let transition = state_machine.process_event(order_id, event).await.unwrap();
    assert_eq!(transition.from, OrderStatus::Partial);
    assert_eq!(transition.to, OrderStatus::Filled);
    
    let order = state_machine.get_order(order_id).unwrap();
    assert_eq!(order.status, OrderStatus::Filled);
    assert_eq!(order.filled_size_int, 1000000);
}

#[tokio::test]
async fn test_get_order_not_found() {
    let state_machine = create_test_state_machine().await;
    let order_id = Uuid::new_v4();
    
    let result = state_machine.get_order(order_id);
    assert!(result.is_err(), "Order not found should return error");
}

