/-
  geas の式と数の範囲（区間）の証明：核 1 の試作。

  対応する Rust の実装：
  - 区間の計算：crates/geas-core/src/check/num.rs の iv_add／iv_neg／iv_mul
  - 式ごとの区間：crates/geas-core/src/check/procs2.rs の interval（+ - * min max abs）
  - 生成：crates/geas-core/src/rustgen.rs の「+ - *」（checked_*／wrapping_*）と abs

  示すこと：
  1. 区間は正しい。値が宣言の区間に入っていれば、式の値は計算した区間に入る。
  2. 検査が通れば（どの途中の計算も i128 に収まると示せれば）、生成した Rust の
     checked_* の計算は失敗せず、桁の限りがない整数の値（.geas の意味）と同じ値を返す。
  3. 同じ検査で、wrapping_*（trust prove／assume の式）の計算も、同じ値を返す。

  ここでは、Rust の有理数（BigRational）の代わりに整数を使い、式は + - * 符号反転 min max abs
  に限る。割り算・if 式・割合・文字列は、次の段階で足す。
-/

namespace Geas

/-! ## 値の範囲（i128） -/

def i128Min : Int := -(2 ^ 127)
def i128Max : Int := 2 ^ 127 - 1

def inRange (x : Int) : Prop := i128Min ≤ x ∧ x ≤ i128Max

instance (x : Int) : Decidable (inRange x) := by unfold inRange; infer_instance

/-- checked_* の結果：収まれば some、あふれれば none -/
def chk (x : Int) : Option Int := if inRange x then some x else none

/-- wrapping_* の結果：2 の補数で 128 ビットに丸める -/
def wrap (x : Int) : Int := (x - i128Min) % (2 ^ 128) + i128Min

theorem wrap_of_inRange {x : Int} (h : inRange x) : wrap x = x := by
  unfold inRange i128Min i128Max at h
  unfold wrap i128Min
  have h2 : (0:Int) ≤ x - -(2 ^ 127) := by omega
  have h3 : x - -(2 ^ 127) < 2 ^ 128 := by
    have : (2:Int) ^ 128 = 2 ^ 127 + 2 ^ 127 := by decide
    omega
  rw [Int.emod_eq_of_lt h2 h3]
  omega

/-! ## 区間（None は限りなし。num.rs の Iv） -/

structure Iv where
  lo : Option Int
  hi : Option Int

def Iv.mem (x : Int) (v : Iv) : Prop :=
  (∀ l, v.lo = some l → l ≤ x) ∧ (∀ h, v.hi = some h → x ≤ h)

/-- 区間の両端が分かっていて、i128 に収まる（検査が「収まる」と示せたこと） -/
def Iv.fits (v : Iv) : Prop :=
  ∃ l h, v.lo = some l ∧ v.hi = some h ∧ inRange l ∧ inRange h

theorem Iv.fits_inRange {v : Iv} {x : Int} (hf : v.fits) (hm : v.mem x) : inRange x := by
  obtain ⟨l, h, hl, hh, ⟨hl1, hl2⟩, ⟨hh1, hh2⟩⟩ := hf
  have a := hm.1 l hl
  have b := hm.2 h hh
  unfold inRange
  omega

def Iv.add (a b : Iv) : Iv :=
  ⟨match a.lo, b.lo with | some x, some y => some (x + y) | _, _ => none,
   match a.hi, b.hi with | some x, some y => some (x + y) | _, _ => none⟩

def Iv.neg (a : Iv) : Iv := ⟨a.hi.map (fun x => -x), a.lo.map (fun x => -x)⟩

def min4 (a b c d : Int) : Int := min (min a b) (min c d)
def max4 (a b c d : Int) : Int := max (max a b) (max c d)

