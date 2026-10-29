//! 库存管理模块 (inventory management).
//!
//! Prices are stored in cents to avoid floating point rounding.
//! 所有金额均以“分”为单位存储。

use std::collections::HashMap;
use std::fmt;

/// Quantity at which the bulk discount starts to apply.
pub const BULK_THRESHOLD: u32 = 10;
/// Bulk discount in percent.
pub const BULK_DISCOUNT_PCT: u64 = 15;
/// Maximum number of distinct SKUs in one order.
pub const MAX_LINES: usize = 200;

/// 商品条目
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub sku: String,
    pub name: String,
    /// Unit price in cents.
    pub price: u64,
    pub quantity: u32,
    pub tags: Vec<String>,
}

impl Item {
    pub fn new(sku: &str, name: &str, price: u64, quantity: u32) -> Self {
        Item { sku: sku.to_string(), name: name.to_string(), price, quantity, tags: vec![] }
    }

    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Line subtotal before discounts.
    pub fn subtotal(&self) -> u64 {
        self.price * self.quantity as u64
    }

    pub fn is_taxable(&self) -> bool {
        !self.tags.iter().any(|t| t == "tax-free" || t == "免税")
    }
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x{} @ {}", self.name, self.quantity, format_cents(self.price))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InventoryError {
    UnknownSku(String),
    OutOfStock { sku: String, wanted: u32, available: u32 },
    TooManyLines(usize),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InventoryError::UnknownSku(s) => write!(f, "unknown SKU {s}"),
            InventoryError::OutOfStock { sku, wanted, available } => {
                write!(f, "{sku}: wanted {wanted}, only {available} in stock")
            }
            InventoryError::TooManyLines(n) => write!(f, "order has {n} lines (max {MAX_LINES})"),
        }
    }
}

impl std::error::Error for InventoryError {}

/// 仓库:SKU → 库存数量
#[derive(Debug, Default)]
pub struct Warehouse {
    stock: HashMap<String, u32>,
    reserved: HashMap<String, u32>,
}

impl Warehouse {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn restock(&mut self, sku: &str, qty: u32) {
        *self.stock.entry(sku.to_string()).or_insert(0) += qty;
    }

    pub fn available(&self, sku: &str) -> u32 {
        let total = self.stock.get(sku).copied().unwrap_or(0);
        let held = self.reserved.get(sku).copied().unwrap_or(0);
        total.saturating_sub(held)
    }

    /// Reserve stock for every item of an order, all or nothing.
    pub fn reserve(&mut self, items: &[Item]) -> Result<(), InventoryError> {
        if items.len() > MAX_LINES {
            return Err(InventoryError::TooManyLines(items.len()));
        }
        for it in items {
            if !self.stock.contains_key(&it.sku) {
                return Err(InventoryError::UnknownSku(it.sku.clone()));
            }
            let available = self.available(&it.sku);
            if available < it.quantity {
                return Err(InventoryError::OutOfStock {
                    sku: it.sku.clone(),
                    wanted: it.quantity,
                    available,
                });
            }
        }
        for it in items {
            *self.reserved.entry(it.sku.clone()).or_insert(0) += it.quantity;
        }
        Ok(())
    }

    pub fn release(&mut self, items: &[Item]) {
        for it in items {
            if let Some(r) = self.reserved.get_mut(&it.sku) {
                *r = r.saturating_sub(it.quantity);
            }
        }
    }
}

/// 计算订单总价(含批量折扣),单位:分
pub fn calc_total(items: &[Item]) -> u64 {
    items.iter().map(|it| apply_discount(it.subtotal(), it.quantity)).sum()
}

/// Apply the bulk discount to a line subtotal.
pub fn apply_discount(subtotal: u64, quantity: u32) -> u64 {
    if quantity > BULK_THRESHOLD {
        subtotal - subtotal * BULK_DISCOUNT_PCT / 100
    } else {
        subtotal
    }
}

/// Total of taxable lines only.
pub fn taxable_total(items: &[Item]) -> u64 {
    let taxable: Vec<Item> = items.iter().filter(|i| i.is_taxable()).cloned().collect();
    calc_total(&taxable)
}

/// Tax in cents for the given rate in basis points (1% = 100).
pub fn tax_for(items: &[Item], rate_bp: u64) -> u64 {
    (taxable_total(items) * rate_bp + 5_000) / 10_000
}

/// Format cents as `¥12.34`.
pub fn format_cents(cents: u64) -> String {
    format!("¥{}.{:02}", cents / 100, cents % 100)
}

/// Group items by their first tag ("untagged" when none).
pub fn group_by_tag(items: &[Item]) -> HashMap<String, Vec<&Item>> {
    let mut groups: HashMap<String, Vec<&Item>> = HashMap::new();
    for it in items {
        let key = it.tags.first().cloned().unwrap_or_else(|| "untagged".to_string());
        groups.entry(key).or_default().push(it);
    }
    groups
}

/// Merge lines with the same SKU, summing quantities.
pub fn merge_lines(items: &[Item]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for it in items {
        match out.iter_mut().find(|o| o.sku == it.sku) {
            Some(o) => o.quantity += it.quantity,
            None => out.push(it.clone()),
        }
    }
    out
}

/// Human readable receipt.
pub fn receipt(items: &[Item]) -> String {
    let mut s = String::new();
    for it in items {
        s.push_str(&format!("{it}\n"));
    }
    s.push_str(&format!("合计: {}\n", format_cents(calc_total(items))));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Item> {
        vec![
            Item::new("A-1", "螺丝刀", 1_250, 2),
            Item::new("B-7", "扳手", 3_000, 10),
            Item::new("C-3", "Gloves", 499, 1).with_tag("免税"),
        ]
    }

    #[test]
    fn totals() {
        let items = sample();
        assert_eq!(apply_discount(1_000, 1), 1_000);
        assert_eq!(calc_total(&items[..1]), 2_500);
    }

    #[test]
    fn bulk_discount_at_threshold() {
        assert_eq!(apply_discount(30_000, BULK_THRESHOLD), 25_500);
    }

    #[test]
    fn reserve_all_or_nothing() {
        let mut w = Warehouse::new();
        w.restock("A-1", 5);
        let items = vec![Item::new("A-1", "螺丝刀", 1_250, 3), Item::new("Z-9", "?", 1, 1)];
        assert!(w.reserve(&items).is_err());
        assert_eq!(w.available("A-1"), 5);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_cents(123_456), "¥1234.56");
        assert!(receipt(&sample()).contains("合计"));
    }
}
