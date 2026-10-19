//! Typed expressions for numeric fields: `2in + 5mm`, `(12 - 0.5) / 3`, `*2`. Pure; the field widgets
//! in `units.rs` are the only callers.

use std::fmt;
use std::ops::RangeInclusive;

use crate::units::Units;

/// What a field holds. A length takes unit suffixes and reads a bare number in `display`; a plain
/// number (an angle, a count, a percentage) refuses units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    Length(Units),
    Plain,
}

/// A parsed entry: a new value (inches for a length), or a factor on each current value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    To(f32),
    Times(f32),
}

impl Typed {
    pub fn apply(self, current: f32) -> f32 {
        match self {
            Self::To(v) => v,
            Self::Times(k) => current * k,
        }
    }

    /// The value this entry gives against a selection's shared value; a scale has nothing to scale
    /// when the selection disagrees.
    pub fn against(self, common: Option<f32>) -> Option<f32> {
        match self {
            Self::To(v) => Some(v),
            Self::Times(_) => common.map(|v| self.apply(v)),
        }
    }

    /// Applies the entry to `value`, clamped to `range`. Returns whether `value` changed.
    pub fn set(self, value: &mut f32, range: RangeInclusive<f32>) -> bool {
        let before = *value;
        *value = self.apply(before).clamp(*range.start(), *range.end());
        *value != before
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError {
    /// 1-based character position.
    pub at: usize,
    pub reason: String,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at character {}", self.reason, self.at)
    }
}

impl std::error::Error for ExprError {}

/// Grammar: `[= | * | /] sum`, `sum = product (+|- product)*`, `product = unary (*|/ unary)*`,
/// `unary = -unary | +unary | number [unit] | ( sum )`. Commas read as decimal points.
pub fn parse(input: &str, q: Quantity) -> Result<Typed, ExprError> {
    let mut p = Parser { chars: input.chars().collect(), i: 0, q };
    let lead_at = p.skip_ws();
    let lead = p.chars.get(lead_at).copied().filter(|c| matches!(c, '=' | '*' | '/'));
    if lead.is_some() {
        p.i += 1;
    }
    let value = p.sum()?;
    if let Some(c) = p.peek() {
        return Err(p.error(p.i, format!("Unexpected '{c}'")));
    }
    let finite = |v: f64| Some(v as f32).filter(|v| v.is_finite()).ok_or_else(|| ExprError { at: 1, reason: "Too large".into() });
    match lead {
        Some(op @ ('*' | '/')) => {
            if value.dim != 0 {
                return Err(p.error(lead_at, "Scale by a plain number"));
            }
            if op == '/' && value.v == 0.0 {
                return Err(p.error(lead_at, "Division by zero"));
            }
            finite(if op == '*' { value.v } else { value.v.recip() }).map(Typed::Times)
        }
        _ => finite(if value.dim == 0 { value.v * p.bare_unit() } else { value.v }).map(Typed::To),
    }
}

/// A number with its power of length: `dim` 0 is bare, 1 is inches. A bare number meets a length
/// in the display unit.
#[derive(Clone, Copy)]
struct Value {
    v: f64,
    dim: i32,
}

struct Parser {
    chars: Vec<char>,
    i: usize,
    q: Quantity,
}

impl Parser {
    fn error(&self, index: usize, reason: impl Into<String>) -> ExprError {
        ExprError { at: index + 1, reason: reason.into() }
    }

    fn skip_ws(&mut self) -> usize {
        while self.chars.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
        self.i
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.chars.get(self.i).copied()
    }

    /// Inches per bare unit; 1 for a plain field.
    fn bare_unit(&self) -> f64 {
        match self.q {
            Quantity::Length(display) => f64::from(display.to_in(1.0)),
            Quantity::Plain => 1.0,
        }
    }

    fn sum(&mut self) -> Result<Value, ExprError> {
        let mut acc = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            let at = self.i;
            self.i += 1;
            let rhs = self.product()?;
            let (a, b) = match (acc.dim, rhs.dim) {
                (x, y) if x == y => (acc.v, rhs.v),
                (0, 1) => (acc.v * self.bare_unit(), rhs.v),
                (1, 0) => (acc.v, rhs.v * self.bare_unit()),
                _ => return Err(self.error(at, "Not a length")),
            };
            acc = Value { v: if op == '+' { a + b } else { a - b }, dim: acc.dim.max(rhs.dim) };
        }
        Ok(acc)
    }

