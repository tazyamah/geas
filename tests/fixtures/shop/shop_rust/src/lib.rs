// SPDX-License-Identifier: MIT OR Apache-2.0
// 自動生成。手で編集しない（コードから geas が毎回生成する）。
// 生成規則の束縛：panic も未定義動作も起こしうる構文を使わない（cargo clippy で強制される）。起きうる異常は Fault で返す。
#![deny(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused_variables, unused_mut)]
#![allow(uncommon_codepoints, mixed_script_confusables, unused_parens)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

/// 生成した処理が、panic も未定義動作も起こさずに返す異常。検査器が「起きない」と示した所で起きたら、検査器の抜けの報せになる。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 検査器が「必ずある」と示した値（書込先・null でない値）が無かった
    Unproven(&'static str),
    /// 計算か物理型への変換があふれた
    Overflow(&'static str),
    /// 外から足した部品の戻り値が、宣言の保証に違反した（部品名, 内容）
    Guarantee(&'static str, String),
    /// 範囲の宣言に反する値を書こうとした
    Range(String),
}
impl Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fault::Unproven(w) => write!(f, "検査で示したはずの前提が成り立ちませんでした: {}", w),
            Fault::Overflow(w) => write!(f, "計算か変換があふれました: {}", w),
            Fault::Guarantee(p, m) => write!(f, "部品「{}」が保証に違反しました: {}", p, m),
            Fault::Range(m) => write!(f, "範囲の宣言に反する値を書こうとしました: {}", m),
        }
    }
}
/// 検査器が「必ずある」と示した所の取り出し。panic も unsafe も使わず、無ければ Fault を返す。
macro_rules! proven { ($e:expr, $what:expr) => { $e.ok_or(Fault::Unproven($what))? }; }

