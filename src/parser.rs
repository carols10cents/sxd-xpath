use std::iter::Peekable;
use std::string;

use self::ParseErr::*;

use super::XPathValue::{String,Number};
use super::token::XPathToken;
use super::tokenizer::{TokenResult,TokenizerErr};
use super::axis::{XPathAxis,SubAxis,PrincipalNodeType};
use super::axis::{
    AxisAttribute,
    AxisChild,
    AxisDescendant,
    AxisDescendantOrSelf,
    AxisParent,
    AxisSelf,
};
use super::expression::{SubExpression,LiteralValue};
use super::expression::{
    ExpressionAnd,
    ExpressionContextNode,
    ExpressionEqual,
    ExpressionFunction,
    ExpressionLiteral,
    ExpressionMath,
    ExpressionNegation,
    ExpressionNotEqual,
    ExpressionOr,
    ExpressionPath,
    ExpressionPredicate,
    ExpressionRelational,
    ExpressionRootNode,
    ExpressionStep,
    ExpressionUnion,
    ExpressionVariable,
};
use super::node_test::SubNodeTest;
use super::node_test::{
    NodeTestAttribute,
    NodeTestElement,
    NodeTestNode,
    NodeTestText,
};

pub struct XPathParser;

impl XPathParser {
    pub fn new() -> XPathParser {
        XPathParser
    }
}

#[deriving(Show,PartialEq,Clone)]
pub enum ParseErr {
    EmptyPredicate,
    ExtraUnparsedTokens,
    InvalidNodeTest(string::String),
    InvalidXPathAxis(string::String),
    RanOutOfInput,
    RightHandSideExpressionMissing,
    TokenizerError(TokenizerErr),
    TrailingSlash,
    UnexpectedToken(XPathToken),
}

pub type ParseResult = Result<Option<SubExpression>, ParseErr>;

type BinaryExpressionBuilder = fn(SubExpression, SubExpression) -> SubExpression;

struct BinaryRule {
    token: XPathToken,
    builder: BinaryExpressionBuilder,
}

struct LeftAssociativeBinaryParser<I> {
    rules: Vec<BinaryRule>,
}

type TokenSource<'a, I> = &'a mut Peekable<TokenResult, I>;

trait XCompat {
    fn has_more_tokens(&mut self) -> bool;
    fn next_token_is(&mut self, token: &XPathToken) -> bool;
    fn consume(&mut self, token: &XPathToken) -> Result<(), ParseErr>;
}

impl<I: Iterator<TokenResult>> XCompat for Peekable<TokenResult, I> {
    fn has_more_tokens(&mut self) -> bool {
        self.peek().is_some()
    }

    fn next_token_is(&mut self, token: &XPathToken) -> bool {
        match self.peek() {
            Some(&Ok(ref t)) => t == token,
            _ => false
        }
    }

    fn consume(&mut self, token: &XPathToken) -> Result<(), ParseErr> {
        match self.next() {
            None => Err(RanOutOfInput),
            Some(Err(x)) => Err(TokenizerError(x)),
            Some(Ok(x)) =>
                if &x == token {
                    Ok(())
                } else {
                    Err(UnexpectedToken(x))
                },
        }
    }
}

/// Similar to `consume`, but can be used when the token carries a
/// single value.
macro_rules! consume_value(
    ($source:expr, XPathToken::$token:ident) => (
        match $source.next() {
            None => return Err(RanOutOfInput),
            Some(Err(x)) => return Err(TokenizerError(x)),
            Some(Ok(XPathToken::$token(x))) => x,
            Some(Ok(x)) => return Err(UnexpectedToken(x)),
        }
    );
)

/// Similar to `next_token_is`, but can be used when the token carries
/// a single value
macro_rules! next_token_is(
    ($source:expr, XPathToken::$token:ident) => (
        match $source.peek() {
            Some(&Ok(XPathToken::$token(_))) => true,
            _ => false,
        }
    );
)

impl<I : Iterator<TokenResult>> LeftAssociativeBinaryParser<I> {
    fn new(rules: Vec<BinaryRule>) -> LeftAssociativeBinaryParser<I> {
        LeftAssociativeBinaryParser {
            rules: rules,
        }
    }

