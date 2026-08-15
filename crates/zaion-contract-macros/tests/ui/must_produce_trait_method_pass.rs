use zaion_contract_macros::must_produce;

pub struct ToolReceipt;
pub struct ToolError;

#[must_produce(ToolReceipt)]
pub trait ToolExecutor {
    fn execute(&self) -> Result<ToolReceipt, ToolError>;
}

fn main() {}