    fn product(&mut self) -> Result<Value, ExprError> {
        let mut acc = self.unary()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            let at = self.i;
            self.i += 1;
            let rhs = self.unary()?;
            let dim = if op == '*' { acc.dim + rhs.dim } else { acc.dim - rhs.dim };
            if !(0..=1).contains(&dim) {
                return Err(self.error(at, "Not a length"));
            }
            if op == '/' && rhs.v == 0.0 {
                return Err(self.error(at, "Division by zero"));
            }
            acc = Value { v: if op == '*' { acc.v * rhs.v } else { acc.v / rhs.v }, dim };
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<Value, ExprError> {
        match self.peek() {
            Some('-') => {
                self.i += 1;
                self.unary().map(|x| Value { v: -x.v, ..x })
            }
            Some('+') => {
                self.i += 1;
                self.unary()
            }
            Some('(') => {
                self.i += 1;
                let inner = self.sum()?;
                if self.peek() != Some(')') {
                    return Err(self.error(self.i, "Expected ')'"));
                }
                self.i += 1;
                if inner.dim == 0 { self.unit(inner.v) } else { Ok(inner) }
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Result<Value, ExprError> {
        let start = self.skip_ws();
        while self.chars.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | ',')) {
            self.i += 1;
        }
        let digits: String = self.chars[start..self.i].iter().map(|&c| if c == ',' { '.' } else { c }).collect();
        let Ok(v) = digits.parse::<f64>() else { return Err(self.error(start, "Expected a number")) };
        self.unit(v)
    }

    /// The suffix after a number, if any, converting it to inches.
    fn unit(&mut self, v: f64) -> Result<Value, ExprError> {
        let at = self.skip_ws();
        let word_end = |chars: &[char]| at + chars[at..].iter().take_while(|c| c.is_alphabetic()).count();
        let end = if self.chars.get(at) == Some(&'"') { at + 1 } else { word_end(&self.chars) };
        let word: String = self.chars[at..end].iter().collect();
        if self.q == Quantity::Plain {
            if !word.is_empty() {
                return Err(self.error(at, "Units are not allowed here"));
            }
            if self.chars.get(at).is_some_and(|c| matches!(c, '°' | '%')) {
                self.i += 1;
            }
            return Ok(Value { v, dim: 0 });
        }
        let per_inch = match word.as_str() {
            "" => return Ok(Value { v, dim: 0 }),
            "in" | "\"" => 1.0,
            "ft" => 12.0,
            "mm" => 1.0 / 25.4,
            "cm" => 1.0 / 2.54,
            "m" => 100.0 / 2.54,
            "pt" => 1.0 / 72.0,
            _ => return Err(self.error(at, format!("Unknown unit '{word}'"))),
        };
        self.i = end;
        Ok(Value { v: v * per_inch, dim: 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: Quantity = Quantity::Length(Units::Imperial);
    const CM: Quantity = Quantity::Length(Units::Metric);

    fn to(input: &str, q: Quantity) -> f32 {
        match parse(input, q) {
            Ok(Typed::To(v)) => v,
            other => panic!("{input}: {other:?}"),
        }
    }

    fn err(input: &str, q: Quantity) -> String {
        parse(input, q).expect_err(input).to_string()
    }

    #[test]
    fn mixed_units_add_up_in_inches() {
        assert!((to("2in + 5mm", IN) - 2.196_85).abs() < 1e-5);
        assert!((to("30cm - 2 * 1in", IN) - (30.0 / 2.54 - 2.0)).abs() < 1e-5);
        assert!((to("1ft", CM) - 12.0).abs() < 1e-6);
        assert!((to("72pt", IN) - 1.0).abs() < 1e-6);
        assert!((to("1m", IN) - 100.0 / 2.54).abs() < 1e-4);
        assert!((to("3\"", CM) - 3.0).abs() < 1e-6);
        assert!((to("2 in", CM) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_bare_number_takes_the_display_unit() {
        assert!((Units::Metric.from_in(to("(12 - 0.5) / 3", CM)) - 3.8333).abs() < 1e-4);
        assert_eq!(to("2", IN), 2.0);
        assert!((to("1in + 2.54", CM) - 2.0).abs() < 1e-6, "the bare 2.54 is centimetres");
        assert_eq!(to("4,25", IN), 4.25);
        assert_eq!(to("-(1 + 2) * 2", IN), -6.0);
        assert_eq!(to(".5", IN), 0.5);
        assert_eq!(to("(1 + 1)in", CM), 2.0);
        assert!((to("4in / 2in", CM) - 2.0 / 2.54).abs() < 1e-6, "a ratio of lengths is a bare number");
    }

    #[test]
    fn a_leading_operator_scales_the_current_value() {
        assert_eq!(parse("*2", IN).unwrap().apply(1.5), 3.0);
        assert!((parse("/3", IN).unwrap().apply(3.0) - 1.0).abs() < 1e-6);
        assert_eq!(parse(" * (1 + 1)", Quantity::Plain).unwrap(), Typed::Times(2.0));
        assert_eq!(parse("=2in", CM).unwrap().apply(5.0), 2.0);
        assert_eq!(parse("=*2", IN).expect_err("= then *").at, 2);
    }

    #[test]
    fn errors_name_the_character_they_stopped_at() {
        assert_eq!(err("2in +", IN), "Expected a number at character 6");
        assert_eq!(err("", IN), "Expected a number at character 1");
        assert_eq!(err("2 ) ", IN), "Unexpected ')' at character 3");
        assert_eq!(err("(2", IN), "Expected ')' at character 3");
        assert_eq!(err("2yd", IN), "Unknown unit 'yd' at character 2");
        assert_eq!(err("1 / 0", IN), "Division by zero at character 3");
        assert_eq!(err("2in * 3in", IN), "Not a length at character 5");
        assert_eq!(err("*2in", IN), "Scale by a plain number at character 1");
        assert_eq!(err("1e39", IN), "Unknown unit 'e' at character 2");
    }

    #[test]
    fn plain_fields_refuse_units_but_tolerate_their_own_marks() {
        assert_eq!(err("90in", Quantity::Plain), "Units are not allowed here at character 3");
        assert_eq!(parse("45 + 45", Quantity::Plain).unwrap(), Typed::To(90.0));
        assert_eq!(parse("90°", Quantity::Plain).unwrap(), Typed::To(90.0));
        assert_eq!(parse("50%", Quantity::Plain).unwrap(), Typed::To(50.0));
    }

    #[test]
    fn huge_results_are_refused() {
        assert_eq!(err("99999999999999999999 * 99999999999999999999", Quantity::Plain), "Too large at character 1");
    }
}
