use super::*;

impl AquaPackage {
    /// Set up version filter expression if configured
    pub fn setup_version_filter(&mut self) -> Result<()> {
        if let Some(version_filter) = &self.version_filter {
            self.version_filter_expr = Some(expr::compile(version_filter)?);
        }
        Ok(())
    }

    /// Check if a version passes the version filter
    pub fn version_filter_ok(&self, v: &str) -> Result<bool> {
        if let Some(filter) = &self.version_filter_expr {
            if let Value::Bool(expr) = self.expr(v, filter)? {
                Ok(expr)
            } else {
                log::warn!(
                    "invalid response from version filter: {}",
                    self.version_filter.as_ref().unwrap()
                );
                Ok(true)
            }
        } else {
            Ok(true)
        }
    }

    fn expr(&self, v: &str, program: &Program) -> Result<Value> {
        let expr = self.expr_parser(v);
        expr.run(program, &self.expr_ctx(v)).map_err(|e| eyre!(e))
    }

    pub(super) fn expr_parser(&self, v: &str) -> Environment<'_> {
        let (_, v) = split_version_prefix(v);
        let ver = Versioning::new(v);
        let mut env = Environment::new();
        env.add_function("semver", move |c| {
            if c.args.len() != 1 {
                return Err("semver() takes exactly one argument".to_string().into());
            }
            let requirements = c.args[0]
                .as_string()
                .unwrap()
                .replace(' ', "")
                .split(',')
                .map(AquaRequirement::parse)
                .collect::<Vec<_>>();
            if requirements.iter().any(|r| r.is_none()) {
                return Err("invalid semver requirement".to_string().into());
            }
            if let Some(ver) = &ver {
                Ok(requirements
                    .iter()
                    .all(|r| r.as_ref().is_some_and(|r| r.matches(ver)))
                    .into())
            } else {
                Err("invalid version".to_string().into())
            }
        });
        env
    }

    pub(super) fn expr_ctx(&self, v: &str) -> Context {
        let mut ctx = Context::default();
        ctx.insert("Version", v);
        ctx
    }
}
