use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::core::gscale::models::{MaterialReceiptDraft, ScaleDriverPrintResponse};

fn request() -> MaterialReceiptPrintRequest {
    MaterialReceiptPrintRequest {
        driver_url: "http://127.0.0.1:39117".to_string(),
        item_code: " ITEM-1 ".to_string(),
        item_name: " Green Tea ".to_string(),
        warehouse: " Stores - A ".to_string(),
        printer: "zebra".to_string(),
        print_mode: "rfid".to_string(),
        gross_qty: 2.5,
        unit: String::new(),
        tare_enabled: true,
        tare_kg: 0.78,
    }
}

#[tokio::test]
async fn creates_draft_prints_then_submits_like_gscale() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let erp = Arc::new(FakeErp::new(events.clone()));
    let driver = Arc::new(FakeDriver::done(events.clone()));
    let service = GscaleService::new()
        .with_erp(erp.clone())
        .with_driver(driver.clone())
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-1"])));

    let response = service.print_material_receipt(request()).await.unwrap();

    assert_eq!(response.status, "submitted");
    assert_eq!(response.draft_name, "MAT-STE-001");
    assert_eq!(response.epc, "EPC-1");
    assert_eq!(response.qty, 1.72);
    assert_eq!(driver.last().epc, "EPC-1");
    assert_eq!(driver.last().gross_qty, 2.5);
    assert_eq!(driver.last().tare_kg, 0.78);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:EPC-1:1.720", "print:EPC-1", "submit:MAT-STE-001"]
    );
}

#[tokio::test]
async fn deletes_draft_when_driver_fails_before_submit() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let erp = Arc::new(FakeErp::new(events.clone()));
    let driver = Arc::new(FakeDriver::status_error(events.clone()));
    let service = GscaleService::new()
        .with_erp(erp)
        .with_driver(driver)
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-1"])));

    let error = service.print_material_receipt(request()).await.unwrap_err();

    assert!(matches!(error, GscaleServiceError::PrintFailed { .. }));
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:EPC-1:1.720", "print:EPC-1", "delete:MAT-STE-001"]
    );
}

#[tokio::test]
async fn submit_failure_does_not_delete_already_printed_draft() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut erp = FakeErp::new(events.clone());
    erp.submit_error = Some("submit failed".to_string());
    let service = GscaleService::new()
        .with_erp(Arc::new(erp))
        .with_driver(Arc::new(FakeDriver::done(events.clone())))
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-1"])));

    let error = service.print_material_receipt(request()).await.unwrap_err();

    assert!(matches!(error, GscaleServiceError::SubmitFailed(_)));
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:EPC-1:1.720", "print:EPC-1", "submit:MAT-STE-001"]
    );
}

#[tokio::test]
async fn retries_duplicate_barcode_before_printing() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut erp = FakeErp::new(events.clone());
    erp.duplicate_failures = 1;
    let service = GscaleService::new()
        .with_erp(Arc::new(erp))
        .with_driver(Arc::new(FakeDriver::done(events.clone())))
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-1", "EPC-2"])));

    let response = service.print_material_receipt(request()).await.unwrap();

    assert_eq!(response.epc, "EPC-2");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "create:EPC-1:1.720",
            "create:EPC-2:1.720",
            "print:EPC-2",
            "submit:MAT-STE-001",
        ]
    );
}

#[tokio::test]
async fn rejects_small_gross_and_net_qty_before_erp() {
    let service = GscaleService::new()
        .with_erp(Arc::new(FakeErp::new(Arc::new(Mutex::new(Vec::new())))))
        .with_driver(Arc::new(FakeDriver::done(Arc::new(Mutex::new(Vec::new())))));
    let mut gross = request();
    gross.gross_qty = 0.099;
    let mut net = request();
    net.gross_qty = 0.5;
    net.tare_kg = 0.45;

    let gross_error = service.print_material_receipt(gross).await.unwrap_err();
    let net_error = service.print_material_receipt(net).await.unwrap_err();

    assert_eq!(
        gross_error.to_string(),
        "invalid input: QTY juda kichik: 0.099 kg | min 0.100 kg"
    );
    assert_eq!(
        net_error.to_string(),
        "invalid input: NETTO juda kichik: brutto 0.500 kg - babina 0.450 kg = 0.050 kg | min 0.100 kg"
    );
}

struct QueueEpc {
    values: Mutex<VecDeque<String>>,
}

impl QueueEpc {
    fn new(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().map(Into::into).collect()),
        }
    }
}

impl EpcSource for QueueEpc {
    fn next_epc(&self) -> String {
        self.values.lock().unwrap().pop_front().unwrap_or_default()
    }
}

struct FakeErp {
    events: Arc<Mutex<Vec<String>>>,
    duplicate_failures: usize,
    submit_error: Option<String>,
}

impl FakeErp {
    fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            duplicate_failures: 0,
            submit_error: None,
        }
    }
}

#[async_trait]
impl MaterialReceiptErpPort for FakeErp {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{}:{:.3}", input.barcode, input.qty));
        if self.duplicate_failures > 0
            && self.events.lock().unwrap().len() <= self.duplicate_failures
        {
            return Err(GscalePortError::ErpWrite(
                "barcode duplicate entry".to_string(),
            ));
        }
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-001".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        if let Some(error) = &self.submit_error {
            return Err(GscalePortError::ErpWrite(error.clone()));
        }
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FakeDriver {
    events: Arc<Mutex<Vec<String>>>,
    status: &'static str,
    ok: bool,
    last: Mutex<Option<ScaleDriverPrintRequest>>,
}

impl FakeDriver {
    fn done(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            status: "done",
            ok: true,
            last: Mutex::new(None),
        }
    }

    fn status_error(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            status: "error",
            ok: false,
            last: Mutex::new(None),
        }
    }

    fn last(&self) -> ScaleDriverPrintRequest {
        self.last.lock().unwrap().clone().unwrap()
    }
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("print:{}", request.epc));
        *self.last.lock().unwrap() = Some(request.clone());
        Ok(ScaleDriverPrintResponse {
            ok: self.ok,
            status: self.status.to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: self.status.to_string(),
            detail: "printer rejected".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}
