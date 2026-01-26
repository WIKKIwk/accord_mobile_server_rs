use std::collections::HashSet;
use std::sync::Arc;

use crate::core::auth::models::Principal;
use crate::core::customer::models::{
    CustomerDeliveryDetail, CustomerDeliveryResponseMode, CustomerDeliveryResponseRequest,
    CustomerHomeSummary,
};
use crate::core::customer::ports::{
    CustomerDeliveryNoteDraft, CustomerDeliveryPort, CustomerServiceError,
};
use crate::core::werka::models::DispatchRecord;
use crate::core::werka::ports::DeliveryNoteStateUpdate;

const DELIVERY_FLOW_STATE_NONE: i32 = 0;
const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
const DELIVERY_ACTOR_WERKA: i32 = 2;
const CUSTOMER_STATE_PENDING: i32 = 1;
const CUSTOMER_STATE_REJECTED: i32 = 2;
const CUSTOMER_STATE_CONFIRMED: i32 = 3;
const CUSTOMER_STATE_PARTIAL: i32 = 4;
const CUSTOMER_QTY_TOLERANCE: f64 = 0.0001;
const MIN_CUSTOMER_REJECT_REASON_RUNES: usize = 3;

#[derive(Clone, Default)]
pub struct CustomerService {
    delivery_port: Option<Arc<dyn CustomerDeliveryPort>>,
}

impl CustomerService {
    pub fn new() -> Self {
        Self {
            delivery_port: None,
        }
    }

    pub fn with_delivery_port(mut self, delivery_port: Arc<dyn CustomerDeliveryPort>) -> Self {
        self.delivery_port = Some(delivery_port);
        self
    }

    pub async fn summary(
        &self,
        principal: &Principal,
    ) -> Result<Option<CustomerHomeSummary>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        let mut summary = CustomerHomeSummary::default();
        for item in items.iter().filter(|item| customer_delivery_visible(item)) {
            match customer_delivery_status(item) {
                "accepted" => summary.confirmed_count += 1,
                "partial" | "rejected" => summary.rejected_count += 1,
                _ => summary.pending_count += 1,
            }
        }
        Ok(Some(summary))
    }

    pub async fn history(
        &self,
        principal: &Principal,
    ) -> Result<Option<Vec<DispatchRecord>>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        Ok(Some(
            items
                .into_iter()
                .filter(customer_delivery_visible)
                .map(delivery_note_to_dispatch_record)
                .collect(),
        ))
    }

    pub async fn status_details(
        &self,
        principal: &Principal,
        kind: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        let mut filter_kind = kind.trim();
        if filter_kind == "confirmed" {
            filter_kind = "accepted";
        }
        Ok(Some(
            items
                .into_iter()
                .filter(customer_delivery_visible)
                .filter(|item| {
                    let status = customer_delivery_status(item);
                    if filter_kind == "rejected" {
                        status == "rejected" || status == "partial"
                    } else {
                        status == filter_kind
                    }
                })
                .map(delivery_note_to_dispatch_record)
                .collect(),
        ))
    }

    pub async fn detail(
        &self,
        principal: &Principal,
        delivery_note_id: &str,
    ) -> Result<Option<CustomerDeliveryDetail>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        let draft = port.get_delivery_note(delivery_note_id.trim()).await?;
        if draft.customer.trim() != principal.ref_.trim() {
            return Err(CustomerServiceError::Unauthorized);
        }
        Ok(Some(detail_from_draft(draft)))
    }

    pub async fn respond(
        &self,
        principal: &Principal,
        request: CustomerDeliveryResponseRequest,
    ) -> Result<Option<CustomerDeliveryDetail>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        let mut draft = port
            .get_delivery_note(request.delivery_note_id.trim())
            .await?;
        if draft.customer.trim() != principal.ref_.trim() {
            return Err(CustomerServiceError::Unauthorized);
        }
        let decision = normalize_customer_delivery_decision(&request, &draft)?;
        if decision.returned_qty > 0.0 {
            if nearly_equal_qty(decision.returned_qty, draft.qty) {
                port.create_and_submit_delivery_note_return(&draft.name)
                    .await?;
            } else {
                port.create_and_submit_partial_delivery_note_return(
                    &draft.name,
                    decision.returned_qty,
                )
                .await?;
            }
        }
        let combined_reason =
            combine_customer_reason_and_comment(&decision.reason, &decision.comment);
        let remarks = upsert_customer_decision_payload_in_remarks(
            &draft.remarks,
            decision.state_label(),
            &decision.reason,
            decision.accepted_qty,
            decision.returned_qty,
            &draft.uom,
            &decision.comment,
        );
        if remarks != draft.remarks.trim() {
            port.update_delivery_note_remarks(&draft.name, &remarks)
                .await?;
        }
        port.update_delivery_note_state(
            &draft.name,
            DeliveryNoteStateUpdate {
                flow_state: DELIVERY_FLOW_STATE_SUBMITTED.to_string(),
                customer_state: decision.customer_state.to_string(),
                customer_reason: combined_reason.clone(),
                delivery_actor: DELIVERY_ACTOR_WERKA.to_string(),
                ui_status: customer_delivery_ui_status(
                    DELIVERY_FLOW_STATE_SUBMITTED,
                    decision.customer_state,
                )
                .to_string(),
            },
        )
        .await?;

        draft.remarks = remarks;
        draft.accord_flow_state = DELIVERY_FLOW_STATE_SUBMITTED.to_string();
        draft.accord_customer_state = decision.customer_state.to_string();
        draft.accord_customer_reason = combined_reason;
        draft.accord_delivery_actor = DELIVERY_ACTOR_WERKA.to_string();
        draft.accord_ui_status =
            customer_delivery_ui_status(DELIVERY_FLOW_STATE_SUBMITTED, decision.customer_state)
                .to_string();

        Ok(Some(CustomerDeliveryDetail {
            record: delivery_note_to_dispatch_record(draft),
            can_approve: false,
            can_reject: false,
            can_partially_accept: false,
            can_report_claim: false,
        }))
    }

    async fn collect_customer_delivery_notes(
        &self,
        customer_ref: &str,
    ) -> Result<Option<Vec<CustomerDeliveryNoteDraft>>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        const PAGE_SIZE: usize = 200;
        let mut result = Vec::with_capacity(PAGE_SIZE);
        let mut seen = HashSet::with_capacity(PAGE_SIZE);
        let mut offset = 0;
        loop {
            let items = port
                .list_customer_delivery_notes_page(customer_ref, PAGE_SIZE, offset)
                .await?;
            for item in &items {
                let name = item.name.trim();
                if name.is_empty() || !seen.insert(name.to_string()) {
                    continue;
                }
                result.push(item.clone());
            }
            if items.len() < PAGE_SIZE {
                return Ok(Some(result));
            }
            offset += PAGE_SIZE;
        }
    }
}

