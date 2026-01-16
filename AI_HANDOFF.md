# AI Handoff: accord_mobile_server_rs

Bu hujjat keyingi AI/coding agent uchun. Maqsad: Go'dagi `accord_mobile_server` loyihasini Rust'dagi `accord_mobile_server_rs` loyihasiga tartibli, testli va maksimal 1:1 behavior bilan ko'chirishni uzluksiz davom ettirish.

## Muhim Kontekst

- Foydalanuvchi tili: o'zbekcha.
- Foydalanuvchi ohangi: do'stona, lekin aniqlik va fokusni juda qadrlaydi.
- Asosiy talab: Go loyihaning behaviorini Rust'da 1:1 qilish.
- Juda muhim nuance: agar Go kodida aniq xato yoki yomon behavior bo'lsa, Rust'da uni ko'r-ko'rona ko'chirma. Hujjatlab, Rust'da to'g'riroq qil.
- Foydalanuvchi "shoshilma, chuqur o'rgan" dedi. Tezlikdan ko'ra ishonchlilik muhim.
- Har bir kichik yakunlangan slice commit qilinadi.
- Push qilinmaydi. Faqat local commit.

## Ishchi Yo'llar

- Parent workspace:
  `/home/wikki/storage/local.git/erpnext_stock_telegram`
- Go source repo:
  `/home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server`
- Rust target repo:
  `/home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server_rs`
- Porting diary:
  `/home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server/ACCORD_MOBILE_SERVER_RS_PORTING.md`
- Real ERPNext bench:
  `/home/wikki/local.git/erpnext_n1/erp`
- ERPNext site:
  `erp.localhost`
- ERPNext HTTP:
  `http://erp.localhost:8000`
- ERPNext site config:
  `/home/wikki/local.git/erpnext_n1/erp/sites/erp.localhost/site_config.json`

## Git Qoidalari

- Hech qachon push qilma, foydalanuvchi aniq aytmaguncha.
- Har yakunlangan slice local commit qilinadi.
- Commit sanasi doim `2026-01-16`, lekin vaqti commit qilinayotgan paytdagi hozirgi local vaqt bo'lishi kerak.
- Commit command namunasi:

```bash
t=$(date +%H:%M:%S) && \
GIT_AUTHOR_DATE="2026-01-16T${t}+05:00" \
GIT_COMMITTER_DATE="2026-01-16T${t}+05:00" \
git commit -m "Commit message"
```

- Oxirgi Rust commit:
  `12c0aeb Add Werka item search endpoints`
- Oxirgi Go diary commit:
  `7846cb4 Update Werka item search port status`
- Handoff yozilayotgan paytda ikkala repo ham clean edi.

## Kod Yozish Qoidalari

- Avval Go behaviorni audit qil, keyin Rust yoz.
- Birdan ko'p bo'limni aralashtirma.
- Katta fayl qilma. Imkon qadar bitta fayl 500 qatordan oshmasin.
- Manual editlar uchun `apply_patch` ishlat.
- `rg` bilan qidir.
- Existing user o'zgarishlarini revert qilma.
- Go'dagi bitta ulkan fayl patternini Rust'da takrorlama.
- Handler, service, model, DB adapter, testlarni ajratib yoz.
- Testlar bo'lmasdan slice yakunlangan hisoblanmaydi.

## Hozirgacha Port Qilingan Katta Bo'limlar

### Skeleton va Config

- Rust project skeleton.
- `axum`, `tokio`, `serde`, `time`, `tracing`.
- `MOBILE_API_ADDR` Go uslubidagi `:8081` qiymatini normalize qiladi.
- Session store env:
  asosiy `MOBILE_API_SESSION_STORE_PATH`, fallback eski `MOBILE_API_SESSION_STORE`.
- Direct DB env:
  `ERP_DIRECT_READ_ENABLED=1`,
  `ERP_DIRECT_SITE_CONFIG_PATH`,
  optional overrides:
  `ERP_DIRECT_DB_HOST`,
  `ERP_DIRECT_DB_PORT`,
  `ERP_DIRECT_DB_USER`,
  `ERP_DIRECT_DB_PASSWORD`,
  `ERP_DIRECT_DB_NAME`.
- Item response `warehouse` uchun `ERP_DEFAULT_TARGET_WAREHOUSE` Rust direct DB configga ulangan.

### Auth, Session, Profile

- Admin login:
  phone `+998880000000`, code `19621978`, ref `admin`.
- Supplier login:
  deterministic code, custom code, blocked/removed state.
- Customer login:
  custom code required.
- Werka login:
  code-driven.
- Token:
  24 random bytes + base64url no padding, 32 chars.
- Session:
  persistent + memory store, Go JSON shape bilan compatible.
- `/v1/mobile/me` profile refresh qiladi.
- Supplier/customer profile ERPNext'dan yangilanadi.
- Supplier avatar proxy:
  login/me response avatar URL `/v1/mobile/profile/avatar/view?token=...`.