    fn parse(&self,
             source: TokenSource<I>,
             child_parse: |TokenSource<I>| -> ParseResult)
             -> ParseResult {
        let left = try!(child_parse(source));

        let mut left = match left {
            None => return Ok(None),
            Some(x) => x,
        };

        while source.has_more_tokens() {
            let mut found = false;

            for rule in self.rules.iter() {
                if source.next_token_is(&rule.token) {
                    try!(source.consume(&rule.token));

                    let right = try!(child_parse(source));

                    let right = match right {
                        None => return Err(RightHandSideExpressionMissing),
                        Some(x) => x,
                    };

                    left = (rule.builder)(left, right);

                    found = true;
                    break;
                }
            }

            if !found { break; }
        }

        Ok(Some(left))
    }
}

fn first_matching_rule
    <I : Iterator<TokenResult>>
    (child_parses: &mut Vec<|TokenSource<I>| -> ParseResult>,
     source: TokenSource<I>)
     -> ParseResult
{
    for child_parse in child_parses.iter_mut() {
        let expr = try!((*child_parse)(source));
        if expr.is_some() {
            return Ok(expr);
        }
    }

    Ok(None)
}

impl<I : Iterator<TokenResult>> XPathParser {

    fn parse_axis(&self, source: TokenSource<I>) -> Result<SubAxis, ParseErr> {
        if next_token_is!(source, XPathToken::Axis) {
            let name = consume_value!(source, XPathToken::Axis);
            try!(source.consume(&XPathToken::DoubleColon));

            match name.as_slice() {
                // TODO: explicit child axis?
                "self" => Ok(box AxisSelf as SubAxis),
                "parent" => Ok(box AxisParent as SubAxis),
                "descendant" => Ok(box AxisDescendant as SubAxis),
                "descendant-or-self" => Ok(AxisDescendantOrSelf::new()),
                "attribute" => Ok(box AxisAttribute as SubAxis),
                _ => Err(InvalidXPathAxis(name)),
            }
        } else {
            Ok(box AxisChild as SubAxis)
        }
    }

    fn parse_node_test(&self, source: TokenSource<I>) -> Result<Option<SubNodeTest>, ParseErr> {
        if next_token_is!(source, XPathToken::NodeTest) {
            let name = consume_value!(source, XPathToken::NodeTest);

            try!(source.consume(&XPathToken::LeftParen));
            try!(source.consume(&XPathToken::RightParen));

            match name.as_slice() {
                // TODO: explicit element, attribute tests?
                "node" => Ok(Some(box NodeTestNode as SubNodeTest)),
                "text" => Ok(Some(box NodeTestText as SubNodeTest)),
                _ => Err(InvalidNodeTest(name))
            }
        } else {
            Ok(None)
        }
    }

    fn default_node_test(&self, source: TokenSource<I>, axis: &XPathAxis)
                         -> Result<Option<SubNodeTest>,ParseErr>
    {
        if next_token_is!(source, XPathToken::String) {
            let name = consume_value!(source, XPathToken::String);

            match axis.principal_node_type() {
                PrincipalNodeType::Attribute => Ok(Some(box NodeTestAttribute{name: name} as SubNodeTest)),
                PrincipalNodeType::Element => Ok(Some(box NodeTestElement{name: name} as SubNodeTest)),
            }
        } else {
            Ok(None)
        }
    }

    fn parse_variable_reference(&self, source: TokenSource<I>) -> ParseResult {
        if source.next_token_is(&XPathToken::DollarSign) {
            try!(source.consume(&XPathToken::DollarSign));
            let name = consume_value!(source, XPathToken::String);
            Ok(Some(box ExpressionVariable { name: name } as SubExpression))
        } else {
            Ok(None)
        }
    }

    fn parse_string_literal(&self, source: TokenSource<I>) -> ParseResult {
        if next_token_is!(source, XPathToken::Literal) {
            let value = consume_value!(source, XPathToken::Literal);
            Ok(Some(box ExpressionLiteral { value: LiteralValue::StringLiteral(value) } as SubExpression))
        } else {
            Ok(None)
        }
    }

