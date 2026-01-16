use crate::core::werka::models::DispatchRecord;
use crate::core::werka::ports::PurchaseReceiptDraft;

const WERKA_UNANNOUNCED_PREFIX: &str = "Accord Werka Aytilmagan:";
const WERKA_UNANNOUNCED_REASON_PREFIX: &str = "Accord Werka Aytilmagan Sabab:";

pub(crate) fn upsert_werka_unannounced_in_remarks(
    existing_note: &str,
    state: &str,
    reason: &str,
) -> String {
    let mut filtered = Vec::new();
    for line in existing_note.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(WERKA_UNANNOUNCED_PREFIX)
            || trimmed.starts_with(WERKA_UNANNOUNCED_REASON_PREFIX)
        {
            continue;
        }
        filtered.push(trimmed.to_string());
    }
    if !state.trim().is_empty() {
        filtered.push(format!("{WERKA_UNANNOUNCED_PREFIX} {}", state.trim()));
    }
    if !reason.trim().is_empty() {
        filtered.push(format!(
            "{WERKA_UNANNOUNCED_REASON_PREFIX} {}",
            reason.trim()
        ));
    }
    filtered.join("\n")
}

pub(crate) fn purchase_receipt_to_dispatch_record(
    draft: PurchaseReceiptDraft,
    fallback_supplier_name: &str,
) -> DispatchRecord {
    let unannounced_state = extract_werka_unannounced_state(&draft.remarks);
    let status = if draft.doc_status == 2 || draft.status.trim().eq_ignore_ascii_case("Cancelled") {
        "cancelled"
    } else if draft.doc_status == 1 {
        "accepted"
    } else if draft.status.trim().eq_ignore_ascii_case("Draft") {
        "draft"
    } else {
        "pending"
    };
    let mut note = String::new();
    if draft.doc_status == 0 && unannounced_state == "pending" {
        note = "Werka siz qayd etmagan mahsulotni qabul qildi. Tasdiqlash kutilmoqda.".to_string();
    }
    let supplier_name = if draft.supplier_name.trim().is_empty() {
        fallback_supplier_name.trim().to_string()
    } else {
        draft.supplier_name.trim().to_string()
    };

    DispatchRecord {
        id: draft.name,
        record_type: "purchase_receipt".to_string(),
        supplier_ref: draft.supplier,
        supplier_name,
        item_code: draft.item_code,
        item_name: draft.item_name,
        uom: draft.uom,
        sent_qty: draft.qty,
        accepted_qty: if status == "accepted" { draft.qty } else { 0.0 },
        amount: draft.amount,
        currency: draft.currency,
        note,
        event_type: if draft.doc_status == 0 && unannounced_state == "pending" {
            "werka_unannounced_pending".to_string()
        } else {
            String::new()
        },
        status: status.to_string(),
        created_label: draft.posting_date,
        ..DispatchRecord::default()
    }
}

pub(crate) fn format_notification_comment(
    label: &str,
    display_name: &str,
    message: &str,
) -> String {
    let name = display_name.trim();
    if name.is_empty() {
        format!("{}\n{}", label.trim(), message.trim())
    } else {
        format!("{} • {}\n{}", label.trim(), name, message.trim())
    }
}

fn extract_werka_unannounced_state(remarks: &str) -> String {
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(WERKA_UNANNOUNCED_PREFIX) {
            return value.trim().to_lowercase();
        }
    }
    String::new()
}
