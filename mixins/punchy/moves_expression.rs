//! Bounded compatibility-expression evaluator, matching Moves 1.0.9's
//! numeric/boolean semantics. Functions use radians (unlike clip Molang).
use std::collections::BTreeMap;

pub trait Context {
    fn get(&self, name: &str) -> Option<f64>;
    fn seed(&self) -> u64 {
        0
    }
}
impl Context for BTreeMap<String, f64> {
    fn get(&self, name: &str) -> Option<f64> {
        self.get(name).copied()
    }
}
#[derive(Clone, Debug)]
enum Node {
    Number(f64),
    Variable(String),
    Unary(u8, Box<Node>),
    Binary(&'static str, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}
#[derive(Clone, Debug)]
pub struct Expression(Node);
impl Expression {
    pub fn compile(text: &str) -> Result<Self, String> {
        if text.len() > 16384 {
            return Err("Expression exceeds 16 KiB".into());
        }
        let mut p = Parser {
            text: text.as_bytes(),
            at: 0,
            nodes: 0,
            depth: 0,
        };
        let node = p.expression(0)?;
        p.space();
        if p.at != p.text.len() {
            return Err(format!("Unexpected expression input at {}", p.at));
        }
        Ok(Self(node))
    }
    pub fn number(&self, context: &dyn Context, fallback: f64) -> f64 {
        self.0
            .eval(context)
            .filter(|v| v.is_finite())
            .unwrap_or(fallback)
    }
    pub fn condition(&self, context: &dyn Context, fallback: bool) -> bool {
        self.0.eval(context).map_or(fallback, |v| v != 0.)
    }
}
pub fn evaluate(text: &str, context: &dyn Context, fallback: f64) -> f64 {
    Expression::compile(text).map_or(fallback, |e| e.number(context, fallback))
}
impl Node {
    fn eval(&self, c: &dyn Context) -> Option<f64> {
        let boolean = |v: bool| if v { 1. } else { 0. };
        Some(match self {
            Self::Number(v) => *v,
            Self::Variable(n) => c.get(n)?,
            Self::Unary(op, node) => match op {
                b'-' => -node.eval(c)?,
                b'!' => boolean(node.eval(c)? == 0.),
                _ => node.eval(c)?,
            },
            Self::Binary(op, a, b) => {
                let a = a.eval(c)?;
                match *op {
                    "||" if a != 0. => 1.,
                    "&&" if a == 0. => 0.,
                    op => {
                        let b = b.eval(c)?;
                        match op {
                            "+" => a + b,
                            "-" => a - b,
                            "*" => a * b,
                            "/" => {
                                if b == 0. {
                                    0.
                                } else {
                                    a / b
                                }
                            }
                            "%" => {
                                if b == 0. {
                                    0.
                                } else {
                                    a % b
                                }
                            }
                            "==" => boolean(a == b),
                            "!=" => boolean(a != b),
                            ">" => boolean(a > b),
                            "<" => boolean(a < b),
                            ">=" => boolean(a >= b),
                            "<=" => boolean(a <= b),
                            "||" => boolean(b != 0.),
                            "&&" => boolean(b != 0.),
                            _ => return None,
                        }
                    }
                }
            }
            Self::Call(name, args) => {
                let values: Option<Vec<_>> = args.iter().map(|n| n.eval(c)).collect();
                let v = values?;
                let a = *v.first()?;
                let two = || v.get(1).copied();
                match name.as_str() {
                    "sin" => a.sin(),
                    "cos" => a.cos(),
                    "abs" => a.abs(),
                    "sqrt" => a.max(0.).sqrt(),
                    "round" => (a + 0.5).floor(),
                    "min" => a.min(two()?),
                    "max" => a.max(two()?),
                    "random" => {
                        let mut mixed = c.seed()
                            ^ (if a.is_finite() { a } else { 0. }).to_bits()
                            ^ 0x9e3779b97f4a7c15;
                        mixed ^= mixed >> 30;
                        mixed = mixed.wrapping_mul(0xbf58476d1ce4e5b9);
                        mixed ^= mixed >> 27;
                        mixed = mixed.wrapping_mul(0x94d049bb133111eb);
                        mixed ^= mixed >> 31;
                        (mixed >> 11) as f64 / 9_007_199_254_740_992.
                    }
                    "clamp" => two()?.max(v.get(2)?.min(a)),
                    "lerp" => {
                        let b = two()?;
                        b + (v.get(2)? - b) * a
                    }
                    "smoothstep" => {
                        let b = two()?;
                        let x = *v.get(2)?;
                        let t = if a == b {
                            0.
                        } else {
                            ((x - a) / (b - a)).clamp(0., 1.)
                        };
                        t * t * (3. - 2. * t)
                    }
                    _ => return None,
                }
            }
        })
    }
}
struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    nodes: usize,
    depth: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn take(&mut self, token: &str) -> bool {
        self.space();
        let bytes = token.as_bytes();
        if self.text[self.at..].starts_with(bytes) {
            self.at += bytes.len();
            true
        } else {
            false
        }
    }
    fn expression(&mut self, level: usize) -> Result<Node, String> {
        if level == 6 {
            return self.unary();
        }
        let mut left = self.expression(level + 1)?;
        let operators: &[&'static str] = match level {
            0 => &["||"],
            1 => &["&&"],
            2 => &["==", "!="],
            3 => &[">=", "<=", ">", "<"],
            4 => &["+", "-"],
            _ => &["*", "/", "%"],
        };
        while let Some(op) = operators.iter().copied().find(|op| self.take(op)) {
            let right = self.expression(level + 1)?;
            self.nodes += 1;
            if self.nodes > 512 {
                return Err("Too many expression nodes".into());
            }
            left = Node::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<Node, String> {
        self.depth += 1;
        if self.depth > 128 {
            return Err("Expression nesting exceeds 128".into());
        }
        let result = if self.take("!") {
            Ok(Node::Unary(b'!', Box::new(self.unary()?)))
        } else if self.take("-") {
            Ok(Node::Unary(b'-', Box::new(self.unary()?)))
        } else if self.take("+") {
            self.unary()
        } else {
            self.primary()
        };
        self.depth -= 1;
        result
    }
    fn primary(&mut self) -> Result<Node, String> {
        if self.take("(") {
            let node = self.expression(0)?;
            if !self.take(")") {
                return Err("Missing closing parenthesis".into());
            }
            return Ok(node);
        }
        self.space();
        let start = self.at;
        if self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            while self
                .text
                .get(self.at)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.at += 1;
            }
            let value = std::str::from_utf8(&self.text[start..self.at])
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite())
                .ok_or("Invalid number")?;
            return Ok(Node::Number(value));
        }
        while self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, b'_' | b'.'))
        {
            self.at += 1;
        }
        if self.at == start {
            return Err("Expected value".into());
        }
        let name = String::from_utf8_lossy(&self.text[start..self.at]).to_ascii_lowercase();
        if self.take("(") {
            let mut args = vec![self.expression(0)?];
            while self.take(",") {
                args.push(self.expression(0)?);
                if args.len() > 3 {
                    return Err("Too many function arguments".into());
                }
            }
            if !self.take(")") {
                return Err("Missing function parenthesis".into());
            }
            let count = match name.as_str() {
                "sin" | "cos" | "abs" | "sqrt" | "round" | "random" => 1,
                "min" | "max" => 2,
                "clamp" | "lerp" | "smoothstep" => 3,
                _ => return Err(format!("Unknown function {name}")),
            };
            if args.len() != count {
                return Err(format!("Wrong argument count for {name}"));
            }
            Ok(Node::Call(name, args))
        } else {
            Ok(match name.as_str() {
                "true" => Node::Number(1.),
                "false" => Node::Number(0.),
                "pi" => Node::Number(std::f64::consts::PI),
                _ => Node::Variable(name),
            })
        }
    }
}
#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn boolean_short_circuit_and_numeric_fallbacks() {
        let context = BTreeMap::from([("var.speed".into(), 0.5)]);
        assert_eq!(evaluate("true || missing", &context, -1.), 1.);
        assert_eq!(evaluate("var.speed * 2 >= 1 && !false", &context, -1.), 1.);
        assert_eq!(evaluate("1 / 0 + 7 % 0", &context, -1.), 0.);
        assert_eq!(evaluate("missing", &context, 3.), 3.);
        assert_eq!(evaluate("lerp(0.5, 2, 4)", &context, -1.), 3.);
        assert!(Expression::compile("evil(0)").is_err());
    }
}
