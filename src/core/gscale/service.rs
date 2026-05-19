use std::sync::Arc;

use tokio::sync::oneshot;

use super::epc::GscaleEpcGenerator;
use super::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptPrintRequest, MaterialReceiptPrintResponse,
    ScaleDriverPrintRequest, ScaleDriverPrintResponse,
};
use super::ports::{EpcSource, MaterialReceiptErpPort, ScaleDriverPort};

const MIN_BATCH_QTY_KG: f64 = 0.100;
const MAX_DUPLICATE_BARCODE_RETRIES: usize = 5;

#[derive(Clone)]
pub struct GscaleService {
    erp: Option<Arc<dyn MaterialReceiptErpPort>>,
    driver: Option<Arc<dyn ScaleDriverPort>>,
    epc: Arc<dyn EpcSource>,
}

impl Default for GscaleService {
    fn default() -> Self {
        Self {
            erp: None,
            driver: None,
            epc: Arc::new(GscaleEpcGenerator::new()),
        }
    }
}

impl GscaleService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_erp(mut self, erp: Arc<dyn MaterialReceiptErpPort>) -> Self {
        self.erp = Some(erp);
        self
    }

    pub fn with_driver(mut self, driver: Arc<dyn ScaleDriverPort>) -> Self {
        self.driver = Some(driver);
        self
    }

    #[cfg(test)]
    pub fn with_epc_source(mut self, epc: Arc<dyn EpcSource>) -> Self {
        self.epc = epc;
        self
    }

    pub async fn print_material_receipt(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let erp = self.erp.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("material receipt erp is not configured".to_string())
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("scale driver is not configured".to_string())
        })?;
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        let draft = self.create_draft_with_fresh_epc(erp.as_ref(), &job).await?;

        let print = driver
            .print_material_receipt(job.driver_request(&draft.barcode))
            .await;
        let print = match print {
            Ok(print) if print_done(&print) => print,
            Ok(print) => {
                let detail = print_error_detail(&print);
                return Err(self
                    .delete_after_print_failure(erp.as_ref(), &draft.name, detail)
                    .await);
            }
            Err(error) => {
                return Err(self
                    .delete_after_print_failure(erp.as_ref(), &draft.name, error.message())
                    .await);
            }
        };

        erp.submit_stock_entry_draft(&draft.name)
            .await
            .map_err(|error| GscaleServiceError::SubmitFailed(error.message()))?;
        Ok(MaterialReceiptPrintResponse {
            ok: true,
            status: "submitted".to_string(),
            draft_name: draft.name,
            epc: draft.barcode,
            item_code: draft.item_code,
            item_name: job.item_name,
            warehouse: draft.warehouse,
            qty: draft.qty,
            net_qty: draft.qty,
            gross_qty: job.gross_qty,
            unit: job.unit,
            printer: print.printer,
            print_mode: print.mode,
            printer_status: print.printer_status,
        })
    }

    pub async fn print_material_receipt_driver_first(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let erp = self.erp.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("material receipt erp is not configured".to_string())
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("scale driver is not configured".to_string())
        })?;
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        let epc = self.next_epc()?;
        let (print_result_tx, print_result_rx) = oneshot::channel();
        tokio::spawn(record_parallel_material_receipt(
            erp.clone(),
            job.clone(),
            epc.clone(),
            print_result_rx,
        ));
        let print = driver
            .print_material_receipt(job.driver_request(&epc))
            .await;
        let print = match print {
            Ok(print) if print_done(&print) => print,
            Ok(print) => {
                let _ = print_result_tx.send(false);
                return Err(GscaleServiceError::PrintFailed {
                    detail: print_error_detail(&print),
                    delete_error: None,
                });
            }
            Err(error) => {
                let _ = print_result_tx.send(false);
                return Err(GscaleServiceError::PrintFailed {
                    detail: error.message(),
                    delete_error: None,
                });
            }
        };
        let _ = print_result_tx.send(true);

        Ok(MaterialReceiptPrintResponse {
            ok: true,
            status: "printed".to_string(),
            draft_name: String::new(),
            epc,
            item_code: job.item_code,
            item_name: job.item_name,
            warehouse: job.warehouse,
            qty: job.net_qty,
            net_qty: job.net_qty,
            gross_qty: job.gross_qty,
            unit: job.unit,
            printer: print.printer,
            print_mode: print.mode,
            printer_status: print.printer_status,
        })
    }

    async fn create_draft_with_fresh_epc(
        &self,
        erp: &dyn MaterialReceiptErpPort,
        job: &NormalizedMaterialReceiptJob,
    ) -> Result<super::models::MaterialReceiptDraft, GscaleServiceError> {
        let mut last_error = None;
        let mut last_epc = String::new();
        for _ in 0..MAX_DUPLICATE_BARCODE_RETRIES {
            let epc = self.next_epc()?;
            last_epc = epc.clone();
            match self.create_draft_with_epc(erp, job, epc).await {
                Ok(draft) => return Ok(draft),
                Err(error) if is_duplicate_barcode_error(&error.to_string()) => {
                    last_error = Some(error.to_string());
                }
                Err(error) => return Err(error),
            }
        }
        Err(GscaleServiceError::DuplicateBarcodeRetriesExhausted {
            epc: last_epc,
            detail: last_error.unwrap_or_else(|| "duplicate retry exhausted".to_string()),
        })
    }

    async fn create_draft_with_epc(
        &self,
        erp: &dyn MaterialReceiptErpPort,
        job: &NormalizedMaterialReceiptJob,
        epc: String,
    ) -> Result<super::models::MaterialReceiptDraft, GscaleServiceError> {
        create_material_receipt_draft(erp, job, epc).await
    }

    fn next_epc(&self) -> Result<String, GscaleServiceError> {
        let epc = self.epc.next_epc().trim().to_ascii_uppercase();
        if epc.is_empty() {
            return Err(GscaleServiceError::EpcGenerationFailed);
        }
        Ok(epc)
    }

    async fn delete_after_print_failure(
        &self,
        erp: &dyn MaterialReceiptErpPort,
        draft_name: &str,
        detail: String,
    ) -> GscaleServiceError {
        let delete_error = erp
            .delete_stock_entry_draft(draft_name)
            .await
            .err()
            .map(|error| error.message());
        GscaleServiceError::PrintFailed {
            detail,
            delete_error,
        }
    }
}

