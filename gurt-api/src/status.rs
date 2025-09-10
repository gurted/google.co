#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    Ok,
    BadRequest,
    TooManyRequests,
    RequestEntityTooLarge,
    InternalServerError,
}

impl StatusCode {
    pub fn as_u16(self) -> u16 {
        match self {
            StatusCode::Ok => 200,
            StatusCode::BadRequest => 400,
            StatusCode::TooManyRequests => 429,
            StatusCode::RequestEntityTooLarge => 413,
            StatusCode::InternalServerError => 500,
        }
    }
}
