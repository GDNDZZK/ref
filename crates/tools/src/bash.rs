use crate::*;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }
    fn description(&self) -> String { todo!() }
    fn parameters(&self) -> Value { todo!() }
    async fn run(&self, _input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> { todo!() }
}
