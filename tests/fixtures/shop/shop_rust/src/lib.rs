// SPDX-License-Identifier: MIT OR Apache-2.0
// 自動生成。手で編集しない（コードから geas が毎回生成する）。
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused_variables, unused_mut)]
#![allow(uncommon_codepoints, mixed_script_confusables, unused_parens)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

/// 端数の出る値（分子/分母）。丸めの指定が無いと整数にできない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct 有理 {
    pub 分子: i128,
    pub 分母: i128,
}
impl Display for 有理 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}/{}", self.分子, self.分母)
    }
}
pub fn 積(x: i128, r: 有理) -> 有理 {
    有理 { 分子: x * r.分子, 分母: r.分母 }
}
pub fn floor(x: 有理) -> i128 {
    x.分子.div_euclid(x.分母)
}
pub fn ceil(x: 有理) -> i128 {
    -((-x.分子).div_euclid(x.分母))
}
pub fn half_up(x: 有理) -> i128 {
    (2 * x.分子 + x.分母).div_euclid(2 * x.分母)
}
pub fn 範囲検査(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if lo.map_or(false, |l| x < l) || hi.map_or(false, |h| x > h) {
        return Err(format!("{}={} は範囲外", 名, x));
    }
    Ok(())
}
/// 時刻（秒）の絶対値の上限。期間の足し引きが i64 であふれないよう、外から入る値を境界で止める
pub const 時刻の限り: i128 = 1_000_000_000_000_000;
pub fn 文字種検査(名: &str, s: &str, 許す: &[(char, char)]) -> Result<(), String> {
    match s.chars().find(|c| !許す.iter().any(|&(a, b)| a <= *c && *c <= b)) {
        Some(c) => Err(format!("{} に使えない文字 {:?} があります", 名, c)),
        None => Ok(()),
    }
}
/// 冪等キーの組を1つの文字列に（["a","b"] の形。" と \\ は逃がすので、区切りがあいまいにならない）
pub fn 冪等キーの組(xs: &[String]) -> String {
    let q: Vec<String> = xs.iter().map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    format!("[{}]", q.join(","))
}
pub fn 割合検査(名: &str, x: &有理, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if x.分母 <= 0 || lo.map_or(false, |l| x.分子 < l * x.分母) || hi.map_or(false, |h| x.分子 > h * x.分母) {
        return Err(format!("{}={}/{} は範囲外", 名, x.分子, x.分母));
    }
    Ok(())
}
pub fn 範囲内(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> i128 {
    範囲検査(名, x, lo, hi).expect("範囲の宣言に反する値を書こうとした");
    x
}
pub fn バイト列表示(b: &[u8]) -> String { format!("0x{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>()) }
/// 検索で同一視する軸をそろえた文字列。大小は小文字へ、かなはカタカナをひらがなへ、幅は全角の英数記号を半角へ
pub fn 照合(s: &str, 大小: bool, かな: bool, 幅: bool) -> String {
    s.chars().flat_map(|c| {
        let mut c = c;
        if 幅 && ('\u{FF01}'..='\u{FF5E}').contains(&c) { c = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c); }
        if かな && ('\u{30A1}'..='\u{30F6}').contains(&c) { c = char::from_u32(c as u32 - 0x60).unwrap_or(c); }
        if 大小 { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }
    }).collect()
}
pub fn 集合表示(s: &BTreeSet<String>) -> String {
    format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(","))
}
pub fn 写像表示<V>(m: &BTreeMap<String, V>, 表示: impl Fn(&V) -> String) -> String {
    format!("{{{}}}", m.iter().map(|(k, v)| format!("{}:{}", k, 表示(v))).collect::<Vec<_>>().join(","))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum 行き先 {
    処理(&'static str),
    待つ(&'static str),
    応答,
}
pub struct トリガ定義 {
    pub 名前: &'static str,
    /// (種別, 要因, 行き先の処理)。期限の要因は秒数。先に起きた要因が勝つ（実行は基盤）。
    pub 要因: &'static [(&'static str, &'static str, &'static str)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderStatus { Received, PaymentRequested, Completed, Canceled }
impl OrderStatus {
    pub fn 名前から(s: &str) -> Option<Self> {
        match s {
            "Received" => Some(OrderStatus::Received),
            "PaymentRequested" => Some(OrderStatus::PaymentRequested),
            "Completed" => Some(OrderStatus::Completed),
            "Canceled" => Some(OrderStatus::Canceled),
            _ => None,
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            OrderStatus::Received => "Received",
            OrderStatus::PaymentRequested => "PaymentRequested",
            OrderStatus::Completed => "Completed",
            OrderStatus::Canceled => "Canceled",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    pub product_id: String,
    pub stock: u64,
    pub unit_price: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Customer {
    pub customer_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub order_id: String,
    pub customer: String,
    pub items: BTreeMap<String, u8>,
    pub total: i64,
    pub status: OrderStatus,
}

#[derive(Clone, Debug, Default)]
pub struct 保存 {
    pub Product: BTreeMap<String, Product>,
    pub Customer: BTreeMap<String, Customer>,
    pub Order: BTreeMap<String, Order>,
}

impl 保存 {
    pub fn 適用(&mut self, 書込列: Vec<書込>) {
        for w in 書込列 {
            match w {
                書込::Order_新規(r) => { self.Order.insert(r.order_id.clone(), r); }
                書込::Product_stock(id, v) => { unsafe { self.Product.get_mut(&id).unwrap_unchecked() }.stock = v; }
                書込::Order_status(id, v) => { unsafe { self.Order.get_mut(&id).unwrap_unchecked() }.status = v; }
            }
        }
    }
    pub fn 差分(&self, 後: &保存) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (id, r) in &後.Product {
            match self.Product.get(id) {
                None => out.push(format!("Product({})新規(stock={}, unit_price={})", id, r.stock, r.unit_price)),
                Some(b) => {
                    if b.stock != r.stock { out.push(format!("Product({}).stock:{}→{}", id, b.stock, r.stock)); }
                    if b.unit_price != r.unit_price { out.push(format!("Product({}).unit_price:{}→{}", id, b.unit_price, r.unit_price)); }
                }
            }
        }
        for id in self.Product.keys() { if !後.Product.contains_key(id) { out.push(format!("Product({})削除", id)); } }
        for (id, r) in &後.Customer {
            match self.Customer.get(id) {
                None => out.push(format!("Customer({})新規()", id)),
                Some(b) => {
                }
            }
        }
        for id in self.Customer.keys() { if !後.Customer.contains_key(id) { out.push(format!("Customer({})削除", id)); } }
        for (id, r) in &後.Order {
            match self.Order.get(id) {
                None => out.push(format!("Order({})新規(customer={}, items={}, total={}, status={})", id, r.customer, 写像表示(&r.items, |v| format!("{}", v)), r.total, r.status.名前())),
                Some(b) => {
                    if b.customer != r.customer { out.push(format!("Order({}).customer:{}→{}", id, b.customer, r.customer)); }
                    if b.items != r.items { out.push(format!("Order({}).items:{}→{}", id, 写像表示(&b.items, |v| format!("{}", v)), 写像表示(&r.items, |v| format!("{}", v)))); }
                    if b.total != r.total { out.push(format!("Order({}).total:{}→{}", id, b.total, r.total)); }
                    if b.status != r.status { out.push(format!("Order({}).status:{}→{}", id, b.status.名前(), r.status.名前())); }
                }
            }
        }
        for id in self.Order.keys() { if !後.Order.contains_key(id) { out.push(format!("Order({})削除", id)); } }
        out.sort();
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum 書込 {
    Order_新規(Order),
    Product_stock(String, u64),
    Order_status(String, OrderStatus),
}

#[derive(Clone, Debug, PartialEq)]
pub enum 外部作用 {
    Payment_charge { order_id: String, amount: i64, 冪等キー: String },
}
impl 外部作用 {
    pub fn 表示(&self) -> String {
        match self {
            外部作用::Payment_charge { order_id, amount, 冪等キー } => format!("Payment.charge({}, {}, 冪等キー={})", order_id, amount, 冪等キー),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaceOrderResult {
    Received(String),
    OutOfStock(BTreeSet<String>),
    NoSuchProduct(BTreeSet<String>),
    NoSuchCustomer,
}
impl PlaceOrderResult {
    pub fn 表示(&self) -> String {
        match self {
            PlaceOrderResult::Received(a0) => format!("Received({})", a0),
            PlaceOrderResult::OutOfStock(a0) => format!("OutOfStock({})", 集合表示(&a0)),
            PlaceOrderResult::NoSuchProduct(a0) => format!("NoSuchProduct({})", 集合表示(&a0)),
            PlaceOrderResult::NoSuchCustomer => "NoSuchCustomer".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            PlaceOrderResult::Received(..) => "Received",
            PlaceOrderResult::OutOfStock(..) => "OutOfStock",
            PlaceOrderResult::NoSuchProduct(..) => "NoSuchProduct",
            PlaceOrderResult::NoSuchCustomer => "NoSuchCustomer",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            PlaceOrderResult::Received(a0) => vec![format!("{}", a0)],
            PlaceOrderResult::OutOfStock(a0) => vec![format!("{}", 集合表示(&a0))],
            PlaceOrderResult::NoSuchProduct(a0) => vec![format!("{}", 集合表示(&a0))],
            PlaceOrderResult::NoSuchCustomer => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequestPaymentResult {
    Requested,
    NoSuchOrder,
    WrongStatus,
    ZeroTotal,
}
impl RequestPaymentResult {
    pub fn 表示(&self) -> String {
        match self {
            RequestPaymentResult::Requested => "Requested".to_string(),
            RequestPaymentResult::NoSuchOrder => "NoSuchOrder".to_string(),
            RequestPaymentResult::WrongStatus => "WrongStatus".to_string(),
            RequestPaymentResult::ZeroTotal => "ZeroTotal".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            RequestPaymentResult::Requested => "Requested",
            RequestPaymentResult::NoSuchOrder => "NoSuchOrder",
            RequestPaymentResult::WrongStatus => "WrongStatus",
            RequestPaymentResult::ZeroTotal => "ZeroTotal",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            RequestPaymentResult::Requested => vec![],
            RequestPaymentResult::NoSuchOrder => vec![],
            RequestPaymentResult::WrongStatus => vec![],
            RequestPaymentResult::ZeroTotal => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReleaseResult {
    Applied,
    NoSuchOrder,
    WrongStatus,
}
impl ReleaseResult {
    pub fn 表示(&self) -> String {
        match self {
            ReleaseResult::Applied => "Applied".to_string(),
            ReleaseResult::NoSuchOrder => "NoSuchOrder".to_string(),
            ReleaseResult::WrongStatus => "WrongStatus".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            ReleaseResult::Applied => "Applied",
            ReleaseResult::NoSuchOrder => "NoSuchOrder",
            ReleaseResult::WrongStatus => "WrongStatus",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            ReleaseResult::Applied => vec![],
            ReleaseResult::NoSuchOrder => vec![],
            ReleaseResult::WrongStatus => vec![],
        }
    }
}

// ---- 処理 place_order ----
pub struct place_order入力 {
    pub customer_id: String,
    pub items: BTreeMap<String, u8>,
}
impl place_order入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        for v in self.items.values() { 範囲検査("items", *v as i128, Some(1), Some(99))?; }
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct place_order文脈 {
    pub tax_rate: 有理,
    pub 新OrderID: String,
}
impl place_order文脈 {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        割合検査("tax_rate", &self.tax_rate, Some(0), Some(1))?;
        Ok(())
    }
}
pub struct place_order出力 {
    pub 結果: PlaceOrderResult,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn place_order(ctx: &place_order文脈, 保存: &保存, 入力: &place_order入力) -> place_order出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let customer = match 保存.Customer.get(&(入力.customer_id.clone())) {
        Some(r) => r,
        None => return place_order出力 { 結果: PlaceOrderResult::NoSuchCustomer, 書込: 書込列, 外部: 外部列 },
    };
    let unregistered: BTreeSet<String> = { let mut 集め: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in 入力.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if !保存.Product.contains_key(&(k.clone())) { 集め.insert(k.clone()); } } 集め };
    if !unregistered.is_empty() {
        return place_order出力 { 結果: PlaceOrderResult::NoSuchProduct(unregistered.clone()), 書込: 書込列, 外部: 外部列 };
    }
    let short: BTreeSet<String> = { let mut 集め: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in 入力.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if ((保存.Product[&(k.clone())].stock as i128) < v) { 集め.insert(k.clone()); } } 集め };
    if !short.is_empty() {
        return place_order出力 { 結果: PlaceOrderResult::OutOfStock(short.clone()), 書込: 書込列, 外部: 外部列 };
    }
    for (k_, v_) in 入力.items.iter() {
        let k: String = k_.clone();
        let v: i128 = *v_ as i128;
        {
            let 新値: i128 = (保存.Product[&(k.clone())].stock as i128) - v;
            書込列.push(書込::Product_stock(k.clone(), u64::try_from(新値).expect("物理数に収まらない")));
        }
    }
    let subtotal: i128 = { let mut 集め: i128 = 0; for (k_, v_) in 入力.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; 集め += ((保存.Product[&(k.clone())].unit_price as i128) * v); } 集め };
    let total: i128 = (subtotal + floor(積(subtotal, ctx.tax_rate)));
    let order = Order {
        order_id: ctx.新OrderID.clone(),
        customer: 入力.customer_id.clone(),
        items: 入力.items.clone(),
        total: i64::try_from(total).expect("物理数に収まらない"),
        status: OrderStatus::Received,
    };
    書込列.push(書込::Order_新規(order.clone()));
    return place_order出力 { 結果: PlaceOrderResult::Received(order.order_id.clone()), 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 request_payment ----
pub struct request_payment入力 {
    pub order_id: String,
}
impl request_payment入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct request_payment文脈 {
}
impl request_payment文脈 {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct request_payment出力 {
    pub 結果: RequestPaymentResult,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn request_payment(ctx: &request_payment文脈, 保存: &保存, 入力: &request_payment入力) -> request_payment出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let order = match 保存.Order.get(&(入力.order_id.clone())) {
        Some(r) => r,
        None => return request_payment出力 { 結果: RequestPaymentResult::NoSuchOrder, 書込: 書込列, 外部: 外部列 },
    };
    if (order.status != OrderStatus::Received) {
        return request_payment出力 { 結果: RequestPaymentResult::WrongStatus, 書込: 書込列, 外部: 外部列 };
    }
    if ((order.total as i128) <= 0i128) {
        return request_payment出力 { 結果: RequestPaymentResult::ZeroTotal, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::Order_status(order.order_id.clone(), OrderStatus::PaymentRequested));
    外部列.push(外部作用::Payment_charge { order_id: 入力.order_id.clone(), amount: i64::try_from(範囲内("amount", (order.total as i128), None, None)).expect("物理数に収まらない"), 冪等キー: 入力.order_id.clone() });
    return request_payment出力 { 結果: RequestPaymentResult::Requested, 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 apply_payment_result ----
pub struct apply_payment_result入力 {
    pub order_id: String,
}
impl apply_payment_result入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct apply_payment_result文脈 {
}
impl apply_payment_result文脈 {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct apply_payment_result出力 {
    pub 結果: ReleaseResult,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn apply_payment_result(ctx: &apply_payment_result文脈, 保存: &保存, 入力: &apply_payment_result入力) -> apply_payment_result出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let order = match 保存.Order.get(&(入力.order_id.clone())) {
        Some(r) => r,
        None => return apply_payment_result出力 { 結果: ReleaseResult::NoSuchOrder, 書込: 書込列, 外部: 外部列 },
    };
    if (order.status != OrderStatus::PaymentRequested) {
        return apply_payment_result出力 { 結果: ReleaseResult::WrongStatus, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::Order_status(order.order_id.clone(), OrderStatus::Completed));
    return apply_payment_result出力 { 結果: ReleaseResult::Applied, 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 expire_payment ----
pub struct expire_payment入力 {
    pub order_id: String,
}
impl expire_payment入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct expire_payment文脈 {
}
impl expire_payment文脈 {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct expire_payment出力 {
    pub 結果: ReleaseResult,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn expire_payment(ctx: &expire_payment文脈, 保存: &保存, 入力: &expire_payment入力) -> expire_payment出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let order = match 保存.Order.get(&(入力.order_id.clone())) {
        Some(r) => r,
        None => return expire_payment出力 { 結果: ReleaseResult::NoSuchOrder, 書込: 書込列, 外部: 外部列 },
    };
    if (order.status != OrderStatus::PaymentRequested) {
        return expire_payment出力 { 結果: ReleaseResult::WrongStatus, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::Order_status(order.order_id.clone(), OrderStatus::Canceled));
    return expire_payment出力 { 結果: ReleaseResult::Applied, 書込: 書込列, 外部: 外部列 };
}

// ---- 問い合わせ order_list（読むだけ） ----
pub struct order_list入力 {
}
impl order_list入力 {
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct order_list文脈 {
}
impl order_list文脈 {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct order_list行 {
    pub order_id: String,
    pub customer: String,
    pub item_count: i128,
}
impl order_list行 {
    pub fn 表示(&self) -> String { format!("order_id={}/customer={}/item_count={}", self.order_id, self.customer, self.item_count) }
}
pub fn order_list(ctx: &order_list文脈, 保存: &保存, 入力: &order_list入力) -> Vec<order_list行> {
    let mut 行: Vec<&Order> = 保存.Order.values().filter(|r| {
        if !(r.status != OrderStatus::Canceled) { return false; }
        true
    }).collect();
    行.sort_by(|a, b| a.order_id.cmp(&b.order_id));
    行.into_iter().filter_map(|r| {
        let j_customer = 保存.Customer.get(&r.customer)?;  // 参照先が無い行は出さない
        Some(order_list行 { order_id: r.order_id.clone(), customer: j_customer.customer_id.clone(), item_count: r.items.values().map(|v| *v as i128).sum::<i128>() })
    }).collect()
}

// 【フロー】：結果の種類ごとの行き先。match の網羅は Rust が検査する。
pub fn OrderIntake_place_order(r: &PlaceOrderResult) -> 行き先 {
    match r {
        PlaceOrderResult::Received(..) => 行き先::処理("request_payment"),
        PlaceOrderResult::OutOfStock(..) => 行き先::応答,
        PlaceOrderResult::NoSuchProduct(..) => 行き先::応答,
        PlaceOrderResult::NoSuchCustomer => 行き先::応答,
    }
}

pub fn OrderIntake_request_payment(r: &RequestPaymentResult) -> 行き先 {
    match r {
        RequestPaymentResult::Requested => 行き先::待つ("PaymentPending"),
        RequestPaymentResult::NoSuchOrder => 行き先::応答,
        RequestPaymentResult::WrongStatus => 行き先::応答,
        RequestPaymentResult::ZeroTotal => 行き先::応答,
    }
}

pub fn PaymentRelease_apply_payment_result(r: &ReleaseResult) -> 行き先 {
    match r {
        ReleaseResult::Applied => 行き先::応答,
        ReleaseResult::NoSuchOrder => 行き先::応答,
        ReleaseResult::WrongStatus => 行き先::応答,
    }
}

pub fn PaymentRelease_expire_payment(r: &ReleaseResult) -> 行き先 {
    match r {
        ReleaseResult::Applied => 行き先::応答,
        ReleaseResult::NoSuchOrder => 行き先::応答,
        ReleaseResult::WrongStatus => 行き先::応答,
    }
}

// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。
pub const トリガ一覧: &[トリガ定義] = &[
    トリガ定義 { 名前: "PaymentPending", 要因: &[("応答", "PaymentSucceeded", "apply_payment_result"), ("期限", "1800", "expire_payment")] },
];

