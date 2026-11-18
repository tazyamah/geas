// SPDX-License-Identifier: GPL-3.0-or-later
//! Python の difflib.get_close_matches（n=1）に当たるもの。SequenceMatcher の ratio を、同じ手順で求める。
use std::collections::HashMap;

fn find_longest_match(a: &[char], b: &[char], b2j: &HashMap<char, Vec<usize>>, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for i in alo..ahi {
        let mut newj2len: HashMap<usize, usize> = HashMap::new();
        if let Some(js) = b2j.get(&a[i]) {
            for &j in js {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j2len.get(&(j.wrapping_sub(1))).copied().unwrap_or(0) + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

fn matching_blocks(a: &[char], b: &[char]) -> Vec<(usize, usize, usize)> {
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (i, &c) in b.iter().enumerate() {
        b2j.entry(c).or_default().push(i);
    }
    let mut queue = vec![(0usize, a.len(), 0usize, b.len())];
    let mut blocks = vec![];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = find_longest_match(a, b, &b2j, alo, ahi, blo, bhi);
        if k > 0 {
            blocks.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocks.sort();
    blocks
}

pub fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let m: usize = matching_blocks(&a, &b).iter().map(|x| x.2).sum();
    2.0 * m as f64 / total as f64
}

/// n=1 の get_close_matches：cutoff 以上の中で、(得点, 文字列) がいちばん大きいもの
pub fn get_close_match(word: &str, possibilities: &[String], cutoff: f64) -> Option<String> {
    let mut best: Option<(f64, &String)> = None;
    for x in possibilities {
        let r = ratio(x, word);
        if r >= cutoff {
            match best {
                Some((bs, bx)) if (bs, bx) >= (r, x) => {}
                _ => best = Some((r, x)),
            }
        }
    }
    best.map(|(_, x)| x.clone())
}
