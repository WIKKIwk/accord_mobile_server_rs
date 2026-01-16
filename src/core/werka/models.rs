use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DispatchRecord {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub record_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub supplier_ref: String,
    pub supplier_name: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub sent_qty: f64,
    pub accepted_qty: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub amount: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub currency: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub highlight: String,
    pub status: String,
    pub created_label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WerkaHomeSummary {
    pub pending_count: i64,
    pub confirmed_count: i64,
    pub returned_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaHomeData {
    pub summary: WerkaHomeSummary,
    pub pending_items: Vec<DispatchRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaStatusBreakdownEntry {
    pub supplier_ref: String,
    pub supplier_name: String,
    pub receipt_count: i64,
    pub total_sent_qty: f64,
    pub total_accepted_qty: f64,
    pub total_returned_qty: f64,
    pub uom: String,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}
