# 库存工具 (inventory toolkit)

这个仓库包含一个小型的库存与订单计算库,以及一组统计辅助函数。
The crate is intentionally dependency-free so it can be vendored easily.

## 快速开始

```rust
use inventory::{calc_total, Item};

let items = vec![Item::new("A-1", "螺丝刀", 1_250, 2)];
println!("{}", calc_total(&items)); // 2500
```

命令行用法:

```
inv total orders.csv
inv tax orders.csv
```

## 金额

- 所有金额均以“分”为单位(`u64`)。
- `format_cents` 把金额格式化为 `¥12.34`。
- 批量折扣:数量达到 `BULK_THRESHOLD`(10)时打 85 折。

## Rust API

| 函数 | 说明 |
| --- | --- |
| `calc_total(items)` | 订单总价(含批量折扣) |
| `apply_discount(subtotal, qty)` | 单行折扣 |
| `taxable_total(items)` | 应税部分合计 |
| `tax_for(items, rate_bp)` | 税额,税率以基点表示 |
| `merge_lines(items)` | 合并相同 SKU |

## 统计函数 (stats.py)

| 函数 | 说明 |
| --- | --- |
| `mean(values)` | 算术平均 |
| `weighted_mean(values, weights)` | 加权平均 |
| `median(values)` | 中位数 |
| `mode(values)` | 众数 |
| `variance(values)` | 方差 |
| `stdev(values)` | 标准差 |

> 注意:`old_mean` 已废弃,将在下个版本移除。

## 开发

运行测试:

```
rustc --edition 2021 --test src/lib.rs -o /tmp/inv_test && /tmp/inv_test
python3 -c "import stats"
```

欢迎提交 PR 🎉。
