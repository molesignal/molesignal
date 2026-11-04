// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组合规则：用 `&&`、`||`、`!` 和括号把别的规则组合起来，一条规则有进行中的事件就算真。表达式里
//! 用 A–Z 的字母指代规则，`rules` 写明每个字母是哪一条。被组合的规则不能本身也是组合规则，这样
//! 不会成环。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id};

/// 一条组合规则最多组合几条规则。
pub const MAX_COMPOSITE_RULES: usize = 10;

/// 表达式最长多少个字符。
pub const MAX_COMPOSITE_EXPRESSION_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeQuery {
    /// 如 `A && (B || !C)`。
    pub expression: String,
    /// 字母 → 规则。
    pub rules: BTreeMap<String, Id>,
    /// 开了事件以后，把被组合的规则进行中的事件并到它下面。
    #[serde(default)]
    pub merge_children: bool,
}

impl CompositeQuery {
    /// 规整并检查：表达式说得通；用到的字母都写了是哪条规则，写了的都用到；规则不重复、不是
    /// `own_id` 自己。返回解析好的表达式。
    pub fn validate(&mut self, own_id: Option<&Id>) -> Result<Expr> {
        self.expression = self.expression.trim().to_string();

        if self.expression.chars().count() > MAX_COMPOSITE_EXPRESSION_CHARS {
            return Err(Error::invalid(format!(
                "a composite expression is at most {MAX_COMPOSITE_EXPRESSION_CHARS} characters"
            )));
        }

        let expr = Expr::parse(&self.expression)?;
        let mut used = Vec::new();
        expr.letters(&mut used);

        self.rules = std::mem::take(&mut self.rules)
            .into_iter()
            .map(|(letter, id)| (letter.trim().to_ascii_uppercase(), id))
            .collect();

        if self.rules.len() > MAX_COMPOSITE_RULES {
            return Err(Error::invalid(format!(
                "a composite rule combines at most {MAX_COMPOSITE_RULES} rules"
            )));
        }

        for letter in &used {
            if !self.rules.contains_key(&letter.to_string()) {
                return Err(Error::invalid(format!(
                    "the composite expression uses {letter} without naming its rule"
                )));
            }
        }

        let mut seen: Vec<&Id> = Vec::new();

        for (letter, id) in &self.rules {
            let mut chars = letter.chars();

            let (Some(name), None) = (chars.next(), chars.next()) else {
                return Err(Error::invalid(format!(
                    "composite rules are named by one letter A to Z, not `{letter}`"
                )));
            };

            if !name.is_ascii_uppercase() {
                return Err(Error::invalid(format!(
                    "composite rules are named by one letter A to Z, not `{letter}`"
                )));
            }

            if !used.contains(&name) {
                return Err(Error::invalid(format!(
                    "rule {letter} is named but not used in the expression"
                )));
            }

            if id.as_str().trim().is_empty() {
                return Err(Error::invalid(format!("rule {letter} names no rule")));
            }

            if own_id == Some(id) {
                return Err(Error::invalid("a composite rule cannot combine itself"));
            }

            if seen.contains(&id) {
                return Err(Error::invalid("a composite rule names each rule once"));
            }

            seen.push(id);
        }

        Ok(expr)
    }

    /// 表达式在 `active`（哪些规则有进行中的事件）之下是不是真。表达式坏了算假。
    pub fn holds(&self, active: impl Fn(&Id) -> bool) -> bool {
        let Ok(expr) = Expr::parse(&self.expression) else {
            return false;
        };

        expr.evaluate(&|letter| self.rules.get(&letter.to_string()).is_some_and(&active))
    }
}

/// 解析好的组合表达式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Rule(char),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Rule(char),
    Not,
    And,
    Or,
    Open,
    Close,
}

impl Expr {
    /// `||` 优先级最低，其次 `&&`，`!` 最高；括号改变优先级。
    pub fn parse(text: &str) -> Result<Self> {
        let tokens = tokenize(text)?;

        if tokens.is_empty() {
            return Err(Error::invalid("the composite expression is empty"));
        }

        let mut parser = Parser {
            tokens: &tokens,
            position: 0,
        };
        let expr = parser.or()?;

        if parser.position != tokens.len() {
            return Err(Error::invalid(
                "the composite expression has something left after its end",
            ));
        }

        Ok(expr)
    }

    pub fn evaluate(&self, truth: &impl Fn(char) -> bool) -> bool {
        match self {
            Self::Rule(letter) => truth(*letter),
            Self::Not(inner) => !inner.evaluate(truth),
            Self::And(left, right) => left.evaluate(truth) && right.evaluate(truth),
            Self::Or(left, right) => left.evaluate(truth) || right.evaluate(truth),
        }
    }