fn detail_from_draft(draft: CustomerDeliveryNoteDraft) -> CustomerDeliveryDetail {
    let status = customer_delivery_status(&draft);
    let pending = status == "pending";
    CustomerDeliveryDetail {
        record: delivery_note_to_dispatch_record(draft),
        can_approve: pending,
        can_reject: pending,
        can_partially_accept: pending,
        can_report_claim: status == "accepted",
    }
}

fn customer_delivery_status(item: &CustomerDeliveryNoteDraft) -> &'static str {
    if item.doc_status != 1 {
        return "draft";
    }
    if parse_accord_int(&item.accord_flow_state, DELIVERY_FLOW_STATE_NONE)
        != DELIVERY_FLOW_STATE_SUBMITTED
    {
        return "pending";
    }
    match parse_accord_int(&item.accord_customer_state, CUSTOMER_STATE_PENDING) {
        CUSTOMER_STATE_REJECTED => "rejected",
        CUSTOMER_STATE_CONFIRMED => "accepted",
        CUSTOMER_STATE_PARTIAL => "partial",
        _ => "pending",
    }
}

fn customer_delivery_visible(item: &CustomerDeliveryNoteDraft) -> bool {
    item.doc_status == 1
        && parse_accord_int(&item.accord_flow_state, DELIVERY_FLOW_STATE_NONE)
            == DELIVERY_FLOW_STATE_SUBMITTED
}

fn delivery_note_to_dispatch_record(item: CustomerDeliveryNoteDraft) -> DispatchRecord {
    let status = customer_delivery_status(&item);
    let (accepted_qty, returned_qty) = customer_decision_quantities(&item, status);
    let mut note = match status {
        "accepted" => "Customer tasdiqladi.".to_string(),
        "partial" => format!(
            "Customer qisman qabul qildi. Qabul: {:.2} {}. Qaytdi: {:.2} {}.",
            accepted_qty, item.uom, returned_qty, item.uom
        ),
        "rejected" => "Customer rad etdi.".to_string(),
        _ => String::new(),
    };
    if !item.accord_customer_reason.trim().is_empty() {
        note.push_str(" Sabab: ");
        note.push_str(item.accord_customer_reason.trim());
    }
    DispatchRecord {
        id: item.name,
        record_type: "delivery_note".to_string(),
        supplier_ref: item.customer,
        supplier_name: item.customer_name,
        item_code: item.item_code,
        item_name: item.item_name,
        uom: item.uom,
        sent_qty: item.qty,
        accepted_qty,
        note,
        status: status.to_string(),
        created_label: first_non_empty(&item.modified, &item.posting_date),
        ..DispatchRecord::default()
    }
}

