use std::collections::HashSet;

use super::nodeset::Nodeset;

use self::LiteralValue::*;

use super::XPathEvaluationContext;
use super::XPathValue;
use super::XPathValue::{Boolean,Number,Nodes};
use super::StringValue;

use super::axis::XPathAxis;
use super::node_test::XPathNodeTest;

// TODO: Figure out how to use HOFs to get rid of returning a Box here
// all the time.

pub trait XPathExpression {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d>;
}

pub type SubExpression = Box<XPathExpression + 'static>;

macro_rules! binary_constructor(
    ($t:ident) => (
        impl $t {
            pub fn new(left: SubExpression, right: SubExpression) -> SubExpression {
                box $t{left: left, right: right} as SubExpression
            }
        }
    );
)

pub struct ExpressionAnd {
    pub left:  SubExpression,
    pub right: SubExpression,
}

binary_constructor!(ExpressionAnd)

impl XPathExpression for ExpressionAnd {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        Boolean(self.left.evaluate(context).boolean() &&
                self.right.evaluate(context).boolean())
    }
}

pub struct ExpressionContextNode;

impl XPathExpression for ExpressionContextNode {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        Nodes(nodeset![context.node])
    }
}

pub struct ExpressionEqual {
    pub left:  SubExpression,
    pub right: SubExpression,
}

binary_constructor!(ExpressionEqual)

impl ExpressionEqual {
    fn boolean_evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> bool {
        let left_val = self.left.evaluate(context);
        let right_val = self.right.evaluate(context);

        fn str_vals(nodes: &Nodeset) -> HashSet<String> {
            nodes.iter().map(|n| n.string_value()).collect()
        }

        fn num_vals(nodes: &Nodeset) -> Vec<f64> {
            // f64 isn't hashable...
            nodes
                .iter()
                .map(|n| XPathValue::String(n.string_value()).number())
                .collect()
        }

        match (&left_val, &right_val) {
            (&Nodes(ref left_nodes), &Nodes(ref right_nodes)) => {
                let left_strings = str_vals(left_nodes);
                let right_strings = str_vals(right_nodes);
                !left_strings.is_disjoint(&right_strings)
            },
            (&Nodes(ref nodes), &Number(val)) |
            (&Number(val), &Nodes(ref nodes)) => {
                let numbers = num_vals(nodes);
                numbers.iter().any(|n| *n == val)
            },
            (&Nodes(ref nodes), &XPathValue::String(ref val)) |
            (&XPathValue::String(ref val), &Nodes(ref nodes)) => {
                let strings = str_vals(nodes);
                strings.contains(val)
            },
            (&Boolean(_), _) |
            (_, &Boolean(_)) => left_val.boolean() == right_val.boolean(),
            (&Number(_), _) |
            (_, &Number(_)) => left_val.number() == right_val.number(),
            _ => left_val.string() == right_val.string()
        }
    }
}

impl XPathExpression for ExpressionEqual {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        Boolean(self.boolean_evaluate(context))
    }
}

pub struct ExpressionNotEqual {
    equal: ExpressionEqual,
}

impl ExpressionNotEqual {
    pub fn new(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionNotEqual {
            equal: ExpressionEqual{left: left, right: right}
        }
    }
}

impl XPathExpression for ExpressionNotEqual {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        Boolean(!self.equal.boolean_evaluate(context))
    }
}

pub struct ExpressionFunction {
    pub name: String,
    pub arguments: Vec<SubExpression>,
}

impl XPathExpression for ExpressionFunction {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        match context.function_for_name(self.name.as_slice()) {
            Some(fun) => {
                // TODO: Error when argument count mismatch
                let args = self.arguments.iter().map(|ref arg| arg.evaluate(context)).collect();

                fun.evaluate(context, args)
            },
            None => panic!("throw UnknownXPathFunctionException(_name)"),
        }
    }
}

pub enum LiteralValue {
    BooleanLiteral(bool),
    NumberLiteral(f64),
    StringLiteral(String),
}

pub struct ExpressionLiteral {
    pub value: LiteralValue,
}

impl XPathExpression for ExpressionLiteral {
    fn evaluate<'a, 'd>(&self, _: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        match &self.value {
            &BooleanLiteral(b) => Boolean(b),
            &NumberLiteral(b) => Number(b),
            &StringLiteral(ref b) => XPathValue::String(b.clone()),
        }
    }
}

pub struct ExpressionMath {
    left:  SubExpression,
    right: SubExpression,
    operation: fn(f64, f64) -> f64,
}

fn      add(a: f64, b: f64) -> f64 {a + b}
fn subtract(a: f64, b: f64) -> f64 {a - b}
fn multiply(a: f64, b: f64) -> f64 {a * b}
fn   divide(a: f64, b: f64) -> f64 {a / b}
fn  modulus(a: f64, b: f64) -> f64 {a % b}

impl ExpressionMath {
    pub fn addition(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionMath{left: left, right: right, operation: add}
    }

    pub fn subtraction(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionMath{left: left, right: right, operation: subtract}
    }

    pub fn multiplication(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionMath{left: left, right: right, operation: multiply}
    }

    pub fn division(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionMath{left: left, right: right, operation: divide}
    }

    pub fn remainder(left: SubExpression, right: SubExpression) -> SubExpression {
        box ExpressionMath{left: left, right: right, operation: modulus}
    }
}

impl XPathExpression for ExpressionMath {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let left = self.left.evaluate(context);
        let right = self.right.evaluate(context);
        let op = self.operation;
        return Number(op(left.number(), right.number()));
    }
}

