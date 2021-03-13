use alloc::fmt;
use std::fmt::Formatter;


#[derive(Debug,Display)]
pub struct Error
{
    message: String
}

impl fmt::Display for Error{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "nucleus error: {:?}",self)
    }
}