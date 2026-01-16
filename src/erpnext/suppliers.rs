use async_trait::async_trait;
use serde::Deserialize;

use crate::core::auth::ports::{AuthPortError, SupplierLookup, SupplierRecord};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl SupplierLookup for ErpnextClient {
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        let limit = normalize_limit(limit);
        let mut request = self
            .http
            .get(format!("{}/api/resource/Supplier", self.base_url))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .query(&[
                (
                    "fields",
                    r#"["name","supplier_name","mobile_no","supplier_details"]"#,
                ),
                ("filters", r#"[["disabled","=",0]]"#),
                ("limit_page_length", &limit.to_string()),
            ]);

        let trimmed = query.trim();
        if !trimmed.is_empty() {
            let like = format!("%{}%", trimmed.replace('"', ""));
            let or_filters = serde_json::json!([
                ["name", "like", like],
                ["supplier_name", "like", like],
                ["mobile_no", "like", like],
                ["supplier_details", "like", like],
            ]);
            request = request.query(&[("or_filters", or_filters.to_string())]);
        }

        let payload = request
            .send()
            .await
            .map_err(|_| AuthPortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| AuthPortError::LookupFailed)?
            .json::<SupplierListResponse>()
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;

        Ok(suppliers_from_list_response(payload))
    }
}

fn normalize_limit(limit: usize) -> usize {
    match limit {
        0 => 20,
        1..=500 => limit,
        _ => 500,
    }
}

#[derive(Debug, Deserialize)]
struct SupplierListResponse {
    data: Vec<SupplierListRow>,
}

#[derive(Debug, Deserialize)]
struct SupplierListRow {
    name: String,
    #[serde(default)]
    supplier_name: String,
    #[serde(default)]
    mobile_no: String,
    #[serde(default)]
    supplier_details: String,
}

fn suppliers_from_list_response(payload: SupplierListResponse) -> Vec<SupplierRecord> {
    payload
        .data
        .into_iter()
        .map(|row| {
            let name = if row.supplier_name.trim().is_empty() {
                row.name.trim().to_string()
            } else {
                row.supplier_name.trim().to_string()
            };
            let phone = if row.mobile_no.trim().is_empty() {
                extract_phone_from_details(&row.supplier_details)
            } else {
                row.mobile_no.trim().to_string()
            };

            SupplierRecord {
                id: row.name.trim().to_string(),
                name,
                phone,
            }
        })
        .collect()
}

fn extract_phone_from_details(details: &str) -> String {
    for line in details.lines() {
        let trimmed = line.trim();
        let Some((label, value)) = trimmed.split_once(':') else {
            continue;
        };
        if label.trim().eq_ignore_ascii_case("telefon") {
            return value.trim().to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::{SupplierListResponse, SupplierListRow, suppliers_from_list_response};

    #[test]
    fn maps_supplier_name_and_details_phone_like_go() {
        let suppliers = suppliers_from_list_response(SupplierListResponse {
            data: vec![SupplierListRow {
                name: "SUP-001".to_string(),
                supplier_name: String::new(),
                mobile_no: String::new(),
                supplier_details: "Telefon: +998901234567".to_string(),
            }],
        });

        assert_eq!(suppliers[0].id, "SUP-001");
        assert_eq!(suppliers[0].name, "SUP-001");
        assert_eq!(suppliers[0].phone, "+998901234567");
    }
}
