use evalexpr::{
    eval_with_context_mut, ContextWithMutableVariables, HashMapContext, Value as EvalValue,
};
use eyre::{eyre, Result};
use std::collections::HashMap;

// Expression evaluation types matching the old expr crate API
pub struct Environment {
    functions: HashMap<String, Box<dyn Fn(&CallContext) -> Result<Value, String>>>,
}

pub struct Context {
    variables: HashMap<String, String>,
}

pub struct Program {
    expression: String,
}

pub struct CallContext {
    pub args: Vec<Value>,
}

#[derive(Clone)]
pub enum Value {
    Bool(bool),
    String(String),
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            functions: HashMap::new(),
        }
    }

    pub fn add_function<F>(&mut self, name: &str, func: F)
    where
        F: Fn(&CallContext) -> Result<Value, String> + 'static,
    {
        self.functions.insert(name.to_string(), Box::new(func));
    }

    pub fn run(&self, program: Program, ctx: &Context) -> Result<Value, String> {
        // Set up evalexpr context
        let mut eval_ctx = HashMapContext::new();

        for (key, value) in &ctx.variables {
            let _ = eval_ctx.set_value(key.clone(), EvalValue::String(value.clone()));
        }

        // For now, just evaluate basic expressions
        // TODO: Add support for custom functions
        match eval_with_context_mut(&program.expression, &mut eval_ctx) {
            Ok(EvalValue::Boolean(b)) => Ok(Value::Bool(b)),
            Ok(EvalValue::String(s)) => Ok(Value::String(s)),
            Ok(EvalValue::Int(i)) => Ok(Value::String(i.to_string())),
            Ok(EvalValue::Float(f)) => Ok(Value::String(f.to_string())),
            Ok(_) => Err("Unsupported expression result type".to_string()),
            Err(e) => Err(format!("Expression evaluation failed: {}", e)),
        }
    }

    pub fn eval(&self, expr: &str, ctx: &Context) -> Result<Value, String> {
        let program = Program {
            expression: expr.to_string(),
        };
        self.run(program, ctx)
    }
}

impl Context {
    pub fn default() -> Self {
        Context {
            variables: HashMap::new(),
        }
    }

    pub fn insert<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.variables.insert(key.into(), value.into());
    }

    pub fn extend(&mut self, other: HashMap<String, String>) {
        self.variables.extend(other);
    }
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}