/// 端数の出る値（分子/分母）。丸めの指定が無いと整数にできない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: i128,
    pub den: i128,
}
impl Display for Ratio {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}
pub fn scale(x: i128, r: Ratio) -> Result<Ratio, Fault> {
    Ok(Ratio { num: x.checked_mul(r.num).ok_or(Fault::Overflow("割合の掛け算"))?, den: r.den })
}
pub fn mul_ratio(a: Ratio, b: Ratio) -> Result<Ratio, Fault> {
    Ok(Ratio {
        num: a.num.checked_mul(b.num).ok_or(Fault::Overflow("割合どうしの掛け算"))?,
        den: a.den.checked_mul(b.den).ok_or(Fault::Overflow("割合どうしの掛け算"))?,
    })
}
/// 外から入る割合の分母の上限と、範囲を書かないときの値の上限
pub const RATIO_DEN_MAX: i128 = 1_000_000_000;
pub const RATIO_VAL_MAX: i128 = 1_000_000_000;
fn positive_den(x: Ratio) -> Result<(), Fault> {
    if x.den > 0 { Ok(()) } else { Err(Fault::Overflow("割合の分母が 0 以下")) }
}
pub fn floor(x: Ratio) -> Result<i128, Fault> {
    positive_den(x)?;
    Ok(x.num.div_euclid(x.den))
}
pub fn ceil(x: Ratio) -> Result<i128, Fault> {
    let q = floor(x)?;
    if x.num.rem_euclid(x.den) == 0 { Ok(q) } else { q.checked_add(1).ok_or(Fault::Overflow("切り上げ")) }
}
pub fn half_up(x: Ratio) -> Result<i128, Fault> {
    positive_den(x)?;
    let n = x.num.checked_mul(2).and_then(|n| n.checked_add(x.den)).ok_or(Fault::Overflow("四捨五入"))?;
    Ok(n.div_euclid(x.den.checked_mul(2).ok_or(Fault::Overflow("四捨五入"))?))
}
pub fn check_range(name: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if lo.map_or(false, |l| x < l) || hi.map_or(false, |h| x > h) {
        return Err(format!("{}={} は範囲外", name, x));
    }
    Ok(())
}
/// 時刻（秒）の絶対値の上限。期間の足し引きが i64 であふれないよう、外から入る値を境界で止める
pub const TIME_LIMIT: i128 = 1_000_000_000_000_000;
pub fn check_chars(name: &str, s: &str, allowed: &[(char, char)]) -> Result<(), String> {
    match s.chars().find(|c| !allowed.iter().any(|&(a, b)| a <= *c && *c <= b)) {
        Some(c) => Err(format!("{} に使えない文字 {:?} があります", name, c)),
        None => Ok(()),
    }
}
/// GTIN（JAN・EAN・UPC）の最後の1桁が合っているか。右から2桁目を3倍、次を1倍…と足して、10の倍数になるのが正しい
pub fn check_gtin(name: &str, s: &str) -> Result<(), String> {
    let ds: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    let sum: u32 = ds.iter().rev().enumerate().map(|(i, d)| if i % 2 == 1 { d.saturating_mul(3) } else { *d }).sum();
    if ds.len() != s.chars().count() || sum % 10 != 0 {
        return Err(format!("{} の最後の1桁（チェックディジット）が合っていません", name));
    }
    Ok(())
}
/// 冪等キーの組を1つの文字列に（["a","b"] の形。" と \\ は逃がすので、区切りがあいまいにならない）
pub fn join_idem_key(xs: &[String]) -> String {
    let q: Vec<String> = xs.iter().map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    format!("[{}]", q.join(","))
}
pub fn check_ratio(name: &str, x: &Ratio, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    let (lo, hi) = (lo.unwrap_or(-RATIO_VAL_MAX), hi.unwrap_or(RATIO_VAL_MAX));
    if x.den <= 0 || x.den > RATIO_DEN_MAX || x.num < lo.saturating_mul(x.den) || x.num > hi.saturating_mul(x.den) {
        return Err(format!("{}={}/{} は範囲外", name, x.num, x.den));
    }
    Ok(())
}
pub fn in_range(name: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<i128, Fault> {
    check_range(name, x, lo, hi).map_err(Fault::Range)?;
    Ok(x)
}
pub fn show_bytes(b: &[u8]) -> String { format!("0x{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>()) }
/// 検索で同一視する軸をそろえた文字列。大小は小文字へ、かなはカタカナをひらがなへ、幅は全角の英数記号を半角へ
pub fn fold_text(s: &str, fold_case: bool, fold_kana: bool, fold_width: bool) -> String {
    s.chars().flat_map(|c| {
        let mut c = c;
        if fold_width && ('\u{FF01}'..='\u{FF5E}').contains(&c) { c = char::from_u32((c as u32).saturating_sub(0xFEE0)).unwrap_or(c); }
        if fold_kana && ('\u{30A1}'..='\u{30F6}').contains(&c) { c = char::from_u32((c as u32).saturating_sub(0x60)).unwrap_or(c); }
        if fold_case { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }
    }).collect()
}
pub fn show_set(s: &BTreeSet<String>) -> String {
    format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(","))
}
pub fn show_map<V>(m: &BTreeMap<String, V>, show: impl Fn(&V) -> String) -> String {
    format!("{{{}}}", m.iter().map(|(k, v)| format!("{}:{}", k, show(v))).collect::<Vec<_>>().join(","))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dest {
    Deed(&'static str),
    Wait(&'static str),
    Reply,
}
pub struct ReplyDef {
    pub name: &'static str,
    /// 応答が運ぶ中身：(名前, 型, 必須か)。外から返ってくる値なので、基盤が受け取るときに確かめる。
    pub fields: &'static [(&'static str, &'static str, bool)],
}
pub struct TriggerDef {
    pub name: &'static str,
    /// (種別, 要因, 行き先の処理)。種別は reply／deadline／all。期限の要因は秒数、all の要因は「A,B」（全部の応答がそろったら進む）。
    /// 先に成り立った行が勝つ（実行は基盤）。
    pub causes: &'static [(&'static str, &'static str, &'static str)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderStatus { Received, PaymentRequested, Completed, Canceled }
impl OrderStatus {
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "Received" => Some(OrderStatus::Received),
            "PaymentRequested" => Some(OrderStatus::PaymentRequested),
            "Completed" => Some(OrderStatus::Completed),
            "Canceled" => Some(OrderStatus::Canceled),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
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
    pub stock: u32,
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
pub struct Store {
    pub Product: BTreeMap<String, Product>,
    pub Customer: BTreeMap<String, Customer>,
    pub Order: BTreeMap<String, Order>,
}

impl Store {
    pub fn apply(&mut self, writes: Vec<Write>) -> Result<(), Fault> {
        for w in writes {
            match w {
                Write::Order_New(r) => { self.Order.insert(r.order_id.clone(), r); }
                Write::Product_stock(id, v) => { proven!(self.Product.get_mut(&id), "検査で示した書込先が無い: Product").stock = v; }
                Write::Order_status(id, v) => { proven!(self.Order.get_mut(&id), "検査で示した書込先が無い: Order").status = v; }
            }
        }
        Ok(())
    }
    pub fn diff(&self, after: &Store) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (id, r) in &after.Product {
            match self.Product.get(id) {
                None => out.push(format!("Product({})新規(stock={}, unit_price={})", id, r.stock, r.unit_price)),
                Some(b) => {
                    if b.stock != r.stock { out.push(format!("Product({}).stock:{}→{}", id, b.stock, r.stock)); }
                    if b.unit_price != r.unit_price { out.push(format!("Product({}).unit_price:{}→{}", id, b.unit_price, r.unit_price)); }
                }
            }
        }
        for id in self.Product.keys() { if !after.Product.contains_key(id) { out.push(format!("Product({})削除", id)); } }
        for (id, r) in &after.Customer {
            match self.Customer.get(id) {
                None => out.push(format!("Customer({})新規()", id)),
                Some(b) => {
                }
            }
        }
        for id in self.Customer.keys() { if !after.Customer.contains_key(id) { out.push(format!("Customer({})削除", id)); } }
        for (id, r) in &after.Order {
            match self.Order.get(id) {
                None => out.push(format!("Order({})新規(customer={}, items={}, total={}, status={})", id, r.customer, show_map(&r.items, |v| format!("{}", v)), r.total, r.status.name())),
                Some(b) => {
                    if b.customer != r.customer { out.push(format!("Order({}).customer:{}→{}", id, b.customer, r.customer)); }
                    if b.items != r.items { out.push(format!("Order({}).items:{}→{}", id, show_map(&b.items, |v| format!("{}", v)), show_map(&r.items, |v| format!("{}", v)))); }
                    if b.total != r.total { out.push(format!("Order({}).total:{}→{}", id, b.total, r.total)); }
                    if b.status != r.status { out.push(format!("Order({}).status:{}→{}", id, b.status.name(), r.status.name())); }
                }
            }
        }
        for id in self.Order.keys() { if !after.Order.contains_key(id) { out.push(format!("Order({})削除", id)); } }
        out.sort();
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Write {
    Order_New(Order),
    Product_stock(String, u32),
    Order_status(String, OrderStatus),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Payment_charge { order_id: String, amount: i64, idem_key: String },
}
impl Effect {
    pub fn show(&self) -> String {
        match self {
            Effect::Payment_charge { order_id, amount, idem_key } => format!("Payment.charge({}, {}, 冪等キー={})", order_id, amount, idem_key),
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
    pub fn show(&self) -> String {
        match self {
            PlaceOrderResult::Received(a0) => format!("Received({})", a0),
            PlaceOrderResult::OutOfStock(a0) => format!("OutOfStock({})", show_set(&a0)),
            PlaceOrderResult::NoSuchProduct(a0) => format!("NoSuchProduct({})", show_set(&a0)),
            PlaceOrderResult::NoSuchCustomer => "NoSuchCustomer".to_string(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            PlaceOrderResult::Received(..) => "Received",
            PlaceOrderResult::OutOfStock(..) => "OutOfStock",
            PlaceOrderResult::NoSuchProduct(..) => "NoSuchProduct",
            PlaceOrderResult::NoSuchCustomer => "NoSuchCustomer",
        }
    }
    pub fn args(&self) -> Vec<String> {
        match self {
            PlaceOrderResult::Received(a0) => vec![format!("{}", a0)],
            PlaceOrderResult::OutOfStock(a0) => vec![format!("{}", show_set(&a0))],
            PlaceOrderResult::NoSuchProduct(a0) => vec![format!("{}", show_set(&a0))],
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
    pub fn show(&self) -> String {
        match self {
            RequestPaymentResult::Requested => "Requested".to_string(),
            RequestPaymentResult::NoSuchOrder => "NoSuchOrder".to_string(),
            RequestPaymentResult::WrongStatus => "WrongStatus".to_string(),
            RequestPaymentResult::ZeroTotal => "ZeroTotal".to_string(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            RequestPaymentResult::Requested => "Requested",
            RequestPaymentResult::NoSuchOrder => "NoSuchOrder",
            RequestPaymentResult::WrongStatus => "WrongStatus",
            RequestPaymentResult::ZeroTotal => "ZeroTotal",
        }
    }
    pub fn args(&self) -> Vec<String> {
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
    pub fn show(&self) -> String {
        match self {
            ReleaseResult::Applied => "Applied".to_string(),
            ReleaseResult::NoSuchOrder => "NoSuchOrder".to_string(),
            ReleaseResult::WrongStatus => "WrongStatus".to_string(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            ReleaseResult::Applied => "Applied",
            ReleaseResult::NoSuchOrder => "NoSuchOrder",
            ReleaseResult::WrongStatus => "WrongStatus",
        }
    }
    pub fn args(&self) -> Vec<String> {
        match self {
            ReleaseResult::Applied => vec![],
            ReleaseResult::NoSuchOrder => vec![],
            ReleaseResult::WrongStatus => vec![],
        }
    }
}

// ---- 処理 place_order ----
pub struct place_orderInput {
    pub customer_id: String,
    pub items: BTreeMap<String, u8>,
}
impl place_orderInput {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        check_range("itemsの要素数", self.items.len() as i128, Some(0), Some(100))?;
        for v in self.items.values() { check_range("items", *v as i128, Some(1), Some(99))?; }
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct place_orderCtx {
    pub tax_rate: Ratio,
    pub new_Order_id: String,
}
impl place_orderCtx {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        check_ratio("tax_rate", &self.tax_rate, Some(0), Some(1))?;
        Ok(())
    }
}
pub struct place_orderOutput {
    pub result: PlaceOrderResult,
    pub Write: Vec<Write>,
    pub effects: Vec<Effect>,
}
pub fn place_order(ctx: &place_orderCtx, Store: &Store, Input: &place_orderInput) -> Result<place_orderOutput, Fault> {
    let mut writes: Vec<Write> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let customer = match Store.Customer.get(&(Input.customer_id.clone())) {
        Some(r) => r,
        None => return Ok(place_orderOutput { result: PlaceOrderResult::NoSuchCustomer, Write: writes, effects: effects }),
    };
    let unregistered: BTreeSet<String> = { let mut acc: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in Input.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if !Store.Product.contains_key(&(k.clone())) { acc.insert(k.clone()); } } acc };
    if !unregistered.is_empty() {
        return Ok(place_orderOutput { result: PlaceOrderResult::NoSuchProduct(unregistered.clone()), Write: writes, effects: effects });
    }
    let short: BTreeSet<String> = { let mut acc: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in Input.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if ((Store.Product.get(&(k.clone())).ok_or(Fault::Unproven("検査で示した参照先が無い: Product"))?.stock as i128) < v) { acc.insert(k.clone()); } } acc };
    if !short.is_empty() {
        return Ok(place_orderOutput { result: PlaceOrderResult::OutOfStock(short.clone()), Write: writes, effects: effects });
    }
    for (k_, v_) in Input.items.iter() {
        let k: String = k_.clone();
        let v: i128 = *v_ as i128;
        {
            let new_value: i128 = ((Store.Product.get(&(k.clone())).ok_or(Fault::Unproven("検査で示した参照先が無い: Product"))?.stock as i128)).checked_sub(v).ok_or(Fault::Overflow("Product.stock"))?;
            writes.push(Write::Product_stock(k.clone(), ((new_value) as u32)));
        }
    }
    let subtotal: i128 = { let mut acc: i128 = 0; for (k_, v_) in Input.items.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; acc = acc.checked_add((((Store.Product.get(&(k.clone())).ok_or(Fault::Unproven("検査で示した参照先が無い: Product"))?.unit_price as i128)).checked_mul(v).ok_or(Fault::Overflow("Product[k].unit_price * v"))?)).ok_or(Fault::Overflow("合計"))?; } acc };
    let total: i128 = ((subtotal).checked_add(floor(scale(subtotal, ctx.tax_rate)?)?).ok_or(Fault::Overflow("subtotal + round(subtotal * tax_rate, floor, JPY)"))?);
    let order = Order {
        order_id: ctx.new_Order_id.clone(),
        customer: Input.customer_id.clone(),
        items: Input.items.clone(),
        total: ((total) as i64),
        status: OrderStatus::Received,
    };
    writes.push(Write::Order_New(order.clone()));
    return Ok(place_orderOutput { result: PlaceOrderResult::Received(order.order_id.clone()), Write: writes, effects: effects });
}

// ---- 処理 request_payment ----
pub struct request_paymentInput {
    pub order_id: String,
}
impl request_paymentInput {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct request_paymentCtx {
}
impl request_paymentCtx {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct request_paymentOutput {
    pub result: RequestPaymentResult,
    pub Write: Vec<Write>,
    pub effects: Vec<Effect>,
}
pub fn request_payment(ctx: &request_paymentCtx, Store: &Store, Input: &request_paymentInput) -> Result<request_paymentOutput, Fault> {
    let mut writes: Vec<Write> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let order = match Store.Order.get(&(Input.order_id.clone())) {
        Some(r) => r,
        None => return Ok(request_paymentOutput { result: RequestPaymentResult::NoSuchOrder, Write: writes, effects: effects }),
    };
    if (order.status != OrderStatus::Received) {
        return Ok(request_paymentOutput { result: RequestPaymentResult::WrongStatus, Write: writes, effects: effects });
    }
    if ((order.total as i128) <= 0i128) {
        return Ok(request_paymentOutput { result: RequestPaymentResult::ZeroTotal, Write: writes, effects: effects });
    }
    writes.push(Write::Order_status(order.order_id.clone(), OrderStatus::PaymentRequested));
    effects.push(Effect::Payment_charge { order_id: Input.order_id.clone(), amount: i64::try_from(in_range("amount", (order.total as i128), None, None)?).map_err(|_| Fault::Overflow("amount"))?, idem_key: Input.order_id.clone() });
    return Ok(request_paymentOutput { result: RequestPaymentResult::Requested, Write: writes, effects: effects });
}

// ---- 処理 apply_payment_result ----
pub struct apply_payment_resultInput {
    pub order_id: String,
}
impl apply_payment_resultInput {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct apply_payment_resultCtx {
}
impl apply_payment_resultCtx {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct apply_payment_resultOutput {
    pub result: ReleaseResult,
    pub Write: Vec<Write>,
    pub effects: Vec<Effect>,
}
pub fn apply_payment_result(ctx: &apply_payment_resultCtx, Store: &Store, Input: &apply_payment_resultInput) -> Result<apply_payment_resultOutput, Fault> {
    let mut writes: Vec<Write> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let order = match Store.Order.get(&(Input.order_id.clone())) {
        Some(r) => r,
        None => return Ok(apply_payment_resultOutput { result: ReleaseResult::NoSuchOrder, Write: writes, effects: effects }),
    };
    if (order.status != OrderStatus::PaymentRequested) {
        return Ok(apply_payment_resultOutput { result: ReleaseResult::WrongStatus, Write: writes, effects: effects });
    }
    writes.push(Write::Order_status(order.order_id.clone(), OrderStatus::Completed));
    return Ok(apply_payment_resultOutput { result: ReleaseResult::Applied, Write: writes, effects: effects });
}

// ---- 処理 expire_payment ----
pub struct expire_paymentInput {
    pub order_id: String,
}
impl expire_paymentInput {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct expire_paymentCtx {
}
impl expire_paymentCtx {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct expire_paymentOutput {
    pub result: ReleaseResult,
    pub Write: Vec<Write>,
    pub effects: Vec<Effect>,
}
pub fn expire_payment(ctx: &expire_paymentCtx, Store: &Store, Input: &expire_paymentInput) -> Result<expire_paymentOutput, Fault> {
    let mut writes: Vec<Write> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let order = match Store.Order.get(&(Input.order_id.clone())) {
        Some(r) => r,
        None => return Ok(expire_paymentOutput { result: ReleaseResult::NoSuchOrder, Write: writes, effects: effects }),
    };
    if (order.status != OrderStatus::PaymentRequested) {
        return Ok(expire_paymentOutput { result: ReleaseResult::WrongStatus, Write: writes, effects: effects });
    }
    writes.push(Write::Order_status(order.order_id.clone(), OrderStatus::Canceled));
    return Ok(expire_paymentOutput { result: ReleaseResult::Applied, Write: writes, effects: effects });
}

// ---- 問い合わせ order_list（読むだけ） ----
pub struct order_listInput {
}
impl order_listInput {
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct order_listCtx {
}
impl order_listCtx {
    /// 外から埋まる値（slot）の、範囲の宣言の検査
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct order_listRow {
    pub order_id: String,
    pub customer: String,
    pub item_count: i128,
}
impl order_listRow {
    pub fn show(&self) -> String { format!("order_id={}/customer={}/item_count={}", self.order_id, self.customer, self.item_count) }
}
pub fn order_list(ctx: &order_listCtx, Store: &Store, Input: &order_listInput) -> Vec<order_listRow> {
    let mut rows: Vec<&Order> = Store.Order.values().filter(|r| {
        if !(r.status != OrderStatus::Canceled) { return false; }
        true
    }).collect();
    rows.sort_by(|a, b| a.order_id.cmp(&b.order_id));
    rows.into_iter().filter_map(|r| {
        let j_customer = Store.Customer.get(&r.customer)?;  // 参照先が無い行は出さない
        Some(order_listRow { order_id: r.order_id.clone(), customer: j_customer.customer_id.clone(), item_count: r.items.values().map(|v| *v as i128).sum::<i128>() })
    }).collect()
}

// 【フロー】：結果の種類ごとの行き先。match の網羅は Rust が検査する。
pub fn OrderIntake_place_order(r: &PlaceOrderResult) -> Dest {
    match r {
        PlaceOrderResult::Received(..) => Dest::Deed("request_payment"),
        PlaceOrderResult::OutOfStock(..) => Dest::Reply,
        PlaceOrderResult::NoSuchProduct(..) => Dest::Reply,
        PlaceOrderResult::NoSuchCustomer => Dest::Reply,
    }
}

pub fn OrderIntake_request_payment(r: &RequestPaymentResult) -> Dest {
    match r {
        RequestPaymentResult::Requested => Dest::Wait("PaymentPending"),
        RequestPaymentResult::NoSuchOrder => Dest::Reply,
        RequestPaymentResult::WrongStatus => Dest::Reply,
        RequestPaymentResult::ZeroTotal => Dest::Reply,
    }
}

pub fn PaymentRelease_apply_payment_result(r: &ReleaseResult) -> Dest {
    match r {
        ReleaseResult::Applied => Dest::Reply,
        ReleaseResult::NoSuchOrder => Dest::Reply,
        ReleaseResult::WrongStatus => Dest::Reply,
    }
}

pub fn PaymentRelease_expire_payment(r: &ReleaseResult) -> Dest {
    match r {
        ReleaseResult::Applied => Dest::Reply,
        ReleaseResult::NoSuchOrder => Dest::Reply,
        ReleaseResult::WrongStatus => Dest::Reply,
    }
}

// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。
pub const triggers: &[TriggerDef] = &[
    TriggerDef { name: "PaymentPending", causes: &[("reply", "PaymentSucceeded", "apply_payment_result"), ("deadline", "1800", "expire_payment")] },
];

