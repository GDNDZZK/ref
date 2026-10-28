use crate::*;

pub struct FindTool;

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str { "find" }
    fn description(&self) -> String { todo!() }
    fn parameters(&self) -> Value { todo!() }
    async fn run(&self, _input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> { todo!() }
}
