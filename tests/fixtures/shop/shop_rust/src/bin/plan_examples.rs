use geas_app::*;
use std::collections::{BTreeMap, BTreeSet};

#[allow(unused_mut, non_snake_case, unused_variables)]
fn main() {
    {
        let mut 保存 = 保存::default();
        保存.商品.insert("A".to_string(), 商品 { 商品ID: "A".to_string(), 在庫: 5u64, 単価: 100i64 });
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 受注文脈 { 税率: 有理 { 分子: 1, 分母: 10 }, 新注文ID: "N1".to_string() };
        let 入力 = 受注入力 { 顧客ID: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]) };
        match 入力.検査() {
            Err(e) => println!("EX1\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 受注(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX1\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.商品.insert("A".to_string(), 商品 { 商品ID: "A".to_string(), 在庫: 5u64, 単価: 100i64 });
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 受注文脈 { 税率: 有理 { 分子: 1, 分母: 10 }, 新注文ID: "N1".to_string() };
        let 入力 = 受注入力 { 顧客ID: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 6u8)]) };
        match 入力.検査() {
            Err(e) => println!("EX2\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 受注(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX2\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.商品.insert("A".to_string(), 商品 { 商品ID: "A".to_string(), 在庫: 5u64, 単価: 100i64 });
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 受注文脈 { 税率: 有理 { 分子: 1, 分母: 10 }, 新注文ID: "N1".to_string() };
        let 入力 = 受注入力 { 顧客ID: "C9".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]) };
        match 入力.検査() {
            Err(e) => println!("EX3\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 受注(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX3\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.商品.insert("A".to_string(), 商品 { 商品ID: "A".to_string(), 在庫: 5u64, 単価: 100i64 });
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 受注文脈 { 税率: 有理 { 分子: 1, 分母: 10 }, 新注文ID: "N1".to_string() };
        let 入力 = 受注入力 { 顧客ID: "C1".to_string(), 明細: BTreeMap::from([("Z".to_string(), 1u8)]) };
        match 入力.検査() {
            Err(e) => println!("EX5\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 受注(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX5\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.商品.insert("A".to_string(), 商品 { 商品ID: "A".to_string(), 在庫: 5u64, 単価: 105i64 });
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 受注文脈 { 税率: 有理 { 分子: 1, 分母: 10 }, 新注文ID: "N1".to_string() };
        let 入力 = 受注入力 { 顧客ID: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 1u8)]) };
        match 入力.検査() {
            Err(e) => println!("EX6\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 受注(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX6\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]), 合計: 220i64, 状態: 注文状態::受付 });
        let ctx = 決済要求文脈 {  };
        let 入力 = 決済要求入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX7\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済要求(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX7\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]), 合計: 220i64, 状態: 注文状態::決済要求済 });
        let ctx = 決済要求文脈 {  };
        let 入力 = 決済要求入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX8\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済要求(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX8\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::new(), 合計: 0i64, 状態: 注文状態::受付 });
        let ctx = 決済要求文脈 {  };
        let 入力 = 決済要求入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX9\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済要求(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX9\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.顧客.insert("C1".to_string(), 顧客 { 顧客ID: "C1".to_string() });
        let ctx = 決済要求文脈 {  };
        let 入力 = 決済要求入力 { 注文ID: "N9".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX10\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済要求(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX10\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]), 合計: 220i64, 状態: 注文状態::決済要求済 });
        let ctx = 決済結果の反映文脈 {  };
        let 入力 = 決済結果の反映入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX11\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済結果の反映(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX11\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]), 合計: 220i64, 状態: 注文状態::決済要求済 });
        let ctx = 決済の期限切れ文脈 {  };
        let 入力 = 決済の期限切れ入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX12\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済の期限切れ(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX12\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
    {
        let mut 保存 = 保存::default();
        保存.注文.insert("N1".to_string(), 注文 { 注文ID: "N1".to_string(), 顧客: "C1".to_string(), 明細: BTreeMap::from([("A".to_string(), 2u8)]), 合計: 220i64, 状態: 注文状態::完了 });
        let ctx = 決済の期限切れ文脈 {  };
        let 入力 = 決済の期限切れ入力 { 注文ID: "N1".to_string() };
        match 入力.検査() {
            Err(e) => println!("EX13\t入力不正({})\t\t", e),
            Ok(()) => {
                let 出力 = 決済の期限切れ(&ctx, &保存, &入力);
                let 前 = 保存.clone();
                保存.適用(出力.書込.clone());
                println!("EX13\t{}\t{}\t{}", 出力.結果.表示(), 前.差分(&保存).join(";"), 出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));
            }
        }
    }
}