pub struct ExpressionNegation {
    pub expression: SubExpression,
}

impl XPathExpression for ExpressionNegation {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let result = self.expression.evaluate(context);
        return Number(-result.number());
    }
}

pub struct ExpressionOr {
    left:  SubExpression,
    right: SubExpression,
}

binary_constructor!(ExpressionOr)

impl XPathExpression for ExpressionOr {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        return Boolean(self.left.evaluate(context).boolean() ||
                       self.right.evaluate(context).boolean())
    }
}

pub struct ExpressionPath {
    start_point: SubExpression,
    steps: Vec<SubExpression>,
}

impl ExpressionPath {
    pub fn new(start_point: SubExpression, steps: Vec<SubExpression>) -> SubExpression {
        box ExpressionPath {start_point: start_point, steps: steps}
    }
}

impl XPathExpression for ExpressionPath {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let mut result = self.start_point.evaluate(context).nodeset();

        for step in self.steps.iter() {
            let mut step_result = Nodeset::new();

            let mut sub_context = context.new_context_for(result.size());

            for current_node in result.iter() {
                sub_context.next(*current_node);
                let selected = step.evaluate(&sub_context);
                // TODO: What if it is not a nodeset?
                step_result.add_nodeset(&selected.nodeset());
            }

            result = step_result;
        }

        Nodes(result)
    }
}

pub struct ExpressionPredicate {
    node_selector: SubExpression,
    predicate: SubExpression,
}

impl ExpressionPredicate {
    pub fn new(node_selector: SubExpression, predicate: SubExpression) -> SubExpression {
        box ExpressionPredicate { node_selector: node_selector, predicate: predicate }
    }

    fn include<'a, 'd>(value: &XPathValue, context: &XPathEvaluationContext<'a, 'd>) -> bool {
        match value {
            &Number(v) => context.position() == v as uint,
            _ => value.boolean()
        }
    }
}

impl XPathExpression for ExpressionPredicate {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let mut selected = Nodeset::new();

        let nodes = self.node_selector.evaluate(context).nodeset();

        let mut sub_context = context.new_context_for(nodes.size());

        for current_node in nodes.iter() {
            sub_context.next(*current_node);

            let value = self.predicate.evaluate(&sub_context);

            if ExpressionPredicate::include(&value, &sub_context) {
                selected.add(*current_node);
            }
        }

        Nodes(selected)
    }
}

pub struct ExpressionRelational {
    pub  left: SubExpression,
    pub right: SubExpression,
    pub operation: fn(f64, f64) -> bool,
}

fn             less_than(left: f64, right: f64) -> bool { left <  right }
fn    less_than_or_equal(left: f64, right: f64) -> bool { left <= right }
fn          greater_than(left: f64, right: f64) -> bool { left >  right }
fn greater_than_or_equal(left: f64, right: f64) -> bool { left >= right }

impl ExpressionRelational {
    pub fn less_than(left: SubExpression, right: SubExpression) -> SubExpression
    {
        box ExpressionRelational{left: left, right: right, operation: less_than}
    }

    pub fn less_than_or_equal(left: SubExpression, right: SubExpression) -> SubExpression
    {
        box ExpressionRelational{left: left, right: right, operation: less_than_or_equal}
    }

    pub fn greater_than(left: SubExpression, right: SubExpression) -> SubExpression
    {
        box ExpressionRelational{left: left, right: right, operation: greater_than}
    }

    pub fn greater_than_or_equal(left: SubExpression, right: SubExpression) -> SubExpression
    {
        box ExpressionRelational{left: left, right: right, operation: greater_than_or_equal}
    }
}

impl XPathExpression for ExpressionRelational {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let left_val = self.left.evaluate(context);
        let right_val = self.right.evaluate(context);
        let op = self.operation;
        Boolean(op(left_val.number(), right_val.number()))
    }
}

pub struct ExpressionRootNode;

impl XPathExpression for ExpressionRootNode {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        Nodes(nodeset![context.node.document().root()])
    }
}

pub type StepAxis = Box<XPathAxis + 'static>;
pub type StepTest = Box<XPathNodeTest + 'static>;

pub struct ExpressionStep {
    axis: StepAxis,
    node_test: StepTest,
}

impl ExpressionStep {
    pub fn new(axis: StepAxis, node_test: StepTest) -> SubExpression {
        box ExpressionStep {axis: axis, node_test: node_test}
    }
}

impl XPathExpression for ExpressionStep {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let mut result = Nodeset::new();
        self.axis.select_nodes(context, &*self.node_test, &mut result);
        Nodes(result)
    }
}

pub struct ExpressionUnion {
    pub left:  SubExpression,
    pub right: SubExpression,
}

binary_constructor!(ExpressionUnion)

impl XPathExpression for ExpressionUnion {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        let mut left_val = self.left.evaluate(context).nodeset();
        let right_val = self.right.evaluate(context).nodeset();
        left_val.add_nodeset(&right_val);
        Nodes(left_val)
    }
}

pub struct ExpressionVariable {
    pub name: String,
}

impl XPathExpression for ExpressionVariable {
    fn evaluate<'a, 'd>(&self, context: &XPathEvaluationContext<'a, 'd>) -> XPathValue<'d> {
        match context.value_of(self.name.as_slice()) {
            Some(v) => v.clone(),
            None => panic!("throw UnknownVariableException(_name)"),
        }
    }
}