/-- num.rs の iv_mul をそのまま写したもの -/
def Iv.mul (a b : Iv) : Iv :=
  match a.lo, b.lo with
  | some a0, some b0 =>
    if 0 ≤ a0 ∧ 0 ≤ b0 then
      ⟨some (a0 * b0), match a.hi, b.hi with | some x, some y => some (x * y) | _, _ => none⟩
    else
      match a.hi, b.hi with
      | some a1, some b1 =>
        ⟨some (min4 (a0 * b0) (a0 * b1) (a1 * b0) (a1 * b1)),
         some (max4 (a0 * b0) (a0 * b1) (a1 * b0) (a1 * b1))⟩
      | _, _ => ⟨none, none⟩
  | _, _ => ⟨none, none⟩

def Iv.min (a b : Iv) : Iv :=
  ⟨match a.lo, b.lo with | some x, some y => some (Min.min x y) | _, _ => none,
   match a.hi, b.hi with
   | some x, some y => some (Min.min x y)
   | some x, none => some x
   | none, some y => some y
   | none, none => none⟩

def Iv.max (a b : Iv) : Iv :=
  ⟨match a.lo, b.lo with
   | some x, some y => some (Max.max x y)
   | some x, none => some x
   | none, some y => some y
   | none, none => none,
   match a.hi, b.hi with | some x, some y => some (Max.max x y) | _, _ => none⟩

/-- procs2.rs の abs：下限が 0 以上なら同じ、上限が 0 以下なら反転、どちらでもなければ 0..max(-lo, hi) -/
def Iv.abs (a : Iv) : Iv :=
  match a.lo, a.hi with
  | some l, _ => if 0 ≤ l then a else
      match a.hi with
      | some h => if h ≤ 0 then ⟨some (-h), some (-l)⟩ else ⟨some 0, some (Max.max (-l) h)⟩
      | none => ⟨some 0, none⟩
  | none, some h => if h ≤ 0 then ⟨some (-h), none⟩ else ⟨some 0, none⟩
  | none, none => ⟨some 0, none⟩

/-! ## 式 -/

inductive Expr where
  | num (v : Int)
  | var (i : Nat)
  | neg (e : Expr)
  | abs (e : Expr)
  | add (a b : Expr)
  | sub (a b : Expr)
  | mul (a b : Expr)
  | min (a b : Expr)
  | max (a b : Expr)

/-- .geas の意味：桁の限りがない整数 -/
def Expr.eval (env : Nat → Int) : Expr → Int
  | .num v => v
  | .var i => env i
  | .neg e => - e.eval env
  | .abs e => Int.ofNat (e.eval env).natAbs
  | .add a b => a.eval env + b.eval env
  | .sub a b => a.eval env - b.eval env
  | .mul a b => a.eval env * b.eval env
  | .min a b => Min.min (a.eval env) (b.eval env)
  | .max a b => Max.max (a.eval env) (b.eval env)

/-- 生成した Rust（checked_*）の意味：あふれたら none（Fault::Overflow） -/
def Expr.evalC (env : Nat → Int) : Expr → Option Int
  | .num v => some v
  | .var i => some (env i)
  | .neg e => (e.evalC env).bind fun x => chk (-x)
  | .abs e => (e.evalC env).bind fun x => chk (Int.ofNat x.natAbs)
  | .add a b => (a.evalC env).bind fun x => (b.evalC env).bind fun y => chk (x + y)
  | .sub a b => (a.evalC env).bind fun x => (b.evalC env).bind fun y => chk (x - y)
  | .mul a b => (a.evalC env).bind fun x => (b.evalC env).bind fun y => chk (x * y)
  | .min a b => (a.evalC env).bind fun x => (b.evalC env).map fun y => Min.min x y
  | .max a b => (a.evalC env).bind fun x => (b.evalC env).map fun y => Max.max x y

