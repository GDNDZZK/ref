use crate::*;

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str { "edit" }
    fn description(&self) -> String { todo!() }
    fn parameters(&self) -> Value { todo!() }
    async fn run(&self, _input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> { todo!() }
}
