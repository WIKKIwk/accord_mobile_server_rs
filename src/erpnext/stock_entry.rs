use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::gscale::models::{CreateMaterialReceiptDraftInput, MaterialReceiptDraft};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptErpPort};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl MaterialReceiptErpPort for ErpnextClient {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        validate_create_input(&input)?;
        let company = self.stock_entry_warehouse_company(&input.warehouse).await?;
        let uom = blank_default(&self.stock_entry_item_uom(&input.item_code).await?, "Kg");
        let item = serde_json::json!({
            "item_code": input.item_code.trim(),
            "t_warehouse": input.warehouse.trim(),
            "qty": input.qty,
            "uom": uom,
            "stock_uom": uom,
            "conversion_factor": 1,
            "barcode": input.barcode.trim().to_ascii_uppercase(),
        });
        let payload = serde_json::json!({
            "stock_entry_type": "Material Receipt",
            "company": company,
            "to_warehouse": input.warehouse.trim(),
            "items": [item],
        });
        let created: ResourceResponse<NameRow> = self
            .stock_entry_json_request(Method::POST, "/api/resource/Stock Entry", Some(payload))
            .await?;
        let name = created.data.name.trim().to_string();
        if name.is_empty() {
            return Err(GscalePortError::ErpWrite(
                "erp stock entry name bo'sh".to_string(),
            ));
        }
        Ok(MaterialReceiptDraft {
            name,
            item_code: input.item_code.trim().to_string(),
            warehouse: input.warehouse.trim().to_string(),
            qty: input.qty,
            uom,
            barcode: input.barcode.trim().to_ascii_uppercase(),
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let path = stock_entry_path(name)?;
        let mut last_error = None;
        for attempt in 0..2 {
            let latest: ResourceResponse<Value> = self
                .stock_entry_json_request(Method::GET, &path, None)
                .await?;
            let payload = serde_json::json!({ "doc": latest.data });
            match self
                .stock_entry_empty_request(
                    Method::POST,
                    "/api/method/frappe.client.submit",
                    Some(payload),
                )
                .await
            {
                Ok(()) => return Ok(()),
                Err(error)
                    if attempt == 0 && error.to_string().contains("TimestampMismatchError") =>
                {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            GscalePortError::ErpWrite("erp stock entry submit failed".to_string())
        }))
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let path = stock_entry_path(name)?;
        self.stock_entry_empty_request(Method::DELETE, &path, None)
            .await
    }
}

impl ErpnextClient {
    async fn stock_entry_warehouse_company(
        &self,
        warehouse: &str,
    ) -> Result<String, GscalePortError> {
        let payload: ResourceResponse<WarehouseRow> = self
            .stock_entry_json_request(
                Method::GET,
                &format!(
                    "/api/resource/Warehouse/{}",
                    urlencoding::encode(warehouse.trim())
                ),
                None,
            )
            .await?;
        let company = payload.data.company.trim().to_string();
        if company.is_empty() {
            Err(GscalePortError::ErpWrite(
                "warehouse company is not configured".to_string(),
            ))
        } else {
            Ok(company)
        }
    }

    async fn stock_entry_item_uom(&self, item_code: &str) -> Result<String, GscalePortError> {
        let payload: ResourceResponse<ItemRow> = self
            .stock_entry_json_request(
                Method::GET,
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                None,
            )
            .await?;
        Ok(payload.data.stock_uom.trim().to_string())
    }

    async fn stock_entry_json_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<T, GscalePortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
        decode_response(response).await
    }

    async fn stock_entry_empty_request(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<(), GscalePortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
        decode_empty_response(response).await
    }
}

fn validate_create_input(input: &CreateMaterialReceiptDraftInput) -> Result<(), GscalePortError> {
    if input.item_code.trim().is_empty() || input.warehouse.trim().is_empty() {
        return Err(GscalePortError::InvalidInput(
            "item code and warehouse are required".to_string(),
        ));
    }
    if input.qty <= 0.0 {
        return Err(GscalePortError::InvalidInput(
            "qty must be greater than 0".to_string(),
        ));
    }
    Ok(())
}

fn stock_entry_path(name: &str) -> Result<String, GscalePortError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(GscalePortError::InvalidInput(
            "stock entry name is required".to_string(),
        ));
    }
    Ok(format!(
        "/api/resource/Stock Entry/{}",
        urlencoding::encode(name)
    ))
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, GscalePortError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
    if !status.is_success() {
        return Err(GscalePortError::ErpWrite(body));
    }
    serde_json::from_str(&body).map_err(|error| GscalePortError::ErpWrite(error.to_string()))
}

async fn decode_empty_response(response: reqwest::Response) -> Result<(), GscalePortError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
    if status.is_success() {
        Ok(())
    } else {
        Err(GscalePortError::ErpWrite(body))
    }
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[derive(Debug, Deserialize)]
struct ResourceResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct NameRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct WarehouseRow {
    #[serde(default)]
    company: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    #[serde(default)]
    stock_uom: String,
}