/-- 生成した Rust（wrapping_*：trust prove／assume の式）の意味 -/
def Expr.evalW (env : Nat → Int) : Expr → Int
  | .num v => v
  | .var i => env i
  | .neg e => wrap (- e.evalW env)
  | .abs e => wrap (Int.ofNat (e.evalW env).natAbs)
  | .add a b => wrap (a.evalW env + b.evalW env)
  | .sub a b => wrap (a.evalW env - b.evalW env)
  | .mul a b => wrap (a.evalW env * b.evalW env)
  | .min a b => Min.min (a.evalW env) (b.evalW env)
  | .max a b => Max.max (a.evalW env) (b.evalW env)

/-- 検査器の区間（宣言した名前ごとの区間 decl から） -/
def Expr.iv (decl : Nat → Iv) : Expr → Iv
  | .num v => ⟨some v, some v⟩
  | .var i => decl i
  | .neg e => (e.iv decl).neg
  | .abs e => (e.iv decl).abs
  | .add a b => (a.iv decl).add (b.iv decl)
  | .sub a b => (a.iv decl).add (b.iv decl).neg
  | .mul a b => (a.iv decl).mul (b.iv decl)
  | .min a b => (a.iv decl).min (b.iv decl)
  | .max a b => (a.iv decl).max (b.iv decl)

/-- 検査：演算の結果の区間が、どれも i128 に収まると示せる -/
def Expr.ok (decl : Nat → Iv) : Expr → Prop
  | .num v => inRange v
  | .var i => (decl i).fits
  | .neg e => e.ok decl ∧ (e.iv decl).neg.fits
  | .abs e => e.ok decl ∧ (e.iv decl).abs.fits
  | .add a b => a.ok decl ∧ b.ok decl ∧ ((a.iv decl).add (b.iv decl)).fits
  | .sub a b => a.ok decl ∧ b.ok decl ∧ ((a.iv decl).add (b.iv decl).neg).fits
  | .mul a b => a.ok decl ∧ b.ok decl ∧ ((a.iv decl).mul (b.iv decl)).fits
  | .min a b => a.ok decl ∧ b.ok decl
  | .max a b => a.ok decl ∧ b.ok decl


/-! ## 1. 区間は正しい -/

theorem mem_add {a b : Iv} {x y : Int} (hx : a.mem x) (hy : b.mem y) : (a.add b).mem (x + y) := by
  constructor
  · intro l hl
    unfold Iv.add at hl
    cases ha : a.lo <;> cases hb : b.lo <;> rw [ha, hb] at hl <;> simp at hl
    have := hx.1 _ ha; have := hy.1 _ hb; omega
  · intro h hh
    unfold Iv.add at hh
    cases ha : a.hi <;> cases hb : b.hi <;> rw [ha, hb] at hh <;> simp at hh
    have := hx.2 _ ha; have := hy.2 _ hb; omega

theorem mem_neg {a : Iv} {x : Int} (hx : a.mem x) : a.neg.mem (-x) := by
  constructor
  · intro l hl
    unfold Iv.neg at hl
    cases h : a.hi <;> rw [h] at hl <;> simp at hl
    have := hx.2 _ h; omega
  · intro h hh
    unfold Iv.neg at hh
    cases hl : a.lo <;> rw [hl] at hh <;> simp at hh
    have := hx.1 _ hl; omega


theorem bil {p q t : Int} (hp : p ≤ t) (hq : t ≤ q) (c : Int) :
    Min.min (p * c) (q * c) ≤ t * c ∧ t * c ≤ Max.max (p * c) (q * c) := by
  rcases Int.le_total 0 c with hc | hc
  · have h1 := Int.mul_le_mul_of_nonneg_right hp hc
    have h2 := Int.mul_le_mul_of_nonneg_right hq hc
    omega
  · have h1 := Int.mul_le_mul_of_nonpos_right hp hc
    have h2 := Int.mul_le_mul_of_nonpos_right hq hc
    omega

