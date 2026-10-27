use geas_app::*;
use std::collections::{BTreeMap, BTreeSet};

#[allow(unused_mut, non_snake_case, unused_variables)]
fn main() {
    {
        let mut Store = Store::default();
        Store.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_orderCtx { tax_rate: Ratio { num: 1, den: 10 }, new_Order_id: "N1".to_string() };
        let Input = place_orderInput { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]) };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX1\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = place_order(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX1\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_orderCtx { tax_rate: Ratio { num: 1, den: 10 }, new_Order_id: "N1".to_string() };
        let Input = place_orderInput { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 6u8)]) };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX2\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = place_order(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX2\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_orderCtx { tax_rate: Ratio { num: 1, den: 10 }, new_Order_id: "N1".to_string() };
        let Input = place_orderInput { customer_id: "C9".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]) };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX3\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = place_order(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX3\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 100i64 });
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_orderCtx { tax_rate: Ratio { num: 1, den: 10 }, new_Order_id: "N1".to_string() };
        let Input = place_orderInput { customer_id: "C1".to_string(), items: BTreeMap::from([("Z".to_string(), 1u8)]) };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX5\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = place_order(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX5\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Product.insert("A".to_string(), Product { product_id: "A".to_string(), stock: 5u64, unit_price: 105i64 });
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = place_orderCtx { tax_rate: Ratio { num: 1, den: 10 }, new_Order_id: "N1".to_string() };
        let Input = place_orderInput { customer_id: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 1u8)]) };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX6\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = place_order(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX6\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::Received });
        let ctx = request_paymentCtx {  };
        let Input = request_paymentInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX7\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = request_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX7\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = request_paymentCtx {  };
        let Input = request_paymentInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX8\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = request_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX8\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::new(), total: 0i64, status: OrderStatus::Received });
        let ctx = request_paymentCtx {  };
        let Input = request_paymentInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX9\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = request_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX9\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        let ctx = request_paymentCtx {  };
        let Input = request_paymentInput { order_id: "N9".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX10\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = request_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX10\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = apply_payment_resultCtx {  };
        let Input = apply_payment_resultInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX11\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = apply_payment_result(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX11\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::PaymentRequested });
        let ctx = expire_paymentCtx {  };
        let Input = expire_paymentInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX12\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = expire_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX12\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8)]), total: 220i64, status: OrderStatus::Completed });
        let ctx = expire_paymentCtx {  };
        let Input = expire_paymentInput { order_id: "N1".to_string() };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX13\t入力不正({})\t\t", e),
            Ok(()) => {
                let Output = expire_payment(&ctx, &Store, &Input);
                let 前 = Store.clone();
                Store.apply(Output.Write.clone());
                println!("EX13\t{}\t{}\t{}", Output.result.show(), 前.diff(&Store).join(";"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut Store = Store::default();
        Store.Customer.insert("C1".to_string(), Customer { customer_id: "C1".to_string() });
        Store.Order.insert("N1".to_string(), Order { order_id: "N1".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 2u8), ("B".to_string(), 1u8)]), total: 300i64, status: OrderStatus::Received });
        Store.Order.insert("N2".to_string(), Order { order_id: "N2".to_string(), customer: "C1".to_string(), items: BTreeMap::from([("A".to_string(), 1u8)]), total: 100i64, status: OrderStatus::Canceled });
        let ctx = order_listCtx {  };
        let Input = order_listInput {  };
        match ctx.validate().and_then(|_| Input.validate()) {
            Err(e) => println!("EX14\t入力不正({})\t\t", e),
            Ok(()) => println!("EX14\t一覧({})\t\t", order_list(&ctx, &Store, &Input).iter().map(|x| x.show()).collect::<Vec<_>>().join(",")),
        }
    }
}