    /// 用到的字母，按出现的顺序，不重复。
    pub fn letters(&self, out: &mut Vec<char>) {
        match self {
            Self::Rule(letter) => {
                if !out.contains(letter) {
                    out.push(*letter);
                }
            }
            Self::Not(inner) => inner.letters(out),
            Self::And(left, right) | Self::Or(left, right) => {
                left.letters(out);
                right.letters(out);
            }
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        let token = match c {
            ' ' | '\t' | '\n' | '\r' => continue,
            '(' => Token::Open,
            ')' => Token::Close,
            '!' => Token::Not,
            '&' if chars.next_if_eq(&'&').is_some() => Token::And,
            '|' if chars.next_if_eq(&'|').is_some() => Token::Or,
            c if c.is_ascii_alphabetic() => {
                if chars.peek().is_some_and(char::is_ascii_alphanumeric) {
                    return Err(Error::invalid(
                        "composite rules are named by one letter A to Z",
                    ));
                }

                Token::Rule(c.to_ascii_uppercase())
            }
            other => {
                return Err(Error::invalid(format!(
                    "the composite expression cannot hold `{other}`: use letters, &&, ||, ! and \
                     brackets"
                )));
            }
        };

        tokens.push(token);
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl Parser<'_> {
    fn next_if(&mut self, token: Token) -> bool {
        if self.tokens.get(self.position) == Some(&token) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<Expr> {
        let mut expr = self.and()?;

        while self.next_if(Token::Or) {
            expr = Expr::Or(Box::new(expr), Box::new(self.and()?));
        }

        Ok(expr)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut expr = self.unary()?;

        while self.next_if(Token::And) {
            expr = Expr::And(Box::new(expr), Box::new(self.unary()?));
        }

        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.next_if(Token::Not) {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }

        match self.tokens.get(self.position).copied() {
            Some(Token::Rule(letter)) => {
                self.position += 1;
                Ok(Expr::Rule(letter))
            }
            Some(Token::Open) => {
                self.position += 1;
                let expr = self.or()?;

                if !self.next_if(Token::Close) {
                    return Err(Error::invalid(
                        "the composite expression opens a bracket it does not close",
                    ));
                }

                Ok(expr)
            }
            _ => Err(Error::invalid(
                "the composite expression is missing a rule where one is expected",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(expression: &str, letters: &[(&str, &str)]) -> CompositeQuery {
        CompositeQuery {
            expression: expression.into(),
            rules: letters
                .iter()
                .map(|(letter, id)| ((*letter).to_string(), Id::from_string(*id)))
                .collect(),
            merge_children: false,
        }
    }

    #[test]
    fn and_binds_tighter_than_or_and_not_tightest() {
        let expr = Expr::parse("A || B && !C").unwrap();

        assert_eq!(
            expr,
            Expr::Or(
                Box::new(Expr::Rule('A')),
                Box::new(Expr::And(
                    Box::new(Expr::Rule('B')),
                    Box::new(Expr::Not(Box::new(Expr::Rule('C'))))
                ))
            )
        );

        let truth = |active: &'static [char]| move |letter: char| active.contains(&letter);
        assert!(expr.evaluate(&truth(&['B'])));
        assert!(!expr.evaluate(&truth(&['B', 'C'])));
        assert!(
            Expr::parse("(A || B) && !C")
                .unwrap()
                .evaluate(&truth(&['A']))
        );
    }

    #[test]
    fn broken_expressions_are_refused() {
        for text in ["", "A &&", "A & B", "(A || B", "A B", "AB", "A || 1", ")A("] {
            assert!(Expr::parse(text).is_err(), "{text} should not parse");
        }
    }

    #[test]
    fn every_letter_names_one_rule_used_once_and_not_itself() {
        let mut fine = query("a && !B", &[("A", "r1"), ("b", "r2")]);
        assert!(fine.validate(Some(&Id::from_string("me"))).is_ok());
        assert!(fine.rules.contains_key("B"));

        assert!(query("A && B", &[("A", "r1")]).validate(None).is_err());
        assert!(
            query("A", &[("A", "r1"), ("B", "r2")])
                .validate(None)
                .is_err()
        );
        assert!(
            query("A || B", &[("A", "r1"), ("B", "r1")])
                .validate(None)
                .is_err()
        );
        assert!(
            query("A", &[("A", "me")])
                .validate(Some(&Id::from_string("me")))
                .is_err()
        );
    }

    #[test]
    fn a_composite_holds_by_which_rules_are_active() {
        let composite = query("A && !B", &[("A", "r1"), ("B", "r2")]);

        assert!(composite.holds(|id| id.as_str() == "r1"));
        assert!(!composite.holds(|_| true));
        assert!(!composite.holds(|_| false));
    }
}