    fn parse_numeric_literal(&self, source: TokenSource<I>) -> ParseResult {
        if next_token_is!(source, XPathToken::Number) {
            let value = consume_value!(source, XPathToken::Number);
            Ok(Some(box ExpressionLiteral { value: LiteralValue::NumberLiteral(value) } as SubExpression))
        } else {
            Ok(None)
        }
    }

    fn parse_function_call(&self, source: TokenSource<I>) -> ParseResult {
        if next_token_is!(source, XPathToken::Function) {
            let name = consume_value!(source, XPathToken::Function);

            let mut arguments = Vec::new();

            try!(source.consume(&XPathToken::LeftParen));
            while ! source.next_token_is(&XPathToken::RightParen) {
                let arg = try!(self.parse_expression(source));
                match arg {
                    Some(arg) => arguments.push(arg),
                    None => break,
                }
            }
            try!(source.consume(&XPathToken::RightParen));

            Ok(Some(box ExpressionFunction{ name: name, arguments: arguments } as SubExpression))
        } else {
            Ok(None)
        }
    }

    fn parse_primary_expression(&self, source: TokenSource<I>) -> ParseResult {
        let mut child_parses = vec![
            |src: TokenSource<I>| self.parse_variable_reference(src),
            |src: TokenSource<I>| self.parse_string_literal(src),
            |src: TokenSource<I>| self.parse_numeric_literal(src),
            |src: TokenSource<I>| self.parse_function_call(src),
        ];

        first_matching_rule(&mut child_parses, source)
    }

    fn parse_predicate_expression(&self, source: TokenSource<I>) -> ParseResult {
        if source.next_token_is(&XPathToken::LeftBracket) {
            try!(source.consume(&XPathToken::LeftBracket));

            match try!(self.parse_expression(source)) {
                Some(predicate) => {
                    try!(source.consume(&XPathToken::RightBracket));
                    Ok(Some(predicate))
                },
                None => Err(EmptyPredicate),
            }
        } else {
            Ok(None)
        }
    }

    fn parse_step(&self, source: TokenSource<I>) -> ParseResult {
        let axis = try!(self.parse_axis(source));

        let node_test = match try!(self.parse_node_test(source)) {
            Some(test) => Some(test),
            None => try!(self.default_node_test(source, &*axis)),
        };

        match node_test {
            Some(test) => Ok(Some(ExpressionStep::new(axis, test))),
            None => Ok(None)
        }
    }

    fn parse_and_add_predicates(&self,
                                source: TokenSource<I>,
                                node_selecting_expr: SubExpression)
                                -> Result<SubExpression,ParseErr>
    {
        let mut predicates = Vec::new();

        loop {
            match try!(self.parse_predicate_expression(source)) {
                Some(predicate) => predicates.push(predicate),
                None => break,
            }
        }

        let wrap = |expr, pred| ExpressionPredicate::new(expr, pred);
        Ok(predicates.into_iter().fold(node_selecting_expr, wrap))
    }

    fn parse_relative_location_path_raw(&self,
                                        source: TokenSource<I>,
                                        start_point: SubExpression) -> ParseResult
    {
        match try!(self.parse_step(source)) {
            Some(step) => {
                let mut steps = Vec::new();

                let step = try!(self.parse_and_add_predicates(source, step));
                steps.push(step);

                while source.next_token_is(&XPathToken::Slash) {
                    try!(source.consume(&XPathToken::Slash));

                    match try!(self.parse_step(source)) {
                        Some(next) => {
                            let next = try!(self.parse_and_add_predicates(source, next));
                            steps.push(next);
                        },
                        None => return Err(TrailingSlash),
                    }
                }

                Ok(Some(ExpressionPath::new(start_point, steps)))
            },
            None => Ok(None),
        }
    }

    fn parse_relative_location_path(&self, source: TokenSource<I>) -> ParseResult {
        let start_point = box ExpressionContextNode;
        self.parse_relative_location_path_raw(source, start_point)
    }

    fn parse_absolute_location_path(&self, source: TokenSource<I>) -> ParseResult {
        if source.next_token_is(&XPathToken::Slash) {
            try!(source.consume(&XPathToken::Slash));

            let start_point = box ExpressionRootNode;
            match try!(self.parse_relative_location_path_raw(source, start_point)) {
                Some(expr) => Ok(Some(expr)),
                None => Ok(Some(box ExpressionRootNode as SubExpression)),
            }
        } else {
            Ok(None)
        }
    }

