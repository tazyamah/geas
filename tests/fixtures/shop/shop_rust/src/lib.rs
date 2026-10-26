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
pub fn 切り捨て(x: 有理) -> i128 {
    x.分子.div_euclid(x.分母)
}
pub fn 切り上げ(x: 有理) -> i128 {
    -((-x.分子).div_euclid(x.分母))
}
pub fn 四捨五入(x: 有理) -> i128 {
    (2 * x.分子 + x.分母).div_euclid(2 * x.分母)
}
pub fn 範囲検査(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if lo.map_or(false, |l| x < l) || hi.map_or(false, |h| x > h) {
        return Err(format!("{}={} は範囲外", 名, x));
    }
    Ok(())
}
pub fn 範囲内(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> i128 {
    範囲検査(名, x, lo, hi).expect("範囲の宣言に反する値を書こうとした");
    x
}
pub fn バイト列表示(b: &[u8]) -> String { format!("0x{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>()) }
pub fn 集合表示(s: &BTreeSet<String>) -> String {
    format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(","))
}
pub fn 写像表示<V: Display>(m: &BTreeMap<String, V>) -> String {
    format!("{{{}}}", m.iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>().join(","))
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
pub enum 注文状態 { 受付, 決済要求済, 完了, キャンセル }
impl 注文状態 {
    pub fn 名前から(s: &str) -> Option<Self> {
        match s {
            "受付" => Some(注文状態::受付),
            "決済要求済" => Some(注文状態::決済要求済),
            "完了" => Some(注文状態::完了),
            "キャンセル" => Some(注文状態::キャンセル),
            _ => None,
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            注文状態::受付 => "受付",
            注文状態::決済要求済 => "決済要求済",
            注文状態::完了 => "完了",
            注文状態::キャンセル => "キャンセル",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct 商品 {
    pub 商品ID: String,
    pub 在庫: u64,
    pub 単価: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct 顧客 {
    pub 顧客ID: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct 注文 {
    pub 注文ID: String,
    pub 顧客: String,
    pub 明細: BTreeMap<String, u8>,
    pub 合計: i64,
    pub 状態: 注文状態,
}

#[derive(Clone, Debug, Default)]
pub struct 保存 {
    pub 商品: BTreeMap<String, 商品>,
    pub 顧客: BTreeMap<String, 顧客>,
    pub 注文: BTreeMap<String, 注文>,
}

impl 保存 {
    pub fn 適用(&mut self, 書込列: Vec<書込>) {
        for w in 書込列 {
            match w {
                書込::商品_在庫(id, v) => { self.商品.get_mut(&id).expect("書込先が無い").在庫 = v; }
                書込::注文_新規(r) => { self.注文.insert(r.注文ID.clone(), r); }
                書込::注文_状態(id, v) => { self.注文.get_mut(&id).expect("書込先が無い").状態 = v; }
            }
        }
    }
    pub fn 差分(&self, 後: &保存) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (id, r) in &後.商品 {
            match self.商品.get(id) {
                None => out.push(format!("商品({})新規(在庫={}, 単価={})", id, r.在庫, r.単価)),
                Some(b) => {
                    if b.在庫 != r.在庫 { out.push(format!("商品({}).在庫:{}→{}", id, b.在庫, r.在庫)); }
                    if b.単価 != r.単価 { out.push(format!("商品({}).単価:{}→{}", id, b.単価, r.単価)); }
                }
            }
        }
        for id in self.商品.keys() { if !後.商品.contains_key(id) { out.push(format!("商品({})削除", id)); } }
        for (id, r) in &後.顧客 {
            match self.顧客.get(id) {
                None => out.push(format!("顧客({})新規()", id)),
                Some(b) => {
                }
            }
        }
        for id in self.顧客.keys() { if !後.顧客.contains_key(id) { out.push(format!("顧客({})削除", id)); } }
        for (id, r) in &後.注文 {
            match self.注文.get(id) {
                None => out.push(format!("注文({})新規(顧客={}, 明細={}, 合計={}, 状態={})", id, r.顧客, 写像表示(&r.明細), r.合計, r.状態.名前())),
                Some(b) => {
                    if b.顧客 != r.顧客 { out.push(format!("注文({}).顧客:{}→{}", id, b.顧客, r.顧客)); }
                    if b.明細 != r.明細 { out.push(format!("注文({}).明細:{}→{}", id, 写像表示(&b.明細), 写像表示(&r.明細))); }
                    if b.合計 != r.合計 { out.push(format!("注文({}).合計:{}→{}", id, b.合計, r.合計)); }
                    if b.状態 != r.状態 { out.push(format!("注文({}).状態:{}→{}", id, b.状態.名前(), r.状態.名前())); }
                }
            }
        }
        for id in self.注文.keys() { if !後.注文.contains_key(id) { out.push(format!("注文({})削除", id)); } }
        out.sort();
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum 書込 {
    商品_在庫(String, u64),
    注文_新規(注文),
    注文_状態(String, 注文状態),
}

#[derive(Clone, Debug, PartialEq)]
pub enum 外部作用 {
    決済_請求 { 注文ID: String, 金額: i64, 冪等キー: String },
}
impl 外部作用 {
    pub fn 表示(&self) -> String {
        match self {
            外部作用::決済_請求 { 注文ID, 金額, 冪等キー } => format!("決済.請求({}, {}, 冪等キー={})", 注文ID, 金額, 冪等キー),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum 受注結果 {
    受付(String),
    在庫不足(BTreeSet<String>),
    商品なし(BTreeSet<String>),
    顧客なし,
}
impl 受注結果 {
    pub fn 表示(&self) -> String {
        match self {
            受注結果::受付(a0) => format!("受付({})", a0),
            受注結果::在庫不足(a0) => format!("在庫不足({})", 集合表示(&a0)),
            受注結果::商品なし(a0) => format!("商品なし({})", 集合表示(&a0)),
            受注結果::顧客なし => "顧客なし".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            受注結果::受付(..) => "受付",
            受注結果::在庫不足(..) => "在庫不足",
            受注結果::商品なし(..) => "商品なし",
            受注結果::顧客なし => "顧客なし",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            受注結果::受付(a0) => vec![format!("{}", a0)],
            受注結果::在庫不足(a0) => vec![format!("{}", 集合表示(&a0))],
            受注結果::商品なし(a0) => vec![format!("{}", 集合表示(&a0))],
            受注結果::顧客なし => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum 決済要求結果 {
    要求済,
    注文なし,
    状態違い,
    合計ゼロ,
}
impl 決済要求結果 {
    pub fn 表示(&self) -> String {
        match self {
            決済要求結果::要求済 => "要求済".to_string(),
            決済要求結果::注文なし => "注文なし".to_string(),
            決済要求結果::状態違い => "状態違い".to_string(),
            決済要求結果::合計ゼロ => "合計ゼロ".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            決済要求結果::要求済 => "要求済",
            決済要求結果::注文なし => "注文なし",
            決済要求結果::状態違い => "状態違い",
            決済要求結果::合計ゼロ => "合計ゼロ",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            決済要求結果::要求済 => vec![],
            決済要求結果::注文なし => vec![],
            決済要求結果::状態違い => vec![],
            決済要求結果::合計ゼロ => vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum 待ち解除結果 {
    反映済,
    注文なし,
    状態違い,
}
impl 待ち解除結果 {
    pub fn 表示(&self) -> String {
        match self {
            待ち解除結果::反映済 => "反映済".to_string(),
            待ち解除結果::注文なし => "注文なし".to_string(),
            待ち解除結果::状態違い => "状態違い".to_string(),
        }
    }
    pub fn 名前(&self) -> &'static str {
        match self {
            待ち解除結果::反映済 => "反映済",
            待ち解除結果::注文なし => "注文なし",
            待ち解除結果::状態違い => "状態違い",
        }
    }
    pub fn 引数(&self) -> Vec<String> {
        match self {
            待ち解除結果::反映済 => vec![],
            待ち解除結果::注文なし => vec![],
            待ち解除結果::状態違い => vec![],
        }
    }
}

// ---- 処理 受注 ----
pub struct 受注入力 {
    pub 顧客ID: String,
    pub 明細: BTreeMap<String, u8>,
}
impl 受注入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        for v in self.明細.values() { 範囲検査("明細", *v as i128, Some(1), Some(99))?; }
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct 受注文脈 {
    pub 税率: 有理,
    pub 新注文ID: String,
}
pub struct 受注出力 {
    pub 結果: 受注結果,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn 受注(ctx: &受注文脈, 保存: &保存, 入力: &受注入力) -> 受注出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let 顧客 = match 保存.顧客.get(&(入力.顧客ID.clone())) {
        Some(r) => r,
        None => return 受注出力 { 結果: 受注結果::顧客なし, 書込: 書込列, 外部: 外部列 },
    };
    let 未登録: BTreeSet<String> = { let mut 集め: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in 入力.明細.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if !保存.商品.contains_key(&(k.clone())) { 集め.insert(k.clone()); } } 集め };
    if !未登録.is_empty() {
        return 受注出力 { 結果: 受注結果::商品なし(未登録.clone()), 書込: 書込列, 外部: 外部列 };
    }
    let 不足: BTreeSet<String> = { let mut 集め: BTreeSet<String> = BTreeSet::new(); for (k_, v_) in 入力.明細.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; if ((保存.商品[&(k.clone())].在庫 as i128) < v) { 集め.insert(k.clone()); } } 集め };
    if !不足.is_empty() {
        return 受注出力 { 結果: 受注結果::在庫不足(不足.clone()), 書込: 書込列, 外部: 外部列 };
    }
    for (k_, v_) in 入力.明細.iter() {
        let k: String = k_.clone();
        let v: i128 = *v_ as i128;
        {
            let 新値: i128 = (保存.商品[&(k.clone())].在庫 as i128) - v;
            書込列.push(書込::商品_在庫(k.clone(), u64::try_from(範囲内("商品.在庫", 新値, Some(0), None)).expect("物理数に収まらない")));
        }
    }
    let 小計: i128 = { let mut 集め: i128 = 0; for (k_, v_) in 入力.明細.iter() { let k: String = k_.clone(); let v: i128 = *v_ as i128; 集め += ((保存.商品[&(k.clone())].単価 as i128) * v); } 集め };
    let 合計: i128 = (小計 + 切り捨て(積(小計, ctx.税率)));
    let 注文 = 注文 {
        注文ID: ctx.新注文ID.clone(),
        顧客: 入力.顧客ID.clone(),
        明細: 入力.明細.clone(),
        合計: i64::try_from(範囲内("注文.合計", 合計, Some(0), None)).expect("物理数に収まらない"),
        状態: 注文状態::受付,
    };
    書込列.push(書込::注文_新規(注文.clone()));
    return 受注出力 { 結果: 受注結果::受付(注文.注文ID.clone()), 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 決済要求 ----
pub struct 決済要求入力 {
    pub 注文ID: String,
}
impl 決済要求入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct 決済要求文脈 {
}
pub struct 決済要求出力 {
    pub 結果: 決済要求結果,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn 決済要求(ctx: &決済要求文脈, 保存: &保存, 入力: &決済要求入力) -> 決済要求出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let 注文 = match 保存.注文.get(&(入力.注文ID.clone())) {
        Some(r) => r,
        None => return 決済要求出力 { 結果: 決済要求結果::注文なし, 書込: 書込列, 外部: 外部列 },
    };
    if (注文.状態 != 注文状態::受付) {
        return 決済要求出力 { 結果: 決済要求結果::状態違い, 書込: 書込列, 外部: 外部列 };
    }
    if ((注文.合計 as i128) <= 0i128) {
        return 決済要求出力 { 結果: 決済要求結果::合計ゼロ, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::注文_状態(注文.注文ID.clone(), 注文状態::決済要求済));
    外部列.push(外部作用::決済_請求 { 注文ID: 入力.注文ID.clone(), 金額: i64::try_from(範囲内("金額", (注文.合計 as i128), None, None)).expect("物理数に収まらない"), 冪等キー: 入力.注文ID.clone() });
    return 決済要求出力 { 結果: 決済要求結果::要求済, 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 決済結果の反映 ----
pub struct 決済結果の反映入力 {
    pub 注文ID: String,
}
impl 決済結果の反映入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct 決済結果の反映文脈 {
}
pub struct 決済結果の反映出力 {
    pub 結果: 待ち解除結果,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn 決済結果の反映(ctx: &決済結果の反映文脈, 保存: &保存, 入力: &決済結果の反映入力) -> 決済結果の反映出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let 注文 = match 保存.注文.get(&(入力.注文ID.clone())) {
        Some(r) => r,
        None => return 決済結果の反映出力 { 結果: 待ち解除結果::注文なし, 書込: 書込列, 外部: 外部列 },
    };
    if (注文.状態 != 注文状態::決済要求済) {
        return 決済結果の反映出力 { 結果: 待ち解除結果::状態違い, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::注文_状態(注文.注文ID.clone(), 注文状態::完了));
    return 決済結果の反映出力 { 結果: 待ち解除結果::反映済, 書込: 書込列, 外部: 外部列 };
}

// ---- 処理 決済の期限切れ ----
pub struct 決済の期限切れ入力 {
    pub 注文ID: String,
}
impl 決済の期限切れ入力 {
    /// 外部から取り込むときの、範囲の宣言の検査
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）
pub struct 決済の期限切れ文脈 {
}
pub struct 決済の期限切れ出力 {
    pub 結果: 待ち解除結果,
    pub 書込: Vec<書込>,
    pub 外部: Vec<外部作用>,
}
pub fn 決済の期限切れ(ctx: &決済の期限切れ文脈, 保存: &保存, 入力: &決済の期限切れ入力) -> 決済の期限切れ出力 {
    let mut 書込列: Vec<書込> = Vec::new();
    let mut 外部列: Vec<外部作用> = Vec::new();
    let 注文 = match 保存.注文.get(&(入力.注文ID.clone())) {
        Some(r) => r,
        None => return 決済の期限切れ出力 { 結果: 待ち解除結果::注文なし, 書込: 書込列, 外部: 外部列 },
    };
    if (注文.状態 != 注文状態::決済要求済) {
        return 決済の期限切れ出力 { 結果: 待ち解除結果::状態違い, 書込: 書込列, 外部: 外部列 };
    }
    書込列.push(書込::注文_状態(注文.注文ID.clone(), 注文状態::キャンセル));
    return 決済の期限切れ出力 { 結果: 待ち解除結果::反映済, 書込: 書込列, 外部: 外部列 };
}

// ---- 問い合わせ 注文一覧（読むだけ） ----
pub struct 注文一覧入力 {
}
impl 注文一覧入力 {
    pub fn 検査(&self) -> Result<(), String> {
        Ok(())
    }
}
pub struct 注文一覧文脈 {
}
#[derive(Clone, Debug, PartialEq)]
pub struct 注文一覧行 {
    pub 注文ID: String,
    pub 顧客: String,
    pub 点数: i128,
}
impl 注文一覧行 {
    pub fn 表示(&self) -> String { format!("注文ID={}/顧客={}/点数={}", self.注文ID, self.顧客, self.点数) }
}
pub fn 注文一覧(ctx: &注文一覧文脈, 保存: &保存, 入力: &注文一覧入力) -> Vec<注文一覧行> {
    let mut 行: Vec<&注文> = 保存.注文.values().filter(|r| {
        if !(r.状態 != 注文状態::キャンセル) { return false; }
        true
    }).collect();
    行.sort_by(|a, b| a.注文ID.cmp(&b.注文ID));
    行.into_iter().filter_map(|r| {
        let j_顧客 = 保存.顧客.get(&r.顧客)?;  // 参照先が無い行は出さない
        Some(注文一覧行 { 注文ID: r.注文ID.clone(), 顧客: j_顧客.顧客ID.clone(), 点数: r.明細.values().map(|v| *v as i128).sum::<i128>() })
    }).collect()
}

// 【フロー】：結果の種類ごとの行き先。match の網羅は Rust が検査する。
pub fn 注文受付_受注(r: &受注結果) -> 行き先 {
    match r {
        受注結果::受付(..) => 行き先::処理("決済要求"),
        受注結果::在庫不足(..) => 行き先::応答,
        受注結果::商品なし(..) => 行き先::応答,
        受注結果::顧客なし => 行き先::応答,
    }
}

pub fn 注文受付_決済要求(r: &決済要求結果) -> 行き先 {
    match r {
        決済要求結果::要求済 => 行き先::待つ("決済待ち"),
        決済要求結果::注文なし => 行き先::応答,
        決済要求結果::状態違い => 行き先::応答,
        決済要求結果::合計ゼロ => 行き先::応答,
    }
}

pub fn 決済待ち解除_決済結果の反映(r: &待ち解除結果) -> 行き先 {
    match r {
        待ち解除結果::反映済 => 行き先::応答,
        待ち解除結果::注文なし => 行き先::応答,
        待ち解除結果::状態違い => 行き先::応答,
    }
}

pub fn 決済待ち解除_決済の期限切れ(r: &待ち解除結果) -> 行き先 {
    match r {
        待ち解除結果::反映済 => 行き先::応答,
        待ち解除結果::注文なし => 行き先::応答,
        待ち解除結果::状態違い => 行き先::応答,
    }
}

// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。
pub const トリガ一覧: &[トリガ定義] = &[
    トリガ定義 { 名前: "決済待ち", 要因: &[("応答", "決済成功", "決済結果の反映"), ("期限", "1800", "決済の期限切れ")] },
];

