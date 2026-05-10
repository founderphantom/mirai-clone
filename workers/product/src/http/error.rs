use serde::Serialize;
use worker::{Response, Result as WorkerResult};

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self {
            status: 401,
            code: "unauthorized",
            message: "Sign in to continue.".to_string(),
        }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code,
            message: message.into(),
        }
    }

    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code,
            message: message.into(),
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 409,
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            code: "internal_error",
            message: message.into(),
        }
    }

    pub fn to_response(&self) -> WorkerResult<Response> {
        #[derive(Serialize)]
        struct Body<'a> {
            error: ErrorBody<'a>,
        }

        #[derive(Serialize)]
        struct ErrorBody<'a> {
            code: &'a str,
            message: &'a str,
        }

        let mut response = Response::from_json(&Body {
            error: ErrorBody {
                code: self.code,
                message: &self.message,
            },
        })?;
        response = response.with_status(self.status);
        Ok(response)
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
