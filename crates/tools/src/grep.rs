use crate::*;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> String { todo!() }
    fn parameters(&self) -> Value { todo!() }
    async fn run(&self, _input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> { todo!() }
}
