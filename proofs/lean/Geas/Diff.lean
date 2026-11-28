/-
  Rust の区間計算（crates/geas-core/src/check/num.rs）と、この証明のモデルを突き合わせるための実行ファイル。
  標準入力の 1 行が 1 件：「演算 a下限 a上限 b下限 b上限」（限りなしは _）。
  標準出力に、結果の「下限 上限」を 1 行ずつ出す。使い方は crates/geas-core/tests/lean_diff.rs を見る。
-/
import Geas.Arith
open Geas

def parseB (s : String) : Option Int := if s == "_" then none else s.toInt?

def showB : Option Int → String
  | none => "_"
  | some x => toString x

def runLine (line : String) : String :=
  match line.splitOn " " with
  | [op, a0, a1, b0, b1] =>
    let a : Iv := ⟨parseB a0, parseB a1⟩
    let b : Iv := ⟨parseB b0, parseB b1⟩
    let r : Iv := match op with
      | "add" => a.add b
      | "neg" => a.neg
      | "mul" => a.mul b
      | "min" => a.min b
      | "max" => a.max b
      | "abs" => a.abs
      | _ => ⟨none, none⟩
    s!"{showB r.lo} {showB r.hi}"
  | _ => "bad"

def main : IO Unit := do
  let stdin ← IO.getStdin
  let mut line ← stdin.getLine
  while line != "" do
    IO.println (runLine line.trimAscii.toString)
    line ← stdin.getLine
