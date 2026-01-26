use async_trait::async_trait;
use serde::Deserialize;

use crate::core::admin::models::AdminDirectoryEntry;
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl AdminReadPort for ErpnextClient {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","supplier_name","mobile_no","supplier_details"]"#.to_string(),
            ),
            ("filters", r#"[["disabled","=",0]]"#.to_string()),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", supplier_or_filters(query)));
        }
        let payload: ListResponse<SupplierRow> = self
            .admin_get_json("/api/resource/Supplier", &params)
            .await?;
        Ok(payload.data.into_iter().map(supplier_entry).collect())
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload: GetResponse<SupplierRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Supplier/{}",
                    urlencoding::encode(ref_.trim())
                ),
                &[(
                    "fields",
                    r#"["name","supplier_name","mobile_no","supplier_details"]"#.to_string(),
                )],
            )
            .await?;
        Ok(supplier_entry(payload.data))
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","customer_name","mobile_no","customer_details"]"#.to_string(),
            ),
            ("filters", r#"[["disabled","=",0]]"#.to_string()),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
            ("order_by", "modified desc".to_string()),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", customer_or_filters(query)));
        }
        let payload: ListResponse<CustomerRow> = self
            .admin_get_json("/api/resource/Customer", &params)
            .await?;
        Ok(payload.data.into_iter().map(customer_entry).collect())
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload: GetResponse<CustomerRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Customer/{}",
                    urlencoding::encode(ref_.trim())
                ),
                &[(
                    "fields",
                    r#"["name","customer_name","mobile_no","customer_details"]"#.to_string(),
                )],
            )
            .await?;
        Ok(customer_entry(payload.data))
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
            ),
            (
                "filters",
                r#"[["disabled","=",0],["is_stock_item","=",1]]"#.to_string(),
            ),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
            ("order_by", "item_name asc, name asc".to_string()),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", item_or_filters(query)));
        }
        let payload: ListResponse<ItemRow> =
            self.admin_get_json("/api/resource/Item", &params).await?;
        let warehouse = self.default_warehouse();
        Ok(payload
            .data
            .into_iter()
            .map(|row| supplier_item(row, &warehouse))
            .collect())
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        if item_codes.is_empty() {
            return Ok(Vec::new());
        }
        let codes = item_codes
            .iter()
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        let filters = serde_json::json!([
            ["disabled", "=", 0],
            ["is_stock_item", "=", 1],
            ["name", "in", codes],
        ]);
        let payload: ListResponse<ItemRow> = self
            .admin_get_json(
                "/api/resource/Item",
                &[
                    (
                        "fields",
                        r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("limit_page_length", codes.len().to_string()),
                ],
            )
            .await?;
        let warehouse = self.default_warehouse();
        Ok(payload
            .data
            .into_iter()
            .map(|row| supplier_item(row, &warehouse))
            .collect())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let payload: SearchLinkResponse = self
            .admin_get_json(
                "/api/method/frappe.desk.search.search_link",
                &[
                    ("doctype", "Item Group".to_string()),
                    ("txt", query.trim().to_string()),
                    ("page_length", normalize_limit(limit, 50, 100).to_string()),
                ],
            )
            .await?;
        Ok(payload
            .results
            .into_iter()
            .map(|row| row.value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let filters = serde_json::json!([["supplier", "=", supplier_ref.trim()]]);
        let payload: ListResponse<ItemSupplierRow> = self
            .admin_get_json(
                "/api/resource/Item Supplier",
                &[
                    ("parent", "Item".to_string()),
                    ("fields", r#"["parent"]"#.to_string()),
                    ("filters", filters.to_string()),
                    (
                        "limit_page_length",
                        normalize_limit(limit, 200, 500).to_string(),
                    ),
                ],
            )
            .await?;
        let codes = payload
            .data
            .into_iter()
            .map(|row| row.parent)
            .collect::<Vec<_>>();
        self.items_by_codes(&codes).await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let payload: GetResponse<CustomerItemsRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Customer/{}",
                    urlencoding::encode(customer_ref.trim())
                ),
                &[("fields", r#"["custom_customer_items"]"#.to_string())],
            )
            .await?;
        let needle = query.trim().to_lowercase();
        let codes = payload
            .data
            .custom_customer_items
            .into_iter()
            .map(|row| row.item_code)
            .filter(|code| needle.is_empty() || code.to_lowercase().contains(&needle))
            .take(normalize_limit(limit, 200, 500))
            .collect::<Vec<_>>();
        self.items_by_codes(&codes).await
    }
}

#[async_trait]
impl AdminWritePort for ErpnextClient {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload = serde_json::json!({
            "supplier_name": name.trim(),
            "supplier_type": "Company",
            "supplier_group": "Services",
            "mobile_no": phone.trim(),
            "supplier_details": if phone.trim().is_empty() {
                String::new()
            } else {
                format!("Telefon: {}", phone.trim())
            },
        });
        let response: GetResponse<SupplierRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Supplier", payload)
            .await?;
        Ok(supplier_entry(response.data))
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let current = self.supplier_by_ref(ref_).await?;
        let details = upsert_phone_in_details("", phone);
        let payload = serde_json::json!({
            "mobile_no": phone.trim(),
            "supplier_details": details,
        });
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Supplier/{}",
                urlencoding::encode(&current.ref_)
            ),
            payload,
        )
        .await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload = serde_json::json!({
            "parent": item_code.trim(),
            "parenttype": "Item",
            "parentfield": "supplier_items",
            "supplier": ref_.trim(),
        });
        self.admin_empty_request(
            reqwest::Method::POST,
            "/api/resource/Item%20Supplier",
            payload,
        )
        .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload: GetResponse<ItemSuppliersRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                &[(
                    "fields",
                    r#"["default_supplier","supplier_items"]"#.to_string(),
                )],
            )
            .await?;
        for row in payload.data.supplier_items {
            if row.supplier.trim().eq_ignore_ascii_case(ref_.trim()) && !row.name.trim().is_empty()
            {
                self.admin_empty_request(
                    reqwest::Method::DELETE,
                    &format!(
                        "/api/resource/Item%20Supplier/{}",
                        urlencoding::encode(row.name.trim())
                    ),
                    serde_json::Value::Null,
                )
                .await?;
            }
        }
        if payload
            .data
            .default_supplier
            .trim()
            .eq_ignore_ascii_case(ref_.trim())
        {
            self.admin_empty_request(
                reqwest::Method::PUT,
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                serde_json::json!({"default_supplier": ""}),
            )
            .await?;
        }
        Ok(())
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload = serde_json::json!({
            "customer_name": name.trim(),
            "customer_type": "Company",
            "mobile_no": phone.trim(),
        });
        let response: GetResponse<CustomerRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Customer", payload)
            .await?;
        Ok(customer_entry(response.data))
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Customer/{}",
                urlencoding::encode(ref_.trim())
            ),
            serde_json::json!({"customer_details": upsert_phone_in_details("", phone)}),
        )
        .await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Customer/{}",
                urlencoding::encode(ref_.trim())
            ),
            serde_json::json!({"customer_details": format!("Accord kodi: {}", code.trim())}),
        )
        .await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload = serde_json::json!({
            "parent": item_code.trim(),
            "parenttype": "Item",
            "parentfield": "customer_items",
            "customer_name": ref_.trim(),
            "ref_code": ref_.trim(),
        });
        self.admin_empty_request(
            reqwest::Method::POST,
            "/api/resource/Item%20Customer%20Detail",
            payload,
        )
        .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload: GetResponse<ItemCustomersRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                &[("fields", r#"["customer_items"]"#.to_string())],
            )
            .await?;
        let filtered = payload
            .data
            .customer_items
            .into_iter()
            .filter(|row| !row.customer_name.trim().eq_ignore_ascii_case(ref_.trim()))
            .map(|row| {
                serde_json::json!({
                    "doctype": "Item Customer Detail",
                    "name": row.name.trim(),
                    "customer_name": row.customer_name.trim(),
                    "customer_group": row.customer_group.trim(),
                    "ref_code": row.ref_code.trim(),
                })
            })
            .collect::<Vec<_>>();
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Item/{}",
                urlencoding::encode(item_code.trim())
            ),
            serde_json::json!({"customer_items": filtered}),
        )
        .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = code.trim();
        let name = if name.trim().is_empty() {
            code
        } else {
            name.trim()
        };
        let uom = if uom.trim().is_empty() {
            "Nos"
        } else {
            uom.trim()
        };
        let item_group = if item_group.trim().is_empty() {
            "All Item Groups"
        } else {
            item_group.trim()
        };
        let payload = serde_json::json!({
            "item_code": code,
            "item_name": name,
            "stock_uom": uom,
            "is_stock_item": 1,
            "item_group": item_group,
        });
        let response: GetResponse<ItemRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Item", payload)
            .await?;
        Ok(supplier_item(response.data, &self.default_warehouse()))
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Item/{}",
                urlencoding::encode(item_code.trim())
            ),
            serde_json::json!({"item_group": item_group.trim()}),
        )
        .await
    }
}