- Avatar view:
  query token yoki bearer, supplier-only.

### Werka Read Endpoints

Quyidagilar Rust'da bor va testlangan:

- `/v1/mobile/werka/home`
- `/v1/mobile/werka/summary`
- `/v1/mobile/werka/pending`
- `/v1/mobile/werka/history`
- `/v1/mobile/werka/notifications`
- `/v1/mobile/werka/status-breakdown`
- `/v1/mobile/werka/status-details`
- `/v1/mobile/werka/archive`
- `/v1/mobile/werka/archive/pdf`
- `/v1/mobile/werka/suppliers`
- `/v1/mobile/werka/customers`
- `/v1/mobile/werka/supplier-items`
- `/v1/mobile/werka/customer-items`
- `/v1/mobile/werka/customer-item-options`

Muhim: Go handlerlar ko'p joyda method check qilmaydi, shuning uchun Rust'da ham Werka read/search route'lar `any(...)` bilan ulangan. POST regressiya testlari bor.

### Oxirgi Yakunlangan Slice: Werka Item Search

Qo'shilgan endpointlar:

- `/v1/mobile/werka/supplier-items`
- `/v1/mobile/werka/customer-items`
- `/v1/mobile/werka/customer-item-options`

Go contract:

- `supplier-items`:
  query params `supplier_ref`, `q`, `limit`, `offset`.
  limit default/max `100/200`.
  error text: `werka supplier items failed`.
- `customer-items`:
  query params `customer_ref`, `q`, `limit`, `offset`.
  limit default/max `100/200`.
  error text: `werka customer items failed`.
- `customer-item-options`:
  query params `q`, `limit`, `offset`.
  limit default/max `200/200`.
  error text: `werka customer item options failed`.

Rust files:

- Models:
  `src/core/werka/models.rs`
- Service/ports:
  `src/core/werka/service.rs`
  `src/core/werka/ports.rs`
- HTTP:
  `src/http/handlers/werka.rs`
  `src/http/router.rs`
  `src/http/werka_items_route_tests.rs`
- Direct DB:
  `src/erpdb/reader.rs`
  `src/erpdb/werka_lookup.rs`
  `src/erpdb/werka_items.rs`
  `src/erpdb/werka_item_search.rs`

Muhim behavior:

- Empty `q` bo'lsa SQL `LIMIT/OFFSET` bilan o'qiydi.
- Non-empty `q` bo'lsa Go'dagi `SearchQueryScore` mantiqi Rust'da port qilingan:
  transliteration, normalize, compact match, skeleton match, score tie-break.
- Customer item search non-empty queryda item code/name bilan birga linked customer refs/names ham search term bo'ladi.
- Customer item options ranking:
  avval item score, keyin customer score, keyin item name, customer name, item code.

## Test Holati

Oxirgi to'liq Rust test:

```bash
cd /home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server_rs
cargo test
```

Natija:

- `128 passed`
- `0 failed`

Eslatma:

- Cargo global cache permission warning chiqadi:
  `/home/wikki/.cargo/registry/... Permission denied`
- Bu warning test/buildni yiqitmayapti. Hozircha ishga to'siq emas.

## Real ERPNext Smoke Test

Oxirgi slice uchun ishlatilgan server command:

```bash
MOBILE_API_ADDR=127.0.0.1:18081 \
MOBILE_API_SESSION_STORE_PATH=/tmp/accord_rs_smoke_sessions.json \
MOBILE_API_ADMIN_SUPPLIER_STORE_PATH=/tmp/accord_rs_smoke_admin.json \
MOBILE_DEV_WERKA_CODE=20ABCDEF1234 \
ERP_DIRECT_READ_ENABLED=1 \
ERP_DIRECT_SITE_CONFIG_PATH=/home/wikki/local.git/erpnext_n1/erp/sites/erp.localhost/site_config.json \
ERP_DEFAULT_TARGET_WAREHOUSE='Stores - A' \
cargo run
```

Login smoke:

```bash
TOKEN=$(curl -sS -X POST http://127.0.0.1:18081/v1/mobile/auth/login \
  -H 'content-type: application/json' \
  -d '{"phone":"+99888862440","code":"20ABCDEF1234"}' | jq -r .token)
```

Tasdiqlangan:

- token length `32`.
- `GET /v1/mobile/werka/customer-items?customer_ref=zenit%20morojniy&limit=5&offset=0`
  count `2`, first `zenit frutto ninja 70 gr`.
- Searchli customer-items ham 200 qaytdi.
- `GET /v1/mobile/werka/customer-item-options?limit=5&offset=0`
  count `5`, first item Go SQL bilan mos.
- Real bazada supplier assignment topilmadi, shuning uchun:
  `GET /v1/mobile/werka/supplier-items?supplier_ref=&limit=5&offset=0`
  count `0`, 200 qaytdi.

