use async_trait::async_trait;
use reqwest::multipart;
use serde::Deserialize;

use crate::core::auth::ports::{AuthPortError, SupplierLookup, SupplierRecord};
use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
};
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

#[async_trait]
impl ProfileLookup for ErpnextClient {
    async fn get_supplier_profile(
        &self,
        id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        let payload = self
            .http
            .get(format!(
                "{}/api/resource/Supplier/{}",
                self.base_url,
                urlencoding::encode(id.trim())
            ))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| ProfilePortError::LookupFailed)?
            .json::<SupplierGetResponse>()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;

        Ok(SupplierProfileRecord {
            phone: if payload.data.mobile_no.trim().is_empty() {
                extract_phone_from_details(&payload.data.supplier_details)
            } else {
                payload.data.mobile_no.trim().to_string()
            },
            image: payload.data.image.trim().to_string(),
        })
    }

    async fn get_customer_profile(
        &self,
        id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        crate::erpnext::customers::get_customer_profile(self, id).await
    }

    async fn download_file(&self, file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        let trimmed = file_url.trim();
        if trimmed.is_empty() {
            return Err(ProfilePortError::LookupFailed);
        }
        let endpoint = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("{}{}", self.base_url, trimmed)
        };
        let response = self
            .http
            .get(endpoint)
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| ProfilePortError::LookupFailed)?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response
            .bytes()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .to_vec();

        Ok(DownloadedFile { content_type, body })
    }

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        let supplier_id = supplier_id.trim();
        if supplier_id.is_empty() || content.is_empty() {
            return Err(ProfilePortError::LookupFailed);
        }
        let filename = if filename.trim().is_empty() {
            "avatar.png"
        } else {
            filename.trim()
        };
        let content_type = if content_type.trim().is_empty() {
            "image/png"
        } else {
            content_type.trim()
        };
        let file_part = multipart::Part::bytes(content)
            .file_name(filename.to_string())
            .mime_str(content_type)
            .map_err(|_| ProfilePortError::LookupFailed)?;
        let form = multipart::Form::new()
            .text("doctype", "Supplier")
            .text("docname", supplier_id.to_string())
            .text("is_private", "0")
            .part("file", file_part);
        let payload = self
            .http
            .post(format!("{}/api/method/upload_file", self.base_url))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .header(reqwest::header::ACCEPT, "application/json")
            .multipart(form)
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| ProfilePortError::LookupFailed)?
            .json::<UploadFileResponse>()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;
        let file_url = payload.message.file_url.trim().to_string();
        if file_url.is_empty() {
            return Err(ProfilePortError::LookupFailed);
        }
        self.http
            .put(format!(
                "{}/api/resource/Supplier/{}",
                self.base_url,
                urlencoding::encode(supplier_id)
            ))
            .header(reqwest::header::AUTHORIZATION, self.auth_header())
            .json(&serde_json::json!({ "image": file_url }))
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| ProfilePortError::LookupFailed)?;
        Ok(file_url)
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

#[derive(Debug, Deserialize)]
struct SupplierGetResponse {
    data: SupplierGetRow,
}

#[derive(Debug, Deserialize)]
struct SupplierGetRow {
    #[serde(default)]
    mobile_no: String,
    #[serde(default)]
    supplier_details: String,
    #[serde(default)]
    image: String,
}

#[derive(Debug, Deserialize)]
struct UploadFileResponse {
    message: UploadedFile,
}

#[derive(Debug, Deserialize)]
struct UploadedFile {
    file_url: String,
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
