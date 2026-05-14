# Accord Mobile Backend Performance Plan

This document tracks the performance plan for `accord_mobile_server_rs`.
It is intentionally kept as a living note while the plan is discussed.

## Current Hot-Path Behavior

The Rust backend is already faster than the legacy Go backend on the benchmarked
read-heavy paths, but some endpoints still do more work than necessary.

The current pattern in several Werka endpoints is:

1. MariaDB returns raw `Purchase Receipt` and `Delivery Note` rows.
2. Rust converts rows into domain records.
3. Rust classifies status, counts totals, groups records, sorts records, and
   truncates results.

Examples:

- `werka_home` loads receipt rows and delivery-note rows, then builds summary
  and pending items in Rust.
- `werka_summary` loads status rows from both tables, then counts pending,
  confirmed, and returned in Rust.
- `werka_status_breakdown` loads receipt and delivery-note rows, converts all
  to dispatch records, then groups by supplier/customer in Rust.
- `werka_status_details` loads receipt and delivery-note rows, then filters by
  `kind` and `supplier_ref` in Rust.
- `werka_pending` filters some state in SQL, but final classification, merge,
  sort, and truncation still happen in Rust.

This keeps business compatibility easy to reason about, but it means the service
can fetch more rows than the mobile response needs.

## Restored ERPNext DB Snapshot

The local `erpfresh.localhost` bench has been restored from the production
backup and migrated to the updated local Frappe/ERPNext code.

Current restored table counts:

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

Largest business tables by size are currently stock/item related:

- `tabStock Ledger Entry`
- `tabStock Entry Detail`
- `tabItem`
- `tabItem Customer Detail`
- `tabBin`

Index observations:

- child-table joins already have `parent` indexes on delivery-note and
  purchase-receipt item tables;
- `tabBin` has a unique `(item_code, warehouse)` index, which is good for stock
  lookups;
- `tabItem Customer Detail` has indexes on `customer_name`, `ref_code`, and
  `parent`;
- `tabPurchase Receipt.supplier_delivery_note` is not indexed, but this dump has
  only two purchase receipts, so it is not a current bottleneck;
- `tabDelivery Note` has indexes on `customer`, `posting_date`, `status`, and
  `modified`, but not on the Accord custom state fields.

Immediate DB conclusion:

- this restored DB is good enough for functional and local latency tests;
- the biggest current read pressure is not search;
- for Werka dashboard paths, the main waste is still fetching rows into Rust and
  then counting/filtering/grouping there;
- for item/customer/stock picker paths, index review and optional cache are more
  relevant than SQL pushdown.

## Search Decision

Search is intentionally out of scope for this performance pass.

The app is an operator/manufacturing workflow, so search must stay forgiving.
Operators may type partial names, incomplete codes, mixed formats, or imperfect
text. Weakening search is a product regression.

For now:

- do not remove fuzzy/tolerant search behavior;
- do not replace current search with strict prefix-only matching;
- do not optimize search at the cost of findability.

## Performance Work Items

### 1. LMDB Session Store

Problem:

- current persistent sessions are stored in `mobile_sessions.json`;
- every login creates one token but rewrites the whole JSON map;
- heavy login benchmarks showed this as a clear bottleneck.

Plan:

- add a `SessionStore` abstraction;
- keep JSON as a legacy fallback;
- add LMDB as the production session backend;
- store each session as `token -> session record`;
- read one key for auth checks;
- delete one key on logout;
- lazily clean expired records or clean them in a bounded background task.

Expected result:

- login storm no longer rewrites a large JSON file;
- session read/write becomes small key-value operations;
- no external PostgreSQL/Redis service is required for the current single-node
  production shape.

### 2. DB Pool Auto Tuning

Problem:

- direct DB pool is currently hardcoded to a fixed max connection count;
- one static value cannot be optimal for every server size.

Plan:

- make pool settings configurable through environment variables;
- compute safe defaults from CPU/RAM when env values are missing;
- keep explicit env override for production tuning.

Candidate env vars:

