//! Expressions for `//calc` and `//generate`: numbers, variables, `+ - * /
//! % ^`, comparisons, `&& || !`, and `sin cos tan asin acos atan atan2 sqrt
//! cbrt abs floor ceil round min max pow exp ln log10 sign`. True is 1.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    /// `<= >= == != && ||` as two characters.
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

pub fn parse(text: &str) -> Result<Expr, String> {
    let mut p = Parser {
        s: text.chars().collect(),
        i: 0,
    };
    let e = p.or()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("Unexpected `{}` in the expression", p.s[p.i..].iter().collect::<String>()));
    }
    Ok(e)
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn eat(&mut self, op: &str) -> bool {
        self.ws();
        let chars: Vec<char> = op.chars().collect();
        if self.s.get(self.i..self.i + chars.len()) == Some(&chars[..]) {
            self.i += chars.len();
            true
        } else {
            false
        }
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.eat("||") {
            l = Expr::Cmp("||", Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.cmp()?;
        while self.eat("&&") {
            l = Expr::Cmp("&&", Box::new(l), Box::new(self.cmp()?));
        }
        Ok(l)
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let l = self.add()?;
        for op in ["<=", ">=", "==", "!=", "<", ">"] {
            if self.eat(op) {
                let r = self.add()?;
                let op: &'static str = match op {
                    "<=" => "<=",
                    ">=" => ">=",
                    "==" => "==",
                    "!=" => "!=",
                    "<" => "<",
                    _ => ">",
                };
                return Ok(Expr::Cmp(op, Box::new(l), Box::new(r)));
            }
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<Expr, String> {
        let mut l = self.mul()?;
        loop {
            if self.eat("+") {
                l = Expr::Bin('+', Box::new(l), Box::new(self.mul()?));
            } else if self.eat("-") {
                l = Expr::Bin('-', Box::new(l), Box::new(self.mul()?));
            } else {
                return Ok(l);
            }
        }
    }
    fn mul(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.eat("*") {
                '*'
            } else if self.eat("/") {
                '/'
            } else if self.eat("%") {
                '%'
            } else {
                return Ok(l);
            };
            l = Expr::Bin(op, Box::new(l), Box::new(self.unary()?));
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        let base = self.atom()?;
        if self.eat("^") {
            return Ok(Expr::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Expr, String> {
        self.ws();
        if self.eat("(") {
            let e = self.or()?;
            if !self.eat(")") {
                return Err("Missing `)`".into());
            }
            return Ok(e);
        }
        let start = self.i;
        if self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                self.i += 1;
            }
            let text: String = self.s[start..self.i].iter().collect();
            return text.parse().map(Expr::Num).map_err(|_| format!("Bad number `{text}`"));
        }
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
            self.i += 1;
        }
        if start == self.i {
            return Err("Expected a number, a name or `(`".into());
        }
        let name: String = self.s[start..self.i].iter().collect();
        if self.eat("(") {
            let mut args = Vec::new();
            if !self.eat(")") {
                loop {
                    args.push(self.or()?);
                    if self.eat(")") {
                        break;
                    }
                    if !self.eat(",") {
                        return Err(format!("Expected `,` or `)` in {name}(...)"));
                    }
                }
            }
            return Ok(Expr::Call(name, args));
        }
        Ok(Expr::Var(name))
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

impl Expr {
    pub fn eval(&self, vars: &HashMap<&str, f64>) -> Result<f64, String> {
        Ok(match self {
            Expr::Num(n) => *n,
            Expr::Var(v) => match v.as_str() {
                "pi" => std::f64::consts::PI,
                "e" => std::f64::consts::E,
                _ => *vars.get(v.as_str()).ok_or_else(|| format!("Unknown variable `{v}`"))?,
            },
            Expr::Neg(e) => -e.eval(vars)?,
            Expr::Not(e) => truth(e.eval(vars)? <= 0.0),
            Expr::Bin(op, l, r) => {
                let (a, b) = (l.eval(vars)?, r.eval(vars)?);
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    '%' => a % b,
                    _ => a.powf(b),
                }
            }
            Expr::Cmp(op, l, r) => {
                let a = l.eval(vars)?;
                if *op == "&&" && a <= 0.0 {
                    return Ok(0.0);
                }
                if *op == "||" && a > 0.0 {
                    return Ok(1.0);
                }
                let b = r.eval(vars)?;
                truth(match *op {
                    "<" => a < b,
                    ">" => a > b,
                    "<=" => a <= b,
                    ">=" => a >= b,
                    "==" => (a - b).abs() < 1e-9,
                    "!=" => (a - b).abs() >= 1e-9,
                    _ => b > 0.0,
                })
            }
            Expr::Call(name, args) => {
                let v: Vec<f64> = args.iter().map(|a| a.eval(vars)).collect::<Result<_, _>>()?;
                let one = |f: fn(f64) -> f64| -> Result<f64, String> {
                    match v.as_slice() {
                        [x] => Ok(f(*x)),
                        _ => Err(format!("{name}() takes one number")),
                    }
                };
                let two = |f: fn(f64, f64) -> f64| -> Result<f64, String> {
                    match v.as_slice() {
                        [x, y] => Ok(f(*x, *y)),
                        _ => Err(format!("{name}() takes two numbers")),
                    }
                };
                match name.as_str() {
                    "sin" => one(f64::sin)?,
                    "cos" => one(f64::cos)?,
                    "tan" => one(f64::tan)?,
                    "asin" => one(f64::asin)?,
                    "acos" => one(f64::acos)?,
                    "atan" => one(f64::atan)?,
                    "sqrt" => one(f64::sqrt)?,
                    "cbrt" => one(f64::cbrt)?,
                    "abs" => one(f64::abs)?,
                    "floor" => one(f64::floor)?,
                    "ceil" => one(f64::ceil)?,
                    "round" => one(f64::round)?,
                    "exp" => one(f64::exp)?,
                    "ln" => one(f64::ln)?,
                    "log10" => one(f64::log10)?,
                    "sign" => one(f64::signum)?,
                    "atan2" => two(f64::atan2)?,
                    "pow" => two(f64::powf)?,
                    "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
                    "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    _ => return Err(format!("Unknown function `{name}`")),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(s: &str) -> f64 {
        parse(s).unwrap().eval(&HashMap::new()).unwrap()
    }

    #[test]
    fn evaluates() {
        assert_eq!(calc("1 + 2 * 3"), 7.0);
        assert_eq!(calc("(1 + 2) * 3"), 9.0);
        assert_eq!(calc("2 ^ 3 ^ 2"), 512.0);
        assert_eq!(calc("-2 ^ 2"), -4.0);
        assert_eq!(calc("max(1, 5, 3) + min(4, 2)"), 7.0);
        assert_eq!(calc("1 < 2 && 3 >= 3"), 1.0);
        assert_eq!(calc("!(1 == 1) || 0"), 0.0);
        assert!((calc("sin(pi / 2)") - 1.0).abs() < 1e-9);
        let vars: HashMap<&str, f64> = [("x", 0.5), ("y", 0.0), ("z", 0.0)].into();
        assert_eq!(parse("x^2 + y^2 + z^2 < 1").unwrap().eval(&vars).unwrap(), 1.0);
        assert!(parse("1 +").is_err());
        assert!(parse("foo(1)").unwrap().eval(&HashMap::new()).is_err());
    }
}
