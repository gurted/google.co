use thiserror::Error;

pub const MAX_MESSAGE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

#[derive(Debug, Error)]
pub enum LimitError {
    #[error("message too large: {actual} bytes (max {max})")]
    TooLarge { max: usize, actual: usize },
}

pub type LimitResult<T> = Result<T, LimitError>;

pub fn enforce_max_message_size(len: usize) -> LimitResult<()> {
    if len > MAX_MESSAGE_BYTES {
        return Err(LimitError::TooLarge { max: MAX_MESSAGE_BYTES, actual: len });
    }
    Ok(())
}

