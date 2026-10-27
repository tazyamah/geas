use geas_app::*;
use std::collections::{BTreeMap, BTreeSet};

#[allow(unused_mut, non_snake_case, unused_variables)]
fn main() {
    {
        let mut 保存 = 保存::default();
        保存.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_order文脈 { tax_rate: 有理 { 分子: 1, 分母: 10 }, 新OrderID: "N1".to_string() };
        let 入力 = place_order入力 { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]) };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX1\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = place_order(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX1\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_order文脈 { tax_rate: 有理 { 分子: 1, 分母: 10 }, 新OrderID: "N1".to_string() };
        let 入力 = place_order入力 { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 6u8)]) };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX2\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = place_order(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX2\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_order文脈 { tax_rate: 有理 { 分子: 1, 分母: 10 }, 新OrderID: "N1".to_string() };
        let 入力 = place_order入力 { customer_id: "C9".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]) };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX3\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = place_order(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX3\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_order文脈 { tax_rate: 有理 { 分子: 1, 分母: 10 }, 新OrderID: "N1".to_string() };
        let 入力 = place_order入力 { customer_id: "C1".to_string(), items: BTreeMap::from([("Z".to_string(), 1u8)]) };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX5\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = place_order(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX5\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 105i64 });
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_order文脈 { tax_rate: 有理 { 分子: 1, 分母: 10 }, 新OrderID: "N1".to_string() };
        let 入力 = place_order入力 { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 1u8)]) };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX6\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = place_order(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX6\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::Received });
        let ctx = request_payment文脈 {  };
        let 入力 = request_payment入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX7\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = request_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX7\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = request_payment文脈 {  };
        let 入力 = request_payment入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX8\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = request_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX8\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::new(), total: 0i64, status: OrderStatus::Received });
        let ctx = request_payment文脈 {  };
        let 入力 = request_payment入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX9\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = request_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX9\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = request_payment文脈 {  };
        let 入力 = request_payment入力 { order_id: "N9".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX10\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = request_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX10\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = apply_payment_result文脈 {  };
        let 入力 = apply_payment_result入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX11\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = apply_payment_result(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX11\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = expire_payment文脈 {  };
        let 入力 = expire_payment入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX12\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = expire_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX12\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::Completed });
        let ctx = expire_payment文脈 {  };
        let 入力 = expire_payment入力 { order_id: "N1".to_string() };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX13\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = expire_payment(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX13\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        保存.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8), ("B".to_string(), 1u8)]), total: 300i64, status: OrderStatus::Received });
        保存.Order.insert("N2".to_string(), Order { order_id: "N2".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 1u8)]), total: 100i64, status: OrderStatus::Canceled });
        let ctx = order_list文脈 {  };
        let 入力 = order_list入力 {  };
        match ctx.検査().and_then(|_| 入力.検査()) {
            Err(e) => println!("EX14\t入力不正({})\t\t", e),
            Ok(()) => println!("EX14\t一覧({})\t\t", order_list(&ctx, &保存, &入力).iter().map(|x| x.表示()).collect::<Vec<_>>().join(",")),
        }
    }
}