fn customer_decision_quantities(item: &CustomerDeliveryNoteDraft, status: &str) -> (f64, f64) {
    let (mut accepted_qty, mut returned_qty) = extract_customer_decision_quantities(&item.remarks);
    if returned_qty <= 0.0 && item.returned_qty > 0.0 {
        returned_qty = item.returned_qty;
    }
    match status {
        "accepted" => {
            if accepted_qty <= 0.0 {
                accepted_qty = item.qty;
            }
            (accepted_qty, 0.0)
        }
        "partial" => {
            if accepted_qty <= 0.0 && returned_qty > 0.0 {
                accepted_qty = (item.qty - returned_qty).max(0.0);
            }
            if returned_qty <= 0.0 && accepted_qty > 0.0 {
                returned_qty = (item.qty - accepted_qty).max(0.0);
            }
            (accepted_qty, returned_qty)
        }
        "rejected" => (0.0, item.qty),
        _ => (accepted_qty, returned_qty),
    }
}

struct CustomerDeliveryDecision {
    customer_state: i32,
    accepted_qty: f64,
    returned_qty: f64,
    reason: String,
    comment: String,
}

impl CustomerDeliveryDecision {
    fn state_label(&self) -> &'static str {
        match self.customer_state {
            CUSTOMER_STATE_CONFIRMED => "confirmed",
            CUSTOMER_STATE_REJECTED => "rejected",
            CUSTOMER_STATE_PARTIAL => "partial",
            _ => "pending",
        }
    }
}

fn normalize_customer_delivery_decision(
    request: &CustomerDeliveryResponseRequest,
    draft: &CustomerDeliveryNoteDraft,
) -> Result<CustomerDeliveryDecision, CustomerServiceError> {
    let current_status = customer_delivery_status(draft);
    let mode = request.mode.or_else(|| {
        request.approve.map(|approve| {
            if approve {
                CustomerDeliveryResponseMode::AcceptAll
            } else {
                CustomerDeliveryResponseMode::RejectAll
            }
        })
    });
    let reason = request.reason.trim().to_string();
    let comment = request.comment.trim().to_string();
    let sent_qty = draft.qty;
    if sent_qty <= 0.0 {
        return Err(CustomerServiceError::InvalidInput);
    }

    match mode {
        Some(CustomerDeliveryResponseMode::AcceptAll) => {
            require_pending(current_status)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_CONFIRMED,
                accepted_qty: sent_qty,
                returned_qty: 0.0,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::RejectAll) => {
            require_pending(current_status)?;
            require_meaningful_return_reason(&reason, &comment)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_REJECTED,
                accepted_qty: 0.0,
                returned_qty: sent_qty,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::AcceptPartial) => {
            require_pending(current_status)?;
            require_meaningful_return_reason(&reason, &comment)?;
            let (accepted_qty, returned_qty) =
                normalize_partial_quantities(sent_qty, request.accepted_qty, request.returned_qty)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_PARTIAL,
                accepted_qty,
                returned_qty,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::ClaimAfterAccept) => {
            if current_status != "accepted" {
                return Err(CustomerServiceError::Failed(format!(
                    "delivery note cannot accept claim in status {current_status}"
                )));
            }
            require_meaningful_return_reason(&reason, &comment)?;
            let returned_qty = request.returned_qty;
            if returned_qty <= 0.0 || returned_qty > sent_qty + CUSTOMER_QTY_TOLERANCE {
                return Err(CustomerServiceError::InvalidInput);
            }
            if nearly_equal_qty(returned_qty, sent_qty) {
                return Ok(CustomerDeliveryDecision {
                    customer_state: CUSTOMER_STATE_REJECTED,
                    accepted_qty: 0.0,
                    returned_qty: sent_qty,
                    reason,
                    comment,
                });
            }
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_PARTIAL,
                accepted_qty: sent_qty - returned_qty,
                returned_qty,
                reason,
                comment,
            })
        }
        None => Err(CustomerServiceError::InvalidInput),
    }
}

fn require_pending(status: &str) -> Result<(), CustomerServiceError> {
    if status == "pending" {
        Ok(())
    } else {
        Err(CustomerServiceError::Failed(
            "delivery note is not pending".to_string(),
        ))
    }
}

