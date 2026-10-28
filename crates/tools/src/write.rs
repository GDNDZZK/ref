use crate::*;

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str { "write" }
    fn description(&self) -> String { todo!() }
    fn parameters(&self) -> Value { todo!() }
    async fn run(&self, _input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> { todo!() }
}
