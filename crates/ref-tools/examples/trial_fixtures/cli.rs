//! Command line front-end for the inventory module.

use crate::inventory::{calc_total, format_cents, tax_for, Item, Warehouse};

const USAGE: &str = "usage: inv <total|tax|reserve> FILE";

/// Parse `sku,name,price,qty[,tag]` lines.
pub fn parse_items(text: &str) -> Result<Vec<Item>, String> {
    let mut items = vec![];
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 4 {
            return Err(format!("line {}: expected at least 4 columns", i + 1));
        }
        let price: u64 = cols[2].parse().map_err(|_| format!("line {}: bad price", i + 1))?;
        let qty: u32 = cols[3].parse().map_err(|_| format!("line {}: bad quantity", i + 1))?;
        let mut item = Item::new(cols[0], cols[1], price, qty);
        if let Some(tag) = cols.get(4) {
            item = item.with_tag(tag);
        }
        items.push(item);
    }
    Ok(items)
}

pub fn run(args: &[String]) -> Result<String, String> {
    let (cmd, file) = match args {
        [cmd, file] => (cmd.as_str(), file.as_str()),
        _ => return Err(USAGE.to_string()),
    };
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let items = parse_items(&text)?;
    match cmd {
        "total" => Ok(format!("total: {}", format_cents(calc_total(&items)))),
        "tax" => {
            let tax = tax_for(&items, 600);
            let grand = calc_total(&items) + tax;
            Ok(format!("tax: {}\ngrand total: {}", format_cents(tax), format_cents(grand)))
        }
        "reserve" => {
            let mut w = Warehouse::new();
            for it in &items {
                w.restock(&it.sku, 100);
            }
            w.reserve(&items).map_err(|e| e.to_string())?;
            Ok(format!("reserved {} lines", items.len()))
        }
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_total() {
        let items = parse_items("A,a,100,2\n# comment\nB,b,50,1,免税\n").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(calc_total(&items), 250);
    }
}