theorem corners {a0 a1 b0 b1 x y : Int} (h1 : a0 ≤ x) (h2 : x ≤ a1) (h3 : b0 ≤ y) (h4 : y ≤ b1) :
    min4 (a0 * b0) (a0 * b1) (a1 * b0) (a1 * b1) ≤ x * y ∧
    x * y ≤ max4 (a0 * b0) (a0 * b1) (a1 * b0) (a1 * b1) := by
  have e1 := bil h1 h2 y
  have e2 := bil h3 h4 a0
  have e3 := bil h3 h4 a1
  rw [Int.mul_comm y a0, Int.mul_comm y a1] at *
  rw [Int.mul_comm b0 a0, Int.mul_comm b1 a0, Int.mul_comm b0 a1, Int.mul_comm b1 a1] at *
  unfold min4 max4
  omega

theorem mem_mul {a b : Iv} {x y : Int} (hx : a.mem x) (hy : b.mem y) : (a.mul b).mem (x * y) := by
  unfold Iv.mul
  cases ha : a.lo with
  | none => simp [Iv.mem]
  | some a0 =>
  cases hb : b.lo with
  | none => simp [Iv.mem]
  | some b0 =>
  have hxa := hx.1 _ ha
  have hyb := hy.1 _ hb
  simp only []
  split
  · rename_i hnn
    constructor
    · intro l hl
      simp at hl
      subst hl
      exact Int.mul_le_mul hxa hyb hnn.2 (Int.le_trans hnn.1 hxa)
    · intro h hh
      cases ha1 : a.hi <;> cases hb1 : b.hi <;> rw [ha1, hb1] at hh <;> simp at hh
      subst hh
      have := hx.2 _ ha1; have := hy.2 _ hb1
      exact Int.mul_le_mul (hx.2 _ ha1) (hy.2 _ hb1) (Int.le_trans hnn.2 hyb) (Int.le_trans hnn.1 (Int.le_trans hxa (hx.2 _ ha1)))
  · cases ha1 : a.hi <;> cases hb1 : b.hi <;> simp
    · exact ⟨by intro l hl; simp at hl, by intro h hh; simp at hh⟩
    · exact ⟨by intro l hl; simp at hl, by intro h hh; simp at hh⟩
    · exact ⟨by intro l hl; simp at hl, by intro h hh; simp at hh⟩
    · rename_i a1 b1
      have c := corners hxa (hx.2 _ ha1) hyb (hy.2 _ hb1)
      exact ⟨by intro l hl; simp at hl; omega, by intro h hh; simp at hh; omega⟩


theorem mem_min {a b : Iv} {x y : Int} (hx : a.mem x) (hy : b.mem y) : (a.min b).mem (Min.min x y) := by
  constructor
  · intro l hl
    unfold Iv.min at hl
    cases ha : a.lo <;> cases hb : b.lo <;> rw [ha, hb] at hl <;> simp at hl
    have := hx.1 _ ha; have := hy.1 _ hb; omega
  · intro h hh
    unfold Iv.min at hh
    cases ha : a.hi <;> cases hb : b.hi <;> rw [ha, hb] at hh <;> simp at hh
    · have := hy.2 _ hb; omega
    · have := hx.2 _ ha; omega
    · have := hx.2 _ ha; have := hy.2 _ hb; omega

theorem mem_max {a b : Iv} {x y : Int} (hx : a.mem x) (hy : b.mem y) : (a.max b).mem (Max.max x y) := by
  constructor
  · intro l hl
    unfold Iv.max at hl
    cases ha : a.lo <;> cases hb : b.lo <;> rw [ha, hb] at hl <;> simp at hl
    · have := hy.1 _ hb; omega
    · have := hx.1 _ ha; omega
    · have := hx.1 _ ha; have := hy.1 _ hb; omega
  · intro h hh
    unfold Iv.max at hh
    cases ha : a.hi <;> cases hb : b.hi <;> rw [ha, hb] at hh <;> simp at hh
    have := hx.2 _ ha; have := hy.2 _ hb; omega

