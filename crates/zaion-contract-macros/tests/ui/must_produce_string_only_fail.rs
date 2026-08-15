use zaion_contract_macros::must_produce;

pub struct ToolError;

#[must_produce(ToolReceipt)]
impl StableExecutor {
    fn execute(&self) -> Result<(), ToolError> {
        let _ = "ToolReceipt";
        Ok(())
    }
}

pub struct StableExecutor;

fn main() {}
