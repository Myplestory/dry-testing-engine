//! Unit tests for type definitions

use dry_testing_engine::types::*;
use std::str::FromStr;

#[test]
fn test_order_status_conversions() {
    // Test FromStr
    assert_eq!(OrderStatus::from_str("pending").unwrap(), OrderStatus::Pending);
    assert_eq!(OrderStatus::from_str("filled").unwrap(), OrderStatus::Filled);
    assert_eq!(OrderStatus::from_str("FILLED").unwrap(), OrderStatus::Filled); // Case insensitive
    
    // Test Display
    assert_eq!(OrderStatus::Filled.to_string(), "filled");
    
    // Test terminal states
    assert!(OrderStatus::Filled.is_terminal());
    assert!(OrderStatus::Rejected.is_terminal());
    assert!(OrderStatus::Canceled.is_terminal());
    assert!(!OrderStatus::Pending.is_terminal());
    assert!(!OrderStatus::Submitted.is_terminal());
    assert!(!OrderStatus::Partial.is_terminal());
}

#[test]
fn test_order_side_conversions() {
    assert_eq!(OrderSide::from_str("buy").unwrap(), OrderSide::Buy);
    assert_eq!(OrderSide::from_str("sell").unwrap(), OrderSide::Sell);
    assert_eq!(OrderSide::Buy.to_string(), "buy");
}

#[test]
fn test_order_leg_conversions() {
    assert_eq!(OrderLeg::from_str("A").unwrap(), OrderLeg::A);
    assert_eq!(OrderLeg::from_str("a").unwrap(), OrderLeg::A); // Case insensitive
    assert_eq!(OrderLeg::A.to_string(), "A");
}

#[test]
fn test_venue_type_conversions() {
    assert_eq!(VenueType::from_str("polymarket").unwrap(), VenueType::Polymarket);
    assert_eq!(VenueType::from_str("kalshi").unwrap(), VenueType::Kalshi);
    assert_eq!(VenueType::Polymarket.as_str(), "polymarket");
    assert_eq!(VenueType::Polymarket.array_index(), Some(0));
    assert_eq!(VenueType::Kalshi.array_index(), Some(1));
    
    // Test dynamic venue
    let other = VenueType::from_str("custom_venue").unwrap();
    assert!(matches!(other, VenueType::Other(_)));
    assert_eq!(other.as_str(), "custom_venue");
    assert_eq!(other.array_index(), None);
}

#[test]
fn test_execution_priority_ordering() {
    assert!(ExecutionPriority::RiskCritical > ExecutionPriority::Opportunity);
}