theorem mem_abs {a : Iv} {x : Int} (hx : a.mem x) : a.abs.mem (Int.ofNat x.natAbs) := by
  unfold Iv.abs
  cases hl : a.lo with
  | none =>
    cases hh : a.hi with
    | none => simp; constructor <;> intros <;> simp_all <;> omega
    | some h =>
      have := hx.2 _ hh
      by_cases c : h ≤ 0
      · simp [c]; constructor <;> intro k hk <;> simp_all <;> omega
      · simp [c]; constructor <;> intro k hk <;> simp_all <;> omega
  | some l =>
    have := hx.1 _ hl
    by_cases c : 0 ≤ l
    · simp [c]; exact ⟨fun k hk => by have := hx.1 _ hk; omega, fun k hk => by have := hx.2 _ hk; omega⟩
    · cases hh : a.hi with
      | none => simp [c]; constructor <;> intro k hk <;> simp_all <;> omega
      | some h =>
        have := hx.2 _ hh
        by_cases c2 : h ≤ 0
        · simp [c, c2]; constructor <;> intro k hk <;> simp_all <;> omega
        · simp [c, c2]; constructor <;> intro k hk <;> simp_all <;> omega

/-! ## 主な定理 -/

theorem sound (decl : Nat → Iv) (env : Nat → Int) (henv : ∀ i, (decl i).mem (env i)) :
    ∀ e : Expr, (e.iv decl).mem (e.eval env)
  | .num v => by simp [Expr.iv, Expr.eval, Iv.mem]
  | .var i => henv i
  | .neg e => mem_neg (sound decl env henv e)
  | .abs e => mem_abs (sound decl env henv e)
  | .add a b => mem_add (sound decl env henv a) (sound decl env henv b)
  | .sub a b => by
      have := mem_add (sound decl env henv a) (mem_neg (sound decl env henv b))
      simpa [Expr.iv, Expr.eval, Int.sub_eq_add_neg] using this
  | .mul a b => mem_mul (sound decl env henv a) (sound decl env henv b)
  | .min a b => mem_min (sound decl env henv a) (sound decl env henv b)
  | .max a b => mem_max (sound decl env henv a) (sound decl env henv b)


/-- 検査が通れば、生成した Rust の checked_* は失敗せず、.geas の意味と同じ値を返す -/
theorem checked (decl : Nat → Iv) (env : Nat → Int) (henv : ∀ i, (decl i).mem (env i)) :
    ∀ e : Expr, e.ok decl → e.evalC env = some (e.eval env)
  | .num v, _ => by simp [Expr.evalC, Expr.eval]
  | .var i, _ => by simp [Expr.evalC, Expr.eval]
  | .neg e, ⟨h1, h2⟩ => by
      have ih := checked decl env henv e h1
      have hr : inRange (-(e.eval env)) := Iv.fits_inRange h2 (mem_neg (sound decl env henv e))
      simp [Expr.evalC, ih, chk, hr, Expr.eval]
  | .abs e, ⟨h1, h2⟩ => by
      have ih := checked decl env henv e h1
      have hr : inRange (Int.ofNat (e.eval env).natAbs) := Iv.fits_inRange h2 (mem_abs (sound decl env henv e))
      simp [Expr.evalC, ih, chk, Expr.eval]
      exact hr
  | .add a b, ⟨h1, h2, h3⟩ => by
      have ia := checked decl env henv a h1
      have ib := checked decl env henv b h2
      have hr : inRange (a.eval env + b.eval env) :=
        Iv.fits_inRange h3 (mem_add (sound decl env henv a) (sound decl env henv b))
      simp [Expr.evalC, ia, ib, chk, hr, Expr.eval]
  | .sub a b, ⟨h1, h2, h3⟩ => by
      have ia := checked decl env henv a h1
      have ib := checked decl env henv b h2
      have hr : inRange (a.eval env - b.eval env) := by
        have := Iv.fits_inRange h3 (mem_add (sound decl env henv a) (mem_neg (sound decl env henv b)))
        simpa [Int.sub_eq_add_neg] using this
      simp [Expr.evalC, ia, ib, chk, hr, Expr.eval]
  | .mul a b, ⟨h1, h2, h3⟩ => by
      have ia := checked decl env henv a h1
      have ib := checked decl env henv b h2
      have hr : inRange (a.eval env * b.eval env) :=
        Iv.fits_inRange h3 (mem_mul (sound decl env henv a) (sound decl env henv b))
      simp [Expr.evalC, ia, ib, chk, hr, Expr.eval]
  | .min a b, ⟨h1, h2⟩ => by
      simp [Expr.evalC, checked decl env henv a h1, checked decl env henv b h2, Expr.eval]
  | .max a b, ⟨h1, h2⟩ => by
      simp [Expr.evalC, checked decl env henv a h1, checked decl env henv b h2, Expr.eval]

