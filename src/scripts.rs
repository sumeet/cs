use super::lang;
use serde_derive::{Serialize,Deserialize};
use super::code_generation;

// doesn't take any arguments, and doesn't return anything
#[derive(Clone, Serialize, Deserialize)]
pub struct Script {
    pub code: lang::Block,
}

impl Script {
    pub fn new() -> Self {
        let mut block = lang::Block::new();
        let null_type = lang::Type::from_spec(&*lang::NULL_TYPESPEC);
        block.expressions.push(
            code_generation::new_placeholder("End of script".to_string(), null_type)
        );
        Self { code: block }
    }

    pub fn code(&self) -> lang::CodeNode {
        lang::CodeNode::Block(self.code.clone())
    }

    pub fn id(&self) -> lang::ID {
        self.code.id
    }
}

