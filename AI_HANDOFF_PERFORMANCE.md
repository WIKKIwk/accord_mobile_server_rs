# AI Handoff: Accord Mobile Server RS Performance State

Last updated: 2026-05-15

This document is the handoff for the next AI/engineer working inside
`accord_mobile_server_rs`. It summarizes the production-stable Rust state, the
restored ERPNext test setup, completed performance work, benchmark evidence,
hard constraints, and the next safe optimization paths.

## Current Repository State

- Branch: `main`
- Remote: `origin/main`
- Latest pushed commit: `3a449aa Benchmark local Go vs Rust on restored ERP`
- Stable pre-performance production marker:
  `b16cd32 Mark stable production version`
- Current performance commits after the stable marker:
  - `bd28894 Document ERP custom field performance contract`
  - `70a28f5 Benchmark SQL pushdown against restored ERP DB`
  - `cf74baf Use SQL pushdown for Werka reads`
  - `0697876 Add direct DB pool auto tuning`
  - `3e291d1 Parallelize independent direct DB reads`
  - `61eb588 Push down Werka status details result states`
  - `1ff7ed0 Add proven direct DB indexes`
  - `c084f97 Remove manual DB index deployment`
  - `3a449aa Benchmark local Go vs Rust on restored ERP`

Important correction about the commit history:

- `1ff7ed0` added an experimental manual index runbook.
- `c084f97` deliberately removed that deployment path.
- The final accepted rule is no manual ERPNext DB/index changes for other
  company deployments.

## Hard Constraints

Do not violate these unless Wikki explicitly changes the requirement:

- Do not require another company to manually change its ERPNext MariaDB schema.
- Do not ship manual production SQL/index instructions as a deployment
  requirement.
- Do not weaken search. This is a manufacturing/operator app; forgiving search
  is required because users may type partial, imperfect, or mixed-format text.
- Do not use sampling for correctness-critical ERP reads. Example: checking only
  10 users/items when 1000 exist is a critical ERP bug.
- `LIMIT` is valid only after the full correct filter/order logic is applied.
- ERPNext REST remains the source of truth for mutations.
- Direct MariaDB reads are allowed for read models only.
- ERPNext business code should not be edited for these Rust performance passes.

## Restored ERPNext Test Setup

Local ERPNext bench:

```text
/Volumes/Samsung990P/local.git/erpnext_n1/erp
```

Site:

```text
erpfresh.localhost
```

Site config:

```text
/Volumes/Samsung990P/local.git/erpnext_n1/erp/sites/erpfresh.localhost/site_config.json
```

ERPNext web:

```text
http://erpfresh.localhost:8000
http://127.0.0.1:8000
```

The restored DB is a production backup migrated to local Frappe/ERPNext:

- Frappe: `15.107.5`
- ERPNext: `15.108.1`
- `accord_erp_custom_field` is installed locally through the app name
  `accord_state_core`.

Current useful row counts from the restored DB:

| Table | Rows |
| --- | ---: |
| `tabItem` | 2807 |
| `tabItem Customer Detail` | 5538 |
| `tabCustomer` | 442 |
| `tabSupplier` | 49 |
| `tabDelivery Note` | 74 |
| `tabDelivery Note Item` | 74 |
| `tabPurchase Receipt` | 2 |
| `tabPurchase Receipt Item` | 2 |
| `tabBin` | 2769 |
| `tabStock Ledger Entry` | 2964 |

The DB is good for functional and local latency testing. It is not huge enough
to prove every future scaling assumption, so keep benchmarking repeatable.

## ERP Custom Field Contract

Direct DB reads depend on the Accord ERP-side custom fields. This is intended
schema, not a Rust workaround.

Fields:

- `accord_flow_state`
- `accord_customer_state`
- `accord_customer_reason`
- `accord_delivery_actor`
- `accord_status_section`
- `accord_ui_status`

The restored DB also has:

- `accord_source_key`

Important distribution in the restored DB:

| Field state | Rows |
| --- | ---: |
| total `Delivery Note` rows | 74 |
| submitted rows | 72 |
| `accord_flow_state = 1` | 73 |
| `accord_customer_state = 1` | 73 |
| `accord_ui_status = 'pending'` | 72 |

Use these fields for mobile delivery workflow reads. Do not infer current
delivery state from comments when these fields are available.

## Completed Performance Work