fn require_meaningful_return_reason(
    reason: &str,
    comment: &str,
) -> Result<(), CustomerServiceError> {
    if reason.trim().chars().count() >= MIN_CUSTOMER_REJECT_REASON_RUNES
        || comment.trim().chars().count() >= MIN_CUSTOMER_REJECT_REASON_RUNES
    {
        Ok(())
    } else {
        Err(CustomerServiceError::InvalidInput)
    }
}

fn normalize_partial_quantities(
    sent_qty: f64,
    mut accepted_qty: f64,
    mut returned_qty: f64,
) -> Result<(f64, f64), CustomerServiceError> {
    if accepted_qty > 0.0 && returned_qty > 0.0 {
    } else if accepted_qty > 0.0 {
        returned_qty = sent_qty - accepted_qty;
    } else if returned_qty > 0.0 {
        accepted_qty = sent_qty - returned_qty;
    } else {
        return Err(CustomerServiceError::InvalidInput);
    }
    if accepted_qty <= 0.0 || returned_qty <= 0.0 {
        return Err(CustomerServiceError::InvalidInput);
    }
    if ((accepted_qty + returned_qty) - sent_qty).abs() > CUSTOMER_QTY_TOLERANCE {
        return Err(CustomerServiceError::InvalidInput);
    }
    Ok((accepted_qty, returned_qty))
}

fn nearly_equal_qty(left: f64, right: f64) -> bool {
    (left - right).abs() <= CUSTOMER_QTY_TOLERANCE
}

fn customer_delivery_ui_status(flow_state: i32, customer_state: i32) -> &'static str {
    if flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
        return "pending";
    }
    match customer_state {
        CUSTOMER_STATE_CONFIRMED => "confirm",
        CUSTOMER_STATE_PARTIAL => "partial",
        CUSTOMER_STATE_REJECTED => "rejected",
        _ => "pending",
    }
}

fn upsert_customer_decision_payload_in_remarks(
    existing_note: &str,
    state: &str,
    reason: &str,
    accepted_qty: f64,
    returned_qty: f64,
    uom: &str,
    comment: &str,
) -> String {
    let mut filtered = Vec::new();
    for line in existing_note.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("AC:")
            || trimmed.starts_with("AR:")
            || trimmed.starts_with("AQ:")
            || trimmed.starts_with("AT:")
            || trimmed.starts_with("AX:")
        {
            continue;
        }
        filtered.push(trimmed.to_string());
    }
    if let Some(normalized) = normalize_customer_decision_state(state) {
        filtered.push(format!("AC:{normalized}"));
    }
    if !reason.trim().is_empty() {
        filtered.push(format!("AR:{}", reason.trim()));
    }
    if accepted_qty > 0.0 {
        filtered.push(format!("AQ:{accepted_qty:.4} {}", uom.trim()));
    }
    if returned_qty > 0.0 {
        filtered.push(format!("AT:{returned_qty:.4} {}", uom.trim()));
    }
    if !comment.trim().is_empty() {
        filtered.push(format!("AX:{}", comment.trim()));
    }
    filtered.join("\n")
}

fn extract_customer_decision_quantities(remarks: &str) -> (f64, f64) {
    let mut accepted_qty = 0.0;
    let mut returned_qty = 0.0;
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("AQ:") {
            accepted_qty = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0);
        } else if let Some(value) = trimmed.strip_prefix("AT:") {
            returned_qty = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0);
        }
    }
    (accepted_qty, returned_qty)
}

fn normalize_customer_decision_state(state: &str) -> Option<&'static str> {
    match state.trim().to_ascii_lowercase().as_str() {
        "pending" | "pd" => Some("pending"),
        "confirmed" | "accepted" | "cf" => Some("confirmed"),
        "partial" | "pt" => Some("partial"),
        "rejected" | "rj" => Some("rejected"),
        _ => None,
    }
}

fn combine_customer_reason_and_comment(reason: &str, comment: &str) -> String {
    let reason = reason.trim();
    let comment = comment.trim();
    match (reason.is_empty(), comment.is_empty()) {
        (true, _) => comment.to_string(),
        (_, true) => reason.to_string(),
        _ => format!("{reason}. {comment}"),
    }
}

fn parse_accord_int(value: &str, default: i32) -> i32 {
    value.trim().parse::<i32>().unwrap_or(default)
}

fn first_non_empty(left: &str, right: &str) -> String {
    if !left.trim().is_empty() {
        left.trim().to_string()
    } else {
        right.trim().to_string()
    }
}