Go SQL solishtirish uchun Frappe Python pattern:

```bash
cd /home/wikki/local.git/erpnext_n1/erp && ./env/bin/python - <<'PY'
import frappe
frappe.init(site='erp.localhost', sites_path='/home/wikki/local.git/erpnext_n1/erp/sites')
frappe.connect()
# frappe.db.sql(...) shu yerda
frappe.destroy()
PY
```

## Keyingi Eng Mantiqiy Nishon

Keyingi endpoint Go'da:

```go
func (s *Server) handleWerkaCustomerIssueCreate(w http.ResponseWriter, r *http.Request)
```

Go file:

```text
/home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server/internal/mobileapi/server.go
```

Boshlanish atrofida:

```text
handleWerkaCustomerIssueCreate
```

Nima qiladi:

- Pathni routerdan topish kerak.
- Method aniq `POST`, aks holda `405 {"error":"method not allowed"}`.
- Werka auth required.
- JSON decode error:
  `400 {"error":"invalid json"}`.
- Core method:
  `CreateWerkaCustomerIssueWithSource`.
- Input:
  customer ref, item code, qty, source barcode, stock entry name, source line index.
- Errorlar:
  insufficient stock -> `409` with:
  `{"error":"insufficient stock","error_code":"insufficient_stock"}`
  duplicate source -> `409` with:
  `{"error":"duplicate customer issue source","error_code":"duplicate_customer_issue_source"}`
  default internal:
  `500 {"error":"werka customer issue create failed"}`
- Successdan keyin push yuborishga urinadi, push xatosi response'ni yiqitmasligi mumkin. Go kodini audit qil.

Bu read-only emas. Bu write flow. Juda ehtiyot bo'lish kerak:

- Avval Go core methodni chuqur o'rgan:
  `CreateWerkaCustomerIssueWithSource`
  `WerkaCustomerIssueCreateInput`
  `WerkaCustomerIssueSource`
  duplicate source detection
  stock check
  Delivery Note yoki Stock Entry yaratish flow'i
- Keyin Rust model/service/ports yoz.
- Real ERPNext test qilishdan oldin unit testlarni kuchli qil.
- Real ERPNext testda production-like data buzilmasin. Kerak bo'lsa vaqtinchalik test customer/item yaratiladi va cleanup qilinadi, yoki faqat fake adapter tests bilan boshlanadi.

## Qayerdan Audit Boshlash

Go repo ichida:

```bash
cd /home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server
rg -n "handleWerkaCustomerIssueCreate|CreateWerkaCustomerIssueWithSource|WerkaCustomerIssueCreateInput|WerkaCustomerIssueSource|DuplicateCustomerIssue|InsufficientStock" internal -S
```

Keyin tegishli fayllarni o'qish:

- `internal/mobileapi/server.go`
- `internal/core/service.go`
- `internal/erpdb/customer_issue.go`
- `internal/erpnext/delivery_note.go`
- kerak bo'lsa `internal/mobileapi/server_test.go`

## Hozirgi Progress Taxmini

Aniq foiz aytish qiyin, chunki write endpointlar murakkabroq. Read/auth/Werka report/search qismi ancha yopildi. Umumiy server behavior bo'yicha taxminiy progress:

- Auth/session/profile: katta qismi bor.
- Werka read/report/search: katta qismi bor.
- Supplier/customer operational write flows: hali ko'p qismi qolgan.
- Admin/import/AI/search/push/stock-entry/customer issue kabi write/integration qismlar: hali chuqur audit kerak.

Taxminan 45-55% atrofida deb qarash mumkin, lekin bu yakuniy emas. Qolgan write flowlar hajmni keskin oshirishi mumkin.

## Keyingi AI Uchun Qisqa Start Plan

1. Rust repo statusini tekshir:

```bash
cd /home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server_rs
git status --short
```

2. Go diary statusini tekshir:

```bash
cd /home/wikki/storage/local.git/erpnext_stock_telegram/accord_mobile_server
git status --short
```

3. Go'da `handleWerkaCustomerIssueCreate` va core methodni audit qil.
4. Rust'da faqat shu endpoint uchun minimal model/port/service/handler qo'sh.
5. Fake/unit/route testlar bilan yop.
6. Zarur bo'lsa real ERPNext smoke testni juda ehtiyotkor qil.
7. `cargo test` yurit.
8. Rust commit qil.
9. Go porting diaryni update qil va alohida commit qil.
10. Push qilma.

## Oxirgi Eslatma

Foydalanuvchi ishning "1 ga 1" bo'lishini juda jiddiy talab qiladi. Shuning uchun har bir endpointda quyidagilarni alohida tekshir:

- path
- method behavior
- auth role
- query/body parsing
- default limit/offset
- status code
- error text
- JSON field names
- empty-provider behavior
- POST/GET farqlari
- Go'dagi bug yoki noaniq joy bo'lsa Rust'da yaxshilash va hujjatlash
