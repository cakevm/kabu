use revm::database::DBErrorMarker;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

#[derive(Clone, Default, Debug)]
pub enum KabuDBError {
    #[default]
    Nonimportant,
    TransportError,
    NoDB,
    DatabaseError(String),
}

impl DBErrorMarker for KabuDBError {}

impl Display for KabuDBError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for KabuDBError {}
