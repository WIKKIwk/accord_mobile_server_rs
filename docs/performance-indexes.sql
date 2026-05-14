-- MariaDB indexes proven against the restored ERPNext test DB for Accord mobile
-- direct-read hot paths. Run during a maintenance window and benchmark after.

CREATE INDEX IF NOT EXISTS accord_sed_barcode_idx
    ON `tabStock Entry Detail` (`barcode`);

CREATE INDEX IF NOT EXISTS accord_item_supplier_supplier_parent_idx
    ON `tabItem Supplier` (`supplier`, `parent`);

CREATE INDEX IF NOT EXISTS accord_icd_customer_parent_idx
    ON `tabItem Customer Detail` (`customer_name`, `parent`);

CREATE INDEX IF NOT EXISTS accord_dn_mobile_state_idx
    ON `tabDelivery Note` (
        `docstatus`,
        `accord_flow_state`,
        `accord_customer_state`,
        `modified`
    );