```env
ERP_DIRECT_DB_MAX_CONNECTIONS=32
ERP_DIRECT_DB_MIN_CONNECTIONS=4
ERP_DIRECT_DB_ACQUIRE_TIMEOUT_MS=500
ERP_DIRECT_DB_IDLE_TIMEOUT_SECONDS=60
```

Default rule should target optimal parallelism, not unlimited maximum load.
More DB connections can make MariaDB slower if the database becomes saturated.

### 3. Parallel MariaDB Reads

Problem:

- several endpoints run independent receipt and delivery-note queries
  sequentially.

Plan:

- use bounded parallel query execution for independent reads;
- start with endpoints where receipt and delivery-note reads are independent:
  `home`, `summary`, `history`, `archive`, `status_breakdown`, and
  `status_details`;
- keep query count bounded so one request cannot explode into unbounded DB work.

Expected result:

- lower endpoint latency when two independent DB reads are needed;
- no mobile contract change.

### 4. SQL Pushdown

Problem:

- some endpoints fetch many rows and count/group/filter in Rust;
- this is easy to maintain but can waste DB/network/Rust CPU work.

Plan:

- move safe counting/filtering/grouping into SQL where it does not change
  mobile-visible behavior;
- keep complex compatibility rules covered by tests;
- compare SQL output against current Rust builder functions before replacing
  production paths.

Priority endpoints:

- `/v1/mobile/werka/summary`
- `/v1/mobile/werka/status-breakdown`
- `/v1/mobile/werka/status-details`
- `/v1/mobile/werka/pending`

Expected result:

- fewer rows transferred from MariaDB;
- less Rust-side sorting/grouping work;
- better behavior under high concurrency.

### 5. Index Review

Problem:

- query performance depends on real MariaDB indexes and query plans;
- code inspection alone is not enough to prove the optimal index set.

Plan:

- run `SHOW INDEX` and `EXPLAIN` on the production-like bench;
- add only indexes that are justified by real query plans;
- avoid ad-hoc production DB changes without a runbook or migration path.

Candidate areas:

- `Purchase Receipt` marker/status/date/supplier reads;
- `Delivery Note` flow/customer/status/date reads;
- child-table joins on `parent` and `idx`;
- item/customer/supplier relation tables used by mobile pickers.

### 6. Optional Read Cache

Problem:

- some data changes rarely but is read frequently.

Plan:

- add short TTL cache only for stable read paths;
- keep cache invalidation conservative;
- do not cache mutation results.

Candidate endpoints/data:

- item group tree;
- item group list;
- first pages of admin item lists;
- first pages of supplier/customer directories.

Expected result:

- lower repeated-read load;
- faster mobile navigation for common screens.

### 7. Mobile Read-Model Table

Problem:

- dashboard, history, archive, pending, and status screens repeatedly derive a
  mobile view from ERPNext document tables.

Plan:

- create a dedicated mobile read model such as `accord_mobile_dispatch_read`;
- normalize `Purchase Receipt` and `Delivery Note` into one mobile dispatch
  projection;
- index by role/ref/status/item/date/record type;
- keep ERPNext REST as the mutation source of truth;
- update the read model from successful mutation paths and/or a reconciliation
  job.

Expected result:

- largest possible read-path improvement;
- simpler SQL for mobile screens;
- bigger architecture change than the earlier steps, so it should come later.

## Current Priority Order

1. LMDB session store.
2. DB pool auto tuning.
3. Parallel MariaDB reads.
4. SQL pushdown.
5. Index review.
6. Optional read cache.
7. Mobile read-model table.

## DB-Informed Execution Order

After inspecting the restored ERPNext DB, the practical execution order should
be:

1. Add DB pool auto tuning first, because it is low-risk and affects every
   direct DB path.
2. Add bounded parallel MariaDB reads for independent receipt/delivery queries.
3. Add SQL pushdown for Werka `summary`, `status_breakdown`, `status_details`,
   and `pending`, with tests comparing output against the current Rust builders.
4. Review and add only proven indexes after running `EXPLAIN` on the exact
   query shapes.
5. Add LMDB session store for login-heavy benchmarks.
6. Add short TTL read cache only for stable picker/list endpoints.
7. Consider a mobile read-model table only after the smaller optimizations are
   measured.