### 1. SQL Pushdown For Werka Reads

Commit:

```text
cf74baf Use SQL pushdown for Werka reads
```

Implemented:

- `/v1/mobile/werka/home` composes SQL-pushdown `summary` and `pending`.
- `/v1/mobile/werka/summary` counts in MariaDB with `SUM/CASE`.
- `/v1/mobile/werka/pending` filters, merges, orders, and limits in SQL.
- `/v1/mobile/werka/status-breakdown` groups and totals in SQL.

Benchmark doc:

```text
docs/benchmarks/2026-05-15-sql-pushdown.md
```

Result:

| Case | Raw/Rust median | SQL pushdown median | Speedup |
| --- | ---: | ---: | ---: |
| `summary` | 1.210ms | 0.143ms | 8.5x |
| `pending` | 0.799ms | 0.555ms | 1.4x |
| `status_breakdown:pending` | 1.353ms | 0.362ms | 3.7x |
| `status_breakdown:confirmed` | 0.864ms | 0.212ms | 4.1x |
| `status_breakdown:returned` | 0.875ms | 0.179ms | 4.9x |

Correctness:

- The benchmark compared raw-row/Rust builder output against SQL output.
- Equality passed for summary, pending IDs/order, and status breakdown totals.
- Summary result on restored DB: `pending=73 confirmed=0 returned=0`.

### 2. DB Pool Auto Tuning

Commit:

```text
0697876 Add direct DB pool auto tuning
```

Environment variables:

```env
ERP_DIRECT_DB_MAX_CONNECTIONS=32
ERP_DIRECT_DB_MIN_CONNECTIONS=4
ERP_DIRECT_DB_ACQUIRE_TIMEOUT_MS=500
ERP_DIRECT_DB_IDLE_TIMEOUT_SECONDS=60
```

Behavior:

- Explicit env values override defaults.
- Missing values are calculated from CPU/RAM.
- Defaults target safe parallelism, not unlimited load.
- `min_connections` is clamped so it cannot exceed `max_connections`.

Verification:

```text
cargo test --locked
311 passed
```

### 3. Bounded Parallel MariaDB Reads

Commit:

```text
3e291d1 Parallelize independent direct DB reads
```

Implemented:

- `history`: purchase receipts, supplier acknowledgements, and delivery notes
  are read in parallel.
- `status_details`: purchase receipts and delivery notes are read in parallel
  before the compatibility builder where needed.
- `archive`: receipt and delivery-note queries run in parallel when the archive
  kind needs both.

Benchmark on restored DB, release build, 200 requests after 30 warmups:

| Endpoint | Before median | After median | Result |
| --- | ---: | ---: | ---: |
| `history` | 0.501ms | 0.391ms | 1.28x faster |
| `status-details?kind=pending&supplier_ref=Nozimaka` | 0.430ms | 0.369ms | 1.17x faster |
| `archive?kind=returned&period=yearly` | 0.443ms | 0.369ms | 1.20x faster |
| `archive?kind=sent&period=yearly` control | 0.384ms | 0.386ms | unchanged |

### 4. `status_details` Hybrid Pushdown

Commit:

```text
61eb588 Push down Werka status details result states
```

Final accepted design:

- `pending` remains on the parallel Rust compatibility builder because full SQL
  pushdown was slower.
- `confirmed` and `returned` use SQL pushdown.

Rejected experiment:

- Full SQL pushdown for `pending` was output-correct, but slower:
  `0.405ms -> 0.665ms` median for all pending rows.

Final benchmark on restored DB, release build, 300 requests after 40 warmups:

| Endpoint | Before median | After median | Result |
| --- | ---: | ---: | ---: |
| `status-details?kind=pending` | 0.405ms | 0.400ms | unchanged/slightly faster |
| `status-details?kind=pending&supplier_ref=Nozimaka` | 0.363ms | 0.362ms | unchanged |
| `status-details?kind=confirmed` | 0.359ms | 0.238ms | 1.51x faster |
| `status-details?kind=returned` | 0.372ms | 0.244ms | 1.52x faster |

### 5. No-Touch DB Query Plan Review

Commits:

```text
1ff7ed0 Add proven direct DB indexes
c084f97 Remove manual DB index deployment
```

Final accepted design:

- No manual DB index deployment.
- No `docs/performance-indexes.sql`.
- Local experimental indexes were removed from the restored DB.
- Rust SQL was kept sargable where it does not change behavior:
  - barcode lookup uses `sed.barcode = ?`;
  - Delivery Note state filters avoid wrapping indexed columns in `COALESCE`
    where the custom fields are `NOT NULL`.

Benchmark binary:

```text
src/bin/direct_db_query_bench.rs
```

No-touch restored DB benchmark after removing experimental indexes:

| Query | Before median | After median | Result |
| --- | ---: | ---: | ---: |
| stock entry barcode predicate | 1.391ms | 1.373ms | unchanged |
| delivery pending state predicate | 0.139ms | 0.095ms | 1.46x faster |
| delivery confirmed state predicate | 0.087ms | 0.086ms | unchanged |

Rejected experiments:

- Manual MariaDB index deployment.
- Forcing customer item paging to walk `tabItem.item_name` first:
  `4.912ms -> 5.796ms`, slower.

## Latest Go vs Rust Restored-ERP Benchmark

Commit:

```text
3a449aa Benchmark local Go vs Rust on restored ERP
```

Benchmark doc:

```text
docs/benchmarks/2026-05-15-local-go-vs-rust-restored-erp.md
```

Scope:

- Go service: `127.0.0.1:18101`
- Rust service: `127.0.0.1:18102`
- ERPNext: local restored `erpfresh.localhost`
- Direct DB reads enabled for both
- Manual DB indexes not used
- Read-only endpoints plus login/session creation
- Tool: ApacheBench
- Raw output: `/private/tmp/accord_bench_current`

Preflight:

```text
go test ./...
cargo test --locked
```

Smoke:

- All selected Go and Rust endpoints returned `200`.
- `werka_summary` matched exactly:
  `pending=73 confirmed=0 returned=0`.
- supplier/customer directory and customer item picker JSON matched exactly.
- dispatch-record list endpoints had matching counts and first IDs, with known
  serialization/detail hash differences.

Load result:

| Endpoint | Rust RPS | Go RPS | Rust p95 ms | Go p95 ms | Failed |
| --- | ---: | ---: | ---: | ---: | ---: |
| `healthz n10000 c200` | 34593.82 | 44345.90 | 5 | 7 | 0 |
| `login_werka n1000 c100` | 1675.61 | 641.03 | 85 | 223 | 0 |
| `werka_summary n5000 c200` | 15891.73 | 19589.71 | 14 | 22 | 0 |
| `werka_pending n3000 c100` | 7495.24 | 6253.09 | 15 | 38 | 0 |
| `werka_status_breakdown n3000 c100` | 7347.45 | 5205.71 | 14 | 48 | 0 |
| `werka_status_details n2000 c100` | 6657.08 | 6056.00 | 31 | 36 | 0 |
| `werka_history n3000 c100` | 6401.60 | 5103.92 | 23 | 38 | 0 |
| `werka_archive_sent n1000 c50` | 9346.23 | 7958.74 | 6 | 15 | 0 |
| `werka_customers n2000 c100` | 367.68 | 374.46 | 290 | 700 | 0 |
| `werka_suppliers n2000 c100` | 12391.88 | 11566.18 | 8 | 22 | 0 |
| `werka_customer_items n1000 c50` | 740.87 | 737.41 | 82 | 168 | 0 |
| `stock_barcode n3000 c100` | 1126.14 | 1113.20 | 108 | 236 | 0 |

Conclusion:

- Rust had lower p95 latency on every measured route.
- Rust had higher throughput on most business routes.
- Go had higher raw RPS on `healthz` and `werka_summary`, but Rust still had
  lower p95 on both.
- `werka_customers` remains the heaviest read path; throughput is effectively
  equal, while Rust tail latency is much lower.

## How To Reproduce Local Benchmarks

Start ERPNext if needed:

```bash
cd /Volumes/Samsung990P/local.git/erpnext_n1/erp
bench start
```

Go service:

```bash
cd /Volumes/Samsung990P/rs/accord_mobile_server
MOBILE_API_ADDR=127.0.0.1:18101 \
ERP_DIRECT_READ_ENABLED=1 \
ERP_DIRECT_SITE_CONFIG_PATH=/Volumes/Samsung990P/local.git/erpnext_n1/erp/sites/erpfresh.localhost/site_config.json \
ERP_DIRECT_DB_HOST=127.0.0.1 \
ERP_DIRECT_DB_PORT=3306 \
MOBILE_DEV_WERKA_CODE=2000 \
WERKA_PHONE=+99888862440 \
MOBILE_API_SESSION_STORE_PATH=/private/tmp/accord_bench_current/go/sessions.json \
MOBILE_API_PROFILE_STORE_PATH=/private/tmp/accord_bench_current/go/profiles.json \
MOBILE_API_PUSH_TOKEN_STORE_PATH=/private/tmp/accord_bench_current/go/push.json \
MOBILE_API_ADMIN_SUPPLIER_STORE_PATH=/private/tmp/accord_bench_current/go/admin.json \
go run ./cmd/core
```

Rust service:

```bash
cd /Volumes/Samsung990P/rs/accord_mobile_server_rs
MOBILE_API_ADDR=127.0.0.1:18102 \
RUST_LOG=error \
ERP_DIRECT_READ_ENABLED=1 \
ERP_DIRECT_SITE_CONFIG_PATH=/Volumes/Samsung990P/local.git/erpnext_n1/erp/sites/erpfresh.localhost/site_config.json \
ERP_DIRECT_DB_HOST=127.0.0.1 \
ERP_DIRECT_DB_PORT=3306 \
MOBILE_DEV_WERKA_CODE=2000 \
WERKA_PHONE=+99888862440 \
MOBILE_API_SESSION_STORE_PATH=/private/tmp/accord_bench_current/rs/sessions.json \
MOBILE_API_PROFILE_STORE_PATH=/private/tmp/accord_bench_current/rs/profiles.json \
MOBILE_API_PUSH_TOKEN_STORE_PATH=/private/tmp/accord_bench_current/rs/push.json \
MOBILE_API_ADMIN_SUPPLIER_STORE_PATH=/private/tmp/accord_bench_current/rs/admin.json \
cargo run --release --locked --bin accord_mobile_server_rs
```

Useful read-only benchmark binaries:

```bash
cargo run --release --locked --bin sql_pushdown_bench
cargo run --release --locked --bin direct_db_query_bench
```

## Current Next Work

Priority order from the performance plan:

1. LMDB session store.
2. Optional short TTL read cache for stable picker/list data.
3. Mobile read-model table only if smaller optimizations stop being enough.

### LMDB Session Store

Reason:

- persistent sessions are currently stored in `mobile_sessions.json`;
- each login creates one token and rewrites the JSON map;
- heavy login benchmarks showed JSON session writes as a bottleneck candidate.

Required design:

- add a `SessionStore` trait/abstraction;
- keep JSON as the default/legacy fallback;
- add LMDB as an optional production backend;
- store one record per token;
- auth checks read one key;
- logout deletes one key;
- expiration cleanup must be lazy or bounded.

Do not introduce PostgreSQL/Redis just for login sessions unless Wikki changes
the architecture requirement. For the current single-node production shape,
LMDB is enough.

### Optional Read Cache

Candidate data:

- first pages of `werka_customers`;
- first pages of `werka_suppliers`;
- first pages of `werka_customer_items` for common customers;
- item group tree/list;
- admin item/customer/supplier directory pages.

Rules:

- short TTL only;
- no mutation-result caching;
- no correctness shortcuts;
- benchmark before and after;
- keep cache invalidation conservative.

### Mobile Read-Model Table

This is the largest future option and should come after smaller work:

- normalize Purchase Receipt and Delivery Note into one mobile dispatch
  projection;
- index by role/ref/status/item/date/record type;
- keep ERPNext REST mutations as source of truth;
- update projection from successful mutation paths and/or reconciliation job.

This likely requires an ERP-side app/schema contract, so it is not the next
default move.

## Verification Commands

Always run the relevant subset before committing:

```bash
cargo fmt --check
cargo test --locked
cargo run --release --locked --bin sql_pushdown_bench
cargo run --release --locked --bin direct_db_query_bench
```

For Go/Rust comparison:

```bash
go test ./...
cargo test --locked
```

Then run read-only smoke and `ab` against both services.

## Current Production Readiness Summary

The current pushed Rust service is production-candidate for the tested restored
ERPNext read-only workload:

- all Rust tests pass;
- local restored ERP smoke passes;
- Go vs Rust benchmark has zero failed requests;
- Rust has better p95 latency on all measured routes;
- manual ERPNext DB/index changes are not required;
- write/mutation source of truth remains ERPNext REST.
