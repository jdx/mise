use evalexpr::{eval_with_context_mut, ContextWithMutableVariables, HashMapContext, Value};
use eyre::{eyre, Result};
use std::collections::HashMap;

// Expression evaluation system using evalexpr
pub fn expr_environment_new() -> ExprEnvironment {
    ExprEnvironment {
        functions: HashMap::new(),
    }
}

pub fn expr_context_default() -> ExprContext {
    ExprContext {
        context: HashMapContext::new(),
    }
}

pub fn expr_compile(expr: &str) -> Result<ExprProgram> {
    // For simple expressions, we can store the expression string
    // evalexpr doesn't require pre-compilation for basic use
    Ok(ExprProgram {
        expression: expr.to_string(),
    })
}

// Type alias to simplify complex function type
type ExprFunction = Box<dyn Fn(&ExprCallContext) -> Result<ExprValue, Box<dyn std::error::Error>>>;

// Expression evaluation types
pub struct ExprEnvironment {
    functions: HashMap<String, ExprFunction>,
}

pub struct ExprContext {
    context: HashMapContext,
}

pub struct ExprProgram {
    expression: String,
}

impl ExprEnvironment {
    pub fn add_function<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&ExprCallContext) -> Result<ExprValue, Box<dyn std::error::Error>> + 'static,
    {
        self.functions.insert(name.to_string(), Box::new(func));
    }

    pub fn run(&self, program: ExprProgram, ctx: &ExprContext) -> Result<ExprValue> {
        // For basic expressions, evaluate directly with the context
        let result = eval_with_context_mut(&program.expression, &mut ctx.context.clone())
            .map_err(|e| eyre!("Expression evaluation failed: {}", e))?;

        match result {
            Value::Boolean(b) => Ok(ExprValue::Bool(b)),
            Value::String(s) => Ok(ExprValue::String(s)),
            Value::Int(i) => Ok(ExprValue::String(i.to_string())),
            Value::Float(f) => Ok(ExprValue::String(f.to_string())),
            _ => Err(eyre!("Unsupported expression result type")),
        }
    }
}

impl ExprContext {
    pub fn insert(&mut self, key: &str, value: &str) {
        let _ = self
            .context
            .set_value(key.to_string(), Value::String(value.to_string()));
        // If setting fails, we can ignore for now
    }
}

pub struct ExprCallContext {
    pub args: Vec<ExprValue>,
}

#[derive(Clone)]
pub enum ExprValue {
    Bool(bool),
    #[allow(dead_code)]
    String(String),
}

impl ExprValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ExprValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            ExprValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl From<bool> for ExprValue {
    fn from(b: bool) -> Self {
        ExprValue::Bool(b)
    }
}

impl From<String> for ExprValue {
    fn from(s: String) -> Self {
        ExprValue::String(s)
    }
}
