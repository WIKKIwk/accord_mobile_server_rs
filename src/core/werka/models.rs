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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchiveTotalByUom {
    pub uom: String,
    pub qty: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaArchiveSummary {
    pub record_count: usize,
    pub totals_by_uom: Vec<ArchiveTotalByUom>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaArchiveResponse {
    pub kind: String,
    pub period: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    pub summary: WerkaArchiveSummary,
    pub items: Vec<DispatchRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierDirectoryEntry {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerDirectoryEntry {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierItem {
    pub code: String,
    pub name: String,
    pub uom: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_group: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerItemOption {
    pub customer_ref: String,
    pub customer_name: String,
    pub customer_phone: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueCreateRequest {
    pub customer_ref: String,
    pub item_code: String,
    pub qty: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_barcode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_stock_entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line_index: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WerkaCustomerIssueSource {
    pub barcode: String,
    pub stock_entry_name: String,
    pub line_index: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WerkaCustomerIssueCreateInput {
    pub customer_ref: String,
    pub item_code: String,
    pub qty: f64,
    pub source: WerkaCustomerIssueSource,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueRecord {
    pub entry_id: String,
    pub customer_ref: String,
    pub customer_name: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub qty: f64,
    pub created_label: String,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}