async fn record_parallel_material_receipt(
    erp: Arc<dyn MaterialReceiptErpPort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
) {
    if let Err(error) = record_parallel_material_receipt_inner(erp, job, epc, print_result_rx).await
    {
        tracing::warn!(%error, "RPS batch ERP record failed after driver print");
    }
}

async fn record_parallel_material_receipt_inner(
    erp: Arc<dyn MaterialReceiptErpPort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
) -> Result<(), GscaleServiceError> {
    let draft = create_material_receipt_draft(erp.as_ref(), &job, epc).await?;
    let print_ok = print_result_rx.await.unwrap_or(false);
    if !print_ok {
        erp.delete_stock_entry_draft(&draft.name)
            .await
            .map_err(|error| GscaleServiceError::ErpWrite(error.message()))?;
        return Ok(());
    }
    erp.submit_stock_entry_draft(&draft.name)
        .await
        .map_err(|error| GscaleServiceError::SubmitFailed(error.message()))
}

async fn create_material_receipt_draft(
    erp: &dyn MaterialReceiptErpPort,
    job: &NormalizedMaterialReceiptJob,
    epc: String,
) -> Result<super::models::MaterialReceiptDraft, GscaleServiceError> {
    let input = CreateMaterialReceiptDraftInput {
        item_code: job.item_code.clone(),
        warehouse: job.warehouse.clone(),
        qty: job.net_qty,
        barcode: epc,
    };
    erp.create_material_receipt_draft(input)
        .await
        .map_err(|error| GscaleServiceError::ErpWrite(error.message()))
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedMaterialReceiptJob {
    driver_url: String,
    item_code: String,
    item_name: String,
    warehouse: String,
    printer: String,
    print_mode: String,
    gross_qty: f64,
    net_qty: f64,
    unit: String,
    tare_enabled: bool,
    tare_kg: f64,
}

impl NormalizedMaterialReceiptJob {
    fn from_request(request: MaterialReceiptPrintRequest) -> Result<Self, GscaleServiceError> {
        let item_code = request.item_code.trim().to_string();
        let warehouse = request.warehouse.trim().to_string();
        if item_code.is_empty() || warehouse.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "item_code_and_warehouse_required".to_string(),
            ));
        }
        let gross_qty = request.gross_qty;
        if !gross_qty.is_finite() || gross_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "QTY juda kichik: {gross_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let tare_enabled = request.tare_enabled || request.tare_kg > 0.0;
        let tare_kg = if tare_enabled && request.tare_kg > 0.0 {
            request.tare_kg
        } else {
            0.0
        };
        let net_qty = if tare_kg > 0.0 {
            (gross_qty - tare_kg).max(0.0)
        } else {
            gross_qty
        };
        if net_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "NETTO juda kichik: brutto {gross_qty:.3} kg - babina {tare_kg:.3} kg = {net_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let item_name = blank_default(&request.item_name, &item_code);
        Ok(Self {
            driver_url: request.driver_url.trim().to_string(),
            item_code,
            item_name,
            warehouse,
            printer: request.printer.trim().to_ascii_lowercase(),
            print_mode: request.print_mode.trim().to_ascii_lowercase(),
            gross_qty,
            net_qty,
            unit: blank_default(&request.unit, "kg"),
            tare_enabled: tare_kg > 0.0,
            tare_kg,
        })
    }

    fn driver_request(&self, epc: &str) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: epc.trim().to_ascii_uppercase(),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            warehouse: self.warehouse.clone(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: self.gross_qty,
            unit: self.unit.clone(),
            tare_enabled: self.tare_enabled,
            tare_kg: self.tare_kg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GscaleServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("epc generation failed")]
    EpcGenerationFailed,
    #[error("duplicate barcode retry exhausted: epc={epc} detail={detail}")]
    DuplicateBarcodeRetriesExhausted { epc: String, detail: String },
    #[error("erp write failed: {0}")]
    ErpWrite(String),
    #[error("print failed: {detail}")]
    PrintFailed {
        detail: String,
        delete_error: Option<String>,
    },
    #[error("submit failed: {0}")]
    SubmitFailed(String),
}

impl GscaleServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotConfigured(_) => "gscale_not_configured",
            Self::EpcGenerationFailed => "epc_generation_failed",
            Self::DuplicateBarcodeRetriesExhausted { .. } => "duplicate_barcode_retry_exhausted",
            Self::ErpWrite(_) => "erp_write_failed",
            Self::PrintFailed { .. } => "print_failed",
            Self::SubmitFailed(_) => "submit_failed",
        }
    }
}

fn print_done(print: &ScaleDriverPrintResponse) -> bool {
    print.ok && print.status.trim().eq_ignore_ascii_case("done")
}

fn print_error_detail(print: &ScaleDriverPrintResponse) -> String {
    for value in [&print.detail, &print.error, &print.status] {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "print failed".to_string()
}

fn is_duplicate_barcode_error(message: &str) -> bool {
    let msg = message.trim().to_lowercase();
    msg.contains("barcode")
        && (msg.contains("duplicate")
            || msg.contains("already exists")
            || msg.contains("unique")
            || msg.contains("duplicate entry"))
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
