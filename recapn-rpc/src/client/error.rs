use crate::Error;

pub struct BrokenClient {
    error: Option<Error>,
}