impl ErpnextClient {
    async fn admin_get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, AdminPortError> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url(), path))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .query(query)
            .send()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AdminPortError::NotFound);
        }
        let body = response
            .text()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        if !status.is_success() {
            return Err(AdminPortError::LookupFailed);
        }
        serde_json::from_str(&body).map_err(|_| AdminPortError::LookupFailed)
    }

    async fn admin_json_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: serde_json::Value,
    ) -> Result<T, AdminPortError> {
        let response = self
            .http
            .request(method, format!("{}{}", self.base_url(), path))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .json(&payload)
            .send()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AdminPortError::NotFound);
        }
        let body = response
            .text()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        if !status.is_success() {
            return Err(AdminPortError::LookupFailed);
        }
        serde_json::from_str(&body).map_err(|_| AdminPortError::LookupFailed)
    }

    async fn admin_empty_request(
        &self,
        method: reqwest::Method,
        path: &str,
        payload: serde_json::Value,
    ) -> Result<(), AdminPortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), path))
            .header(reqwest::header::AUTHORIZATION, self.auth_header());
        if !payload.is_null() {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AdminPortError::NotFound);
        }
        response
            .error_for_status()
            .map(|_| ())
            .map_err(|_| AdminPortError::LookupFailed)
    }
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct GetResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct SupplierRow {
    name: String,
    #[serde(default)]
    supplier_name: String,
    #[serde(default)]
    mobile_no: String,
    #[serde(default)]
    supplier_details: String,
}

