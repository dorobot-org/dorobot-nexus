//! Just enough JSON to emit one object per line.
//!
//! Deliberately a writer and not a library. The headless surfaces need to print
//! records another process can parse without agreeing on a display format, and
//! nothing here needs to *read* JSON — so this is an emitter, and adding a
//! dependency for it would cost the property that a clean clone builds with
//! nothing but a Rust toolchain.
//!
//! Numbers are written at fixed precision rather than with `{}`: a consumer
//! diffing two runs wants `0.250`, not the shortest round-trip of a float, and
//! non-finite values are not JSON at all, so they become `null` instead of
//! silently emitting the literal `NaN` that every strict parser rejects.

/// A JSON object under construction. Field order is insertion order, which
/// keeps the output diffable between runs.
pub struct Obj {
    buf: String,
}

impl Obj {
    pub fn new() -> Self {
        Self { buf: String::from("{") }
    }

    fn sep(&mut self) {
        if self.buf.len() > 1 {
            self.buf.push(',');
        }
    }

    fn key(&mut self, k: &str) {
        self.sep();
        self.buf.push('"');
        escape_into(k, &mut self.buf);
        self.buf.push_str("\":");
    }

    pub fn str(mut self, k: &str, v: &str) -> Self {
        self.key(k);
        self.buf.push('"');
        escape_into(v, &mut self.buf);
        self.buf.push('"');
        self
    }

    pub fn bool(mut self, k: &str, v: bool) -> Self {
        self.key(k);
        self.buf.push_str(if v { "true" } else { "false" });
        self
    }

    /// An explicit absence. Emitting the key as `null` says "asked, and there
    /// is none", which a missing key does not.
    pub fn null(mut self, k: &str) -> Self {
        self.key(k);
        self.buf.push_str("null");
        self
    }

    /// `Some(n)` or an explicit `null`.
    pub fn opt_usize(self, k: &str, v: Option<usize>) -> Self {
        match v {
            Some(n) => self.usize(k, n),
            None => self.null(k),
        }
    }

    pub fn int(mut self, k: &str, v: u64) -> Self {
        self.key(k);
        self.buf.push_str(&v.to_string());
        self
    }

    pub fn usize(self, k: &str, v: usize) -> Self {
        self.int(k, v as u64)
    }

    /// A float at `dp` decimal places, or `null` if it is not finite.
    pub fn num(mut self, k: &str, v: f64, dp: usize) -> Self {
        self.key(k);
        if v.is_finite() {
            self.buf.push_str(&format!("{v:.dp$}"));
        } else {
            self.buf.push_str("null");
        }
        self
    }

    pub fn f32(self, k: &str, v: f32, dp: usize) -> Self {
        self.num(k, v as f64, dp)
    }

    /// An array of floats, same number rules as `num`.
    pub fn nums(mut self, k: &str, vs: &[f32], dp: usize) -> Self {
        self.key(k);
        push_nums(&mut self.buf, vs.iter().map(|v| Some(*v as f64)), dp);
        self
    }

    /// A rectangular grid of optional floats: an array of arrays, with `None`
    /// as `null`. A sweep cell that has not been computed and a cell that
    /// computed zero are different results and must not print the same.
    pub fn grid(mut self, k: &str, rows: &[Vec<Option<f64>>], dp: usize) -> Self {
        self.key(k);
        self.buf.push('[');
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            push_nums(&mut self.buf, row.iter().copied(), dp);
        }
        self.buf.push(']');
        self
    }

    pub fn strs(mut self, k: &str, vs: &[&str]) -> Self {
        self.key(k);
        self.buf.push('[');
        for (i, v) in vs.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            self.buf.push('"');
            escape_into(v, &mut self.buf);
            self.buf.push('"');
        }
        self.buf.push(']');
        self
    }

    /// Nest an already-built object under `k`.
    pub fn obj(mut self, k: &str, v: Obj) -> Self {
        self.key(k);
        self.buf.push_str(&v.done());
        self
    }

    pub fn done(mut self) -> String {
        self.buf.push('}');
        self.buf
    }
}

impl Default for Obj {
    fn default() -> Self {
        Self::new()
    }
}

/// Write `[a,b,…]`, with anything absent or non-finite as `null`.
fn push_nums(out: &mut String, vs: impl Iterator<Item = Option<f64>>, dp: usize) {
    out.push('[');
    for (i, v) in vs.enumerate() {
        if i > 0 {
            out.push(',');
        }
        match v {
            Some(v) if v.is_finite() => out.push_str(&format!("{v:.dp$}")),
            _ => out.push_str("null"),
        }
    }
    out.push(']');
}

/// Escape per RFC 8259: the two mandatory escapes, the shorthands, and every
/// remaining control character as `\u00XX`.
fn escape_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_keep_insertion_order() {
        let s = Obj::new().int("b", 2).int("a", 1).done();
        assert_eq!(s, r#"{"b":2,"a":1}"#);
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_controls() {
        let s = Obj::new().str("k", "a\"b\\c\nd\u{1}e").done();
        assert_eq!(s, r#"{"k":"a\"b\\c\nd\u0001e"}"#);
    }

    #[test]
    fn a_path_with_a_backslash_survives_a_round_trip_shape() {
        // Windows-style paths are the realistic source of stray backslashes.
        let s = Obj::new().str("path", r"C:\runs\ckpt.bin").done();
        assert_eq!(s, r#"{"path":"C:\\runs\\ckpt.bin"}"#);
    }

    #[test]
    fn non_finite_numbers_become_null_not_nan() {
        // `NaN` and `Infinity` are not JSON; emitting them makes the whole line
        // unparseable, which is worse than losing one field.
        let s = Obj::new()
            .num("nan", f64::NAN, 3)
            .num("inf", f64::INFINITY, 3)
            .done();
        assert_eq!(s, r#"{"nan":null,"inf":null}"#);
    }

    #[test]
    fn floats_are_fixed_precision_so_runs_diff_cleanly() {
        let s = Obj::new().num("x", 0.25, 3).f32("y", 1.0, 1).done();
        assert_eq!(s, r#"{"x":0.250,"y":1.0}"#);
    }

    #[test]
    fn arrays_carry_the_same_number_rules() {
        let s = Obj::new().nums("v", &[1.0, f32::NAN], 2).done();
        assert_eq!(s, r#"{"v":[1.00,null]}"#);
    }

    #[test]
    fn an_uncomputed_grid_cell_is_null_not_zero() {
        // The distinction the sweep depends on: a cell that has not run and a
        // cell that scored zero mean opposite things.
        let g = vec![vec![Some(0.0), None], vec![Some(1.0), Some(0.5)]];
        assert_eq!(
            Obj::new().grid("cells", &g, 2).done(),
            r#"{"cells":[[0.00,null],[1.00,0.50]]}"#
        );
    }

    #[test]
    fn objects_nest() {
        let inner = Obj::new().int("n", 1);
        assert_eq!(Obj::new().obj("a", inner).done(), r#"{"a":{"n":1}}"#);
    }

    #[test]
    fn an_empty_object_is_still_valid() {
        assert_eq!(Obj::new().done(), "{}");
    }

    #[test]
    fn an_absent_option_is_null_rather_than_a_missing_key() {
        let s = Obj::new().opt_usize("a", Some(3)).opt_usize("b", None).done();
        assert_eq!(s, r#"{"a":3,"b":null}"#);
    }
}