/-- 同じ検査で、wrapping_*（trust prove／assume の式）も、.geas の意味と同じ値を返す -/
theorem wrapping (decl : Nat → Iv) (env : Nat → Int) (henv : ∀ i, (decl i).mem (env i)) :
    ∀ e : Expr, e.ok decl → e.evalW env = e.eval env
  | .num v, _ => by simp [Expr.evalW, Expr.eval]
  | .var i, _ => by simp [Expr.evalW, Expr.eval]
  | .neg e, ⟨h1, h2⟩ => by
      have ih := wrapping decl env henv e h1
      have hr : inRange (-(e.eval env)) := Iv.fits_inRange h2 (mem_neg (sound decl env henv e))
      simp [Expr.evalW, ih, Expr.eval, wrap_of_inRange hr]
  | .abs e, ⟨h1, h2⟩ => by
      have ih := wrapping decl env henv e h1
      have hr : inRange (Int.ofNat (e.eval env).natAbs) := Iv.fits_inRange h2 (mem_abs (sound decl env henv e))
      simp [Expr.evalW, ih, Expr.eval]
      exact wrap_of_inRange hr
  | .add a b, ⟨h1, h2, h3⟩ => by
      have ia := wrapping decl env henv a h1
      have ib := wrapping decl env henv b h2
      have hr : inRange (a.eval env + b.eval env) :=
        Iv.fits_inRange h3 (mem_add (sound decl env henv a) (sound decl env henv b))
      simp [Expr.evalW, ia, ib, Expr.eval, wrap_of_inRange hr]
  | .sub a b, ⟨h1, h2, h3⟩ => by
      have ia := wrapping decl env henv a h1
      have ib := wrapping decl env henv b h2
      have hr : inRange (a.eval env - b.eval env) := by
        have := Iv.fits_inRange h3 (mem_add (sound decl env henv a) (mem_neg (sound decl env henv b)))
        simpa [Int.sub_eq_add_neg] using this
      simp [Expr.evalW, ia, ib, Expr.eval, wrap_of_inRange hr]
  | .mul a b, ⟨h1, h2, h3⟩ => by
      have ia := wrapping decl env henv a h1
      have ib := wrapping decl env henv b h2
      have hr : inRange (a.eval env * b.eval env) :=
        Iv.fits_inRange h3 (mem_mul (sound decl env henv a) (sound decl env henv b))
      simp [Expr.evalW, ia, ib, Expr.eval, wrap_of_inRange hr]
  | .min a b, ⟨h1, h2⟩ => by
      simp [Expr.evalW, wrapping decl env henv a h1, wrapping decl env henv b h2, Expr.eval]
  | .max a b, ⟨h1, h2⟩ => by
      simp [Expr.evalW, wrapping decl env henv a h1, wrapping decl env henv b h2, Expr.eval]

#print axioms sound
#print axioms checked
#print axioms wrapping

end Geas
