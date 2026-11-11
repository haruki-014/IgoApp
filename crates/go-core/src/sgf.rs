//! SGF(FF[4])の出力。着手履歴から生成する。

use crate::color::Color;
use crate::game::Game;

/// 対局者名など、着手履歴に含まれない情報。
#[derive(Clone, Copy, Debug, Default)]
pub struct SgfInfo<'a> {
    pub black: &'a str,
    pub white: &'a str,
    /// `B+3.5`、`W+R` など。対局中は `None`。
    pub result: Option<&'a str>,
}

/// SGF のテキスト値として安全な形にする(`\` と `]` をエスケープ)。
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\\' || ch == ']' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 対局を SGF にする。含める情報は SZ、KM、RU[Chinese]、PB/PW、RE、着手、パス。
pub fn to_sgf(game: &Game, info: &SgfInfo<'_>) -> String {
    let mut out = String::new();
    out.push_str("(;GM[1]FF[4]CA[UTF-8]AP[igo:0.1]");
    out.push_str(&format!("SZ[{}]KM[{}]RU[Chinese]", game.size(), game.komi()));
    out.push_str(&format!("PB[{}]PW[{}]", escape(info.black), escape(info.white)));
    if let Some(result) = info.result {
        out.push_str(&format!("RE[{}]", escape(result)));
    }
    for (i, m) in game.moves().iter().enumerate() {
        if i % 10 == 0 {
            out.push('\n');
        }
        let color = match m.color {
            Color::Black => 'B',
            Color::White => 'W',
        };
        out.push_str(&format!(";{}[{}]", color, m.mv.to_sgf()));
    }
    out.push_str(")\n");
    out
}