    fn parse_location_path(&self, source: TokenSource<I>) -> ParseResult {
        let mut child_parses = vec![
            |source: TokenSource<I>| self.parse_relative_location_path(source),
            |source: TokenSource<I>| self.parse_absolute_location_path(source),
        ];

        first_matching_rule(&mut child_parses, source)
    }

    fn parse_filter_expression(&self, source: TokenSource<I>) -> ParseResult {
        match try!(self.parse_primary_expression(source)) {
            Some(expr) => Ok(Some(try!(self.parse_and_add_predicates(source, expr)))),
            None => Ok(None),
        }
    }

    fn parse_path_expression(&self, source: TokenSource<I>) -> ParseResult {
        let expr = try!(self.parse_location_path(source));
        if expr.is_some() {
            return Ok(expr);
        } // TODO: investigate if this is a pattern

        match try!(self.parse_filter_expression(source)) {
            Some(expr) =>
                if source.next_token_is(&XPathToken::Slash) {
                    try!(source.consume(&XPathToken::Slash));

                    match try!(self.parse_relative_location_path_raw(source, expr)) {
                        Some(expr) => Ok(Some(expr)),
                        None => Err(TrailingSlash),
                    }
                } else {
                    Ok(Some(expr))
                },
            None => Ok(None),
        }
    }

    fn parse_union_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::Pipe, builder: ExpressionUnion::new }
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_path_expression(source))
    }

    fn parse_unary_expression(&self, source: TokenSource<I>) -> ParseResult {
        let expr = try!(self.parse_union_expression(source));
        if expr.is_some() {
            return Ok(expr);
        }

        if source.next_token_is(&XPathToken::MinusSign) {
            try!(source.consume(&XPathToken::MinusSign));

            let expr = try!(self.parse_unary_expression(source));

            match expr {
                Some(expr) => {
                    let expr: SubExpression = box ExpressionNegation { expression: expr };
                    Ok(Some(expr))
                },
                None => Err(RightHandSideExpressionMissing),
            }
        } else {
            Ok(None)
        }
    }

    fn parse_multiplicative_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::Multiply,  builder: ExpressionMath::multiplication },
            BinaryRule { token: XPathToken::Divide,    builder: ExpressionMath::division },
            BinaryRule { token: XPathToken::Remainder, builder: ExpressionMath::remainder }
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_unary_expression(source))
    }

    fn parse_additive_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::PlusSign,  builder: ExpressionMath::addition },
            BinaryRule { token: XPathToken::MinusSign, builder: ExpressionMath::subtraction}
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_multiplicative_expression(source))
    }

    fn parse_relational_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::LessThan,
                         builder: ExpressionRelational::less_than },
            BinaryRule { token: XPathToken::LessThanOrEqual,
                         builder: ExpressionRelational::less_than_or_equal },
            BinaryRule { token: XPathToken::GreaterThan,
                         builder: ExpressionRelational::greater_than },
            BinaryRule { token: XPathToken::GreaterThanOrEqual,
                         builder: ExpressionRelational::greater_than_or_equal },
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_additive_expression(source))
    }

    fn parse_equality_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::Equal,    builder: ExpressionEqual::new },
            BinaryRule { token: XPathToken::NotEqual, builder: ExpressionNotEqual::new },
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_relational_expression(source))
    }

    fn parse_and_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::And, builder: ExpressionAnd::new }
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_equality_expression(source))
    }

    fn parse_or_expression(&self, source: TokenSource<I>) -> ParseResult {
        let rules = vec![
            BinaryRule { token: XPathToken::Or, builder: ExpressionOr::new }
        ];

        let parser = LeftAssociativeBinaryParser::new(rules);
        parser.parse(source, |source| self.parse_and_expression(source))
    }

    fn parse_expression(&self, source: TokenSource<I>) -> ParseResult {
        self.parse_or_expression(source)
    }

    pub fn parse(&self, source: I) -> ParseResult {
        let mut source = source.peekable();

        let expr = try!(self.parse_or_expression(&mut source));

        if source.has_more_tokens() {
            return Err(ExtraUnparsedTokens);
        }

        Ok(expr)
    }
}