#[derive(Debug, Deserialize)]
struct CustomerRow {
    name: String,
    #[serde(default)]
    customer_name: String,
    #[serde(default)]
    mobile_no: String,
    #[serde(default)]
    customer_details: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    name: String,
    #[serde(default)]
    item_name: String,
    #[serde(default)]
    stock_uom: String,
    #[serde(default)]
    item_group: String,
}

#[derive(Debug, Deserialize)]
struct ItemSupplierRow {
    parent: String,
}

#[derive(Debug, Deserialize)]
struct ItemSuppliersRow {
    #[serde(default)]
    default_supplier: String,
    #[serde(default)]
    supplier_items: Vec<ItemSupplierChildRow>,
}

#[derive(Debug, Deserialize)]
struct ItemSupplierChildRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    supplier: String,
}

#[derive(Debug, Deserialize)]
struct ItemCustomersRow {
    #[serde(default)]
    customer_items: Vec<ItemCustomerChildRow>,
}

#[derive(Debug, Deserialize)]
struct ItemCustomerChildRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    customer_name: String,
    #[serde(default)]
    customer_group: String,
    #[serde(default)]
    ref_code: String,
}

#[derive(Debug, Deserialize)]
struct CustomerItemsRow {
    #[serde(default)]
    custom_customer_items: Vec<CustomerItemRow>,
}

#[derive(Debug, Deserialize)]
struct CustomerItemRow {
    #[serde(default)]
    item_code: String,
}

#[derive(Debug, Deserialize)]
struct SearchLinkResponse {
    #[serde(default, alias = "message")]
    results: Vec<SearchLinkRow>,
}

#[derive(Debug, Deserialize)]
struct SearchLinkRow {
    #[serde(default)]
    value: String,
}

fn supplier_entry(row: SupplierRow) -> AdminDirectoryEntry {
    let phone = if row.mobile_no.trim().is_empty() {
        extract_phone_from_details(&row.supplier_details)
    } else {
        row.mobile_no.trim().to_string()
    };
    AdminDirectoryEntry {
        ref_: row.name.trim().to_string(),
        name: blank_default(&row.supplier_name, &row.name),
        phone,
    }
}

fn customer_entry(row: CustomerRow) -> AdminDirectoryEntry {
    let phone = if row.mobile_no.trim().is_empty() {
        extract_phone_from_details(&row.customer_details)
    } else {
        row.mobile_no.trim().to_string()
    };
    AdminDirectoryEntry {
        ref_: row.name.trim().to_string(),
        name: blank_default(&row.customer_name, &row.name),
        phone,
    }
}

fn supplier_item(row: ItemRow, warehouse: &str) -> SupplierItem {
    SupplierItem {
        code: row.name.trim().to_string(),
        name: blank_default(&row.item_name, &row.name),
        uom: row.stock_uom.trim().to_string(),
        warehouse: warehouse.trim().to_string(),
        item_group: row.item_group.trim().to_string(),
    }
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_phone_from_details(details: &str) -> String {
    for line in details.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("telefon:") {
            return trimmed["telefon:".len()..].trim().to_string();
        }
        if lower.starts_with("phone:") {
            return trimmed["phone:".len()..].trim().to_string();
        }
    }
    String::new()
}

fn normalize_limit(limit: usize, default: usize, max: usize) -> usize {
    if limit == 0 || limit > max {
        default
    } else {
        limit
    }
}

fn supplier_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([
        ["name", "like", like],
        ["supplier_name", "like", like],
        ["mobile_no", "like", like],
        ["supplier_details", "like", like],
    ])
    .to_string()
}

fn customer_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([
        ["name", "like", like],
        ["customer_name", "like", like],
        ["mobile_no", "like", like],
    ])
    .to_string()
}

fn item_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([["name", "like", like], ["item_name", "like", like],]).to_string()
}

fn like_pattern(query: &str) -> String {
    format!("%{}%", query.trim().replace('"', ""))
}

fn upsert_phone_in_details(_details: &str, phone: &str) -> String {
    format!("Telefon: {}", phone.trim())
}
