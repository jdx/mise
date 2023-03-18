use once_cell::sync::Lazy;
use tera::Context;

use crate::env;

pub static BASE_CONTEXT: Lazy<Context> = Lazy::new(|| {
    let mut context = Context::new();
    context.insert("env", &*env::PRISTINE_ENV);
    context
});